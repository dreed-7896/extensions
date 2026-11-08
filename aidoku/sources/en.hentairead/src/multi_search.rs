//! `a | b` searches, in the query or a text filter: each alternative is searched on its own
//! and the results are merged.
use aidoku::{
	FilterValue, MangaPageResult, Result,
	alloc::{String, Vec, vec},
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

/// Every combination of the `|` alternatives in the query and in text filters such as Artist,
/// or `None` when nothing uses `|`. At most `MAX_SEARCHES` combinations are searched.
pub fn variants(
	query: Option<&str>,
	filters: &[FilterValue],
) -> Option<Vec<(Option<String>, Vec<FilterValue>)>> {
	let query_terms = terms(query);
	let split_filters = filters.iter().any(|filter| {
		matches!(filter, FilterValue::Text { value, .. } if terms(Some(value)).is_some())
	});
	if query_terms.is_none() && !split_filters {
		return None;
	}
	let queries: Vec<Option<String>> = match query_terms {
		Some(terms) => terms.into_iter().map(Some).collect(),
		None => vec![query.map(String::from)],
	};
	let mut combos: Vec<(Option<String>, Vec<FilterValue>)> =
		queries.into_iter().map(|query| (query, Vec::new())).collect();
	for filter in filters {
		let alternatives = match filter {
			FilterValue::Text { id, value } => match terms(Some(value)) {
				Some(values) => values
					.into_iter()
					.map(|value| FilterValue::Text {
						id: id.clone(),
						value,
					})
					.collect(),
				None => vec![filter.clone()],
			},
			_ => vec![filter.clone()],
		};
		let mut next = Vec::new();
		for (query, chosen) in &combos {
			for alternative in &alternatives {
				let mut chosen = chosen.clone();
				chosen.push(alternative.clone());
				next.push((query.clone(), chosen));
			}
		}
		combos = next;
		combos.truncate(MAX_SEARCHES);
	}
	Some(combos)
}

/// Keeps a search with many `|` alternatives from sending dozens of requests.
pub const MAX_SEARCHES: usize = 16;

/// Runs every term for the same page and interleaves the results without duplicates.
/// A term that fails is skipped unless every term fails.
pub fn search<T, F>(terms: Vec<T>, mut run: F) -> Result<MangaPageResult>
where
	F: FnMut(T) -> Result<MangaPageResult>,
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
	// HentaiRead keys are title slugs, so there is no ID to order by. Each term is searched
	// newest first, and alternating between them keeps the merged list close to date order.
	Ok(MangaPageResult {
		entries,
		has_next_page,
	})
}

