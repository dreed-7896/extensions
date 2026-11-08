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

/// Runs every term for the same page and interleaves the results without duplicates.
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
	Ok(MangaPageResult {
		entries,
		has_next_page,
	})
}
