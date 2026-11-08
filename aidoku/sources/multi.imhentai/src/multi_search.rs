//! `a | b` searches: each term is searched on its own and the results are merged.
use aidoku::{
	MangaPageResult, Result,
	alloc::{String, Vec},
};

/// The separate terms of a query written as `term one | term two`, or `None` for a plain query.
pub fn terms(query: Option<&str>) -> Option<Vec<String>> {
	let query = query?;
	if !query.contains('|') {
		return None;
	}
	let terms: Vec<String> = query
		.split('|')
		.map(str::trim)
		.filter(|term| !term.is_empty())
		.map(String::from)
		.collect();
	if terms.is_empty() { None } else { Some(terms) }
}

/// Runs every term for the same page and merges the results without duplicates, newest first.
/// Gallery IDs grow with upload order on these sites, so the ID stands in for the upload date.
/// A term that fails is skipped unless every term fails.
pub fn search<F>(terms: Vec<String>, mut run: F) -> Result<MangaPageResult>
where
	F: FnMut(String) -> Result<MangaPageResult>,
{
	let mut results = Vec::new();
	let mut first_error = None;
	for term in terms {
		match run(term) {
			Ok(result) => results.push(result),
			Err(error) => {
				if first_error.is_none() {
					first_error = Some(error);
				}
			}
		}
	}
	if results.is_empty()
		&& let Some(error) = first_error
	{
		return Err(error);
	}
	let has_next_page = results.iter().any(|result| result.has_next_page);
	let mut lists: Vec<_> = results
		.into_iter()
		.map(|result| result.entries.into_iter())
		.collect();
	let mut seen: Vec<String> = Vec::new();
	let mut entries = Vec::new();
	loop {
		let mut progressed = false;
		for list in lists.iter_mut() {
			if let Some(manga) = list.next() {
				progressed = true;
				if !seen.contains(&manga.key) {
					seen.push(manga.key.clone());
					entries.push(manga);
				}
			}
		}
		if !progressed {
			break;
		}
	}
	// Stable sort: entries without a numeric ID keep their interleaved order at the end.
	entries.sort_by_key(|manga| core::cmp::Reverse(gallery_id(&manga.key)));
	Ok(MangaPageResult {
		entries,
		has_next_page,
	})
}

/// The numeric gallery ID in a key such as `123456` or `/gallery/123456/`.
fn gallery_id(key: &str) -> Option<u64> {
	let digits: String = key
		.trim_end_matches('/')
		.chars()
		.rev()
		.take_while(|c| c.is_ascii_digit())
		.collect::<Vec<char>>()
		.into_iter()
		.rev()
		.collect();
	digits.parse().ok()
}
