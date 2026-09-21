#![no_std]

use aidoku::{
	alloc::{
		borrow::ToOwned,
		format,
		string::{String, ToString},
		vec,
		vec::Vec,
	},
	helpers::uri::QueryParameters,
	imports::net::Request,
	prelude::*,
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Home, HomeLayout,
	ImageRequestProvider, Listing, ListingProvider, Manga, MangaPageResult, MangaStatus, Page,
	PageContent, PageContext, Result, Source, UpdateStrategy,
};
use base64::{engine::general_purpose::{STANDARD, URL_SAFE}, Engine as _};
use serde_json::Value;

const BASE_URL: &str = "https://hentairead.com";

struct HentaiRead;

impl HentaiRead {
	fn absolute_url(value: &str) -> String {
		if value.starts_with("http://") || value.starts_with("https://") {
			value.into()
		} else if value.starts_with('/') {
			format!("{BASE_URL}{value}")
		} else {
			format!("{BASE_URL}/{value}")
		}
	}

	fn image_url(element: &aidoku::imports::html::Element) -> Option<String> {
		for key in ["data-src", "data-lazy-src", "data-cfsrc"] {
			if let Some(value) = element.attr(key).filter(|value| !value.trim().is_empty()) {
				let value = value.trim();
				if !Self::is_placeholder(value) {
					return Some(Self::absolute_url(value));
				}
			}
		}
		if let Some(srcset) = element.attr("srcset") {
			if let Some(value) = srcset.split_whitespace().next() {
				return Some(Self::absolute_url(value));
			}
		}
		element
			.attr("abs:src")
			.or_else(|| element.attr("src"))
			.filter(|value| !Self::is_placeholder(value))
			.map(|value| Self::absolute_url(value.trim()))
	}

	fn is_placeholder(value: &str) -> bool {
		let value = value.to_ascii_lowercase();
		value.is_empty()
			|| value.contains("placeholder")
			|| value.contains("blank.")
			|| value.contains("spacer")
			|| value.contains("spinner")
			|| value.contains("logo")
			|| value.contains("avatar")
	}

	fn parse_list(document: &aidoku::imports::html::Document) -> Result<MangaPageResult> {
		let entries: Vec<Manga> = document
			.select("div.manga-item")
			.map(|elements| {
				elements
					.filter_map(|element| {
						let link = element.select_first("h3 a[href*='/hentai/']")?;
						let title = link
							.attr("title")
							.filter(|title| !title.trim().is_empty())
							.or_else(|| link.text())?
							.trim()
							.to_owned();
						if title.is_empty() {
							return None;
						}
						let href = link.attr("abs:href").or_else(|| link.attr("href"))?;
						let cover = element
							.select_first("img.manga-item__img-inner, img")
							.and_then(|image| Self::image_url(&image));
						let absolute_url = Self::absolute_url(&href);
						Some(Manga {
							key: absolute_url
								.strip_prefix(BASE_URL)
								.unwrap_or(&href)
								.into(),
							title,
							cover,
							content_rating: ContentRating::NSFW,
							..Default::default()
						})
					})
					.collect()
			})
			.unwrap_or_default();
		let has_next_page = document
			.select_first("a[rel=next], div.nav-previous, a.nextpostslink")
			.is_some();
		if entries.is_empty() {
			bail!("HentaiRead returned no titles. If a Cloudflare challenge is visible, complete it and retry.");
		}
		Ok(MangaPageResult { entries, has_next_page })
	}

	fn browse(order: &str, page: i32) -> Result<MangaPageResult> {
		let path = if page > 1 {
			format!("/hentai/page/{page}/")
		} else {
			"/hentai/".into()
		};
		Self::parse_list(&Request::get(format!("{BASE_URL}{path}?sortby={order}"))?.html()?)
	}

	fn get_term_id(term: &str, taxonomy: &str) -> Result<String> {
		let taxonomy = if taxonomy == "artist" { "manga_artist" } else { taxonomy };
		let mut params = QueryParameters::new();
		params.push("action", Some("search_manga_terms"));
		params.push("search", Some(term));
		params.push("taxonomy", Some(taxonomy));
		let body = Request::get(format!("{BASE_URL}/wp-admin/admin-ajax.php?{params}"))?.string()?;
		let value: Value = serde_json::from_str(&body)
			.map_err(|_| aidoku::AidokuError::message("Invalid filter response"))?;
		let item = value
			.get("results")
			.and_then(Value::as_array)
			.and_then(|items| {
				items.iter().find(|item| {
					item.get("text")
						.and_then(Value::as_str)
						.map(|text| text.eq_ignore_ascii_case(term))
						.unwrap_or(false)
				})
			});
		let Some(id) = item.and_then(|item| item.get("id")) else {
			bail!("Filter value not found: {term}");
		};
		if let Some(id) = id.as_i64() {
			Ok(id.to_string())
		} else if let Some(id) = id.as_str() {
			Ok(id.to_owned())
		} else {
			bail!("Invalid filter value: {term}")
		}
	}

	fn add_term_filters(
		params: &mut QueryParameters,
		value: &str,
		taxonomy: &str,
		include_key: &str,
		exclude_key: Option<&str>,
	) -> Result<()> {
		for term in value.split(',').map(str::trim).filter(|term| !term.is_empty()) {
			let excluded = term.starts_with('-');
			let term = term.trim_start_matches('-').trim();
			if term.is_empty() {
				continue;
			}
			let id = Self::get_term_id(term, taxonomy)?;
			let key = if excluded { exclude_key.unwrap_or(include_key) } else { include_key };
			params.push(key, Some(&id));
		}
		Ok(())
	}

	fn parse_page_range(value: &str) -> Option<(i32, i32)> {
		let number = value
			.chars()
			.filter(|character| character.is_ascii_digit())
			.collect::<String>()
			.parse::<i32>()
			.ok()?
			.clamp(1, 9999);
		let value = value.trim();
		if value.starts_with("<=") || value.starts_with("=<") {
			Some((1, number))
		} else if value.starts_with('<') {
			Some((1, (number - 1).max(1)))
		} else if value.starts_with(">=") || value.starts_with("=>") {
			Some((number, 9999))
		} else if value.starts_with('>') {
			Some(((number + 1).min(9999), 9999))
		} else {
			Some((number, number))
		}
	}

	fn search_url(query: Option<String>, page: i32, filters: Vec<FilterValue>) -> Result<String> {
		let mut params = QueryParameters::new();
		if let Some(query) = query.map(|query| query.trim().to_owned()).filter(|query| !query.is_empty()) {
			params.push("s", Some(&query));
		} else {
			params.push("s", Some(""));
		}
		params.push("title-type", Some("contains"));

		for filter in filters {
			match filter {
				FilterValue::Sort { id, index, ascending } if id == "sort" => {
					let sort = match index {
						1 => "alphabet",
						2 => "rating",
						3 => "views",
						_ => "new",
					};
					params.push("sortby", Some(sort));
					params.push("order", Some(if ascending { "asc" } else { "desc" }));
				}
				FilterValue::MultiSelect { id, included, .. } if id == "types" => {
					for value in included {
						params.push("categories[]", Some(&value));
					}
				}
				FilterValue::Text { id, value } if !value.trim().is_empty() => match id.as_str() {
					"tags" => Self::add_term_filters(
						&mut params,
						&value,
						"manga_tag",
						"including[]",
						Some("excluding[]"),
					)?,
					"artists" => Self::add_term_filters(&mut params, &value, "artist", "artists[]", None)?,
					"circles" => Self::add_term_filters(&mut params, &value, "circle", "circles[]", None)?,
					"characters" => Self::add_term_filters(&mut params, &value, "character", "characters[]", None)?,
					"collections" => Self::add_term_filters(&mut params, &value, "collection", "collections[]", None)?,
					"scanlators" => Self::add_term_filters(&mut params, &value, "scanlator", "scanlators[]", None)?,
					"conventions" => Self::add_term_filters(&mut params, &value, "convention", "conventions[]", None)?,
					"uploaded" => {
						let release = value.chars().filter(|character| character.is_ascii_digit()).collect::<String>();
						if !release.is_empty() {
							let release_type = if value.trim().starts_with('>') {
								"after"
							} else if value.trim().starts_with('<') {
								"before"
							} else {
								"in"
							};
							params.push("release-type", Some(release_type));
							params.push("release", Some(&release));
						}
					}
					"pages" => {
						if let Some((minimum, maximum)) = Self::parse_page_range(&value) {
							params.push("pages", Some(&format!("{minimum}-{maximum}")));
						}
					}
					_ => {}
				},
				_ => {}
			}
		}

		Ok(format!("{BASE_URL}/page/{}/?{params}", page.max(1)))
	}

	fn text_list(document: &aidoku::imports::html::Document, selector: &str) -> Option<Vec<String>> {
		document.select(selector).and_then(|elements| {
			let values = elements
				.filter_map(|element| element.text())
				.map(|value| value.trim().to_owned())
				.filter(|value| !value.is_empty())
				.collect::<Vec<_>>();
			(!values.is_empty()).then_some(values)
		})
	}

	fn extract_base64(script: &str) -> Option<&str> {
		let start = script.find("eyJ")?;
		let tail = &script[start..];
		let end = tail
			.char_indices()
			.find(|(_, ch)| !ch.is_ascii_alphanumeric() && !matches!(ch, '+' | '/' | '=' | '-' | '_'))
			.map(|(index, _)| index)
			.unwrap_or(tail.len());
		Some(&tail[..end])
	}

	fn script_text(element: aidoku::imports::html::Element) -> Option<String> {
		element.data().or_else(|| element.html())
	}

	fn parse_base_url(script: &str) -> String {
		if let (Some(start), Some(end)) = (script.find('{'), script.rfind('}')) {
			if start <= end {
				if let Ok(value) = serde_json::from_str::<Value>(&script[start..=end]) {
					if let Some(url) = value.get("baseUrl").and_then(Value::as_str) {
						return url.into();
					}
				}
			}
		}
		for key in ["\"baseUrl\"", "'baseUrl'"] {
			if let Some(value) = script.split_once(key).map(|(_, value)| value) {
				let value = value.trim_start_matches(|ch: char| ch.is_whitespace() || ch == ':');
				if let Some(quote) = value.chars().next().filter(|ch| *ch == '\'' || *ch == '\"') {
					if let Some(end) = value[1..].find(quote) {
						return value[1..=end].into();
					}
				}
			}
		}
		String::new()
	}

	fn page_content(url: String, referer: &str) -> PageContent {
		let mut context = PageContext::new();
		context.insert("referer".into(), referer.into());
		PageContent::url_context(url, context)
	}

	fn parse_reader_pages(
		document: &aidoku::imports::html::Document,
		reader_url: &str,
	) -> Vec<Page> {
		let page_base = document
			.select_first("#single-chapter-js-extra")
			.and_then(Self::script_text)
			.map(|script| Self::parse_base_url(&script))
			.unwrap_or_default();

		let mut pages = Vec::new();
		if let Some(mut encoded) = document
			.select_first("#single-chapter-js-before")
			.and_then(Self::script_text)
			.and_then(|script| Self::extract_base64(&script).map(String::from))
		{
			while encoded.len() % 4 != 0 {
				encoded.push('=');
			}
			if let Ok(decoded) = STANDARD
				.decode(encoded.as_bytes())
				.or_else(|_| URL_SAFE.decode(encoded.as_bytes()))
			{
				if let Ok(value) = serde_json::from_slice::<Value>(&decoded) {
					if let Some(images) = value.pointer("/data/chapter/images").and_then(Value::as_array) {
						pages = images
							.iter()
							.filter_map(|image| image.get("src").and_then(Value::as_str))
							.filter(|path| !Self::is_placeholder(path))
							.map(|path| {
								let url = if path.starts_with("http://") || path.starts_with("https://") {
									path.into()
								} else if page_base.is_empty() {
									Self::absolute_url(path)
								} else {
									format!("{}/{}", page_base.trim_end_matches('/'), path.trim_start_matches('/'))
								};
								Page {
									content: Self::page_content(url, reader_url),
									..Default::default()
								}
							})
							.collect();
					}
				}
			}
		}

		if pages.is_empty() {
			pages = document
				.select(".reading-content img, .page-break img, img.wp-manga-chapter-img")
				.map(|elements| {
					elements
						.filter_map(|element| Self::image_url(&element))
						.map(|url| Page {
							content: Self::page_content(url, reader_url),
							..Default::default()
						})
						.collect()
				})
				.unwrap_or_default();
		}
		pages
	}
}

impl Source for HentaiRead {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		Self::parse_list(&Request::get(Self::search_url(query, page, filters)?)?.html()?)
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		_needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let url = Self::absolute_url(&manga.key);
		let document = Request::get(&url)?.html()?;
		manga.title = document
			.select_first("div.post-title h1, .manga-title h1, h1")
			.and_then(|element| element.text())
			.unwrap_or(manga.title);
		manga.cover = document
			.select_first("meta[name='twitter:image']")
			.and_then(|element| element.attr("content"))
			.map(|value| Self::absolute_url(value.trim()))
			.or_else(|| {
				document
					.select_first("img[fetchpriority='high'], div.summary_image img, .manga-thumb img")
					.and_then(|image| Self::image_url(&image))
			})
			.or(manga.cover);
		manga.authors = Self::text_list(&document, "a[href*='/circle/'] span:first-of-type");
		manga.artists = Self::text_list(&document, "a[href*='/artist/'] span:first-of-type");
		if manga.authors.is_none() {
			manga.authors = manga.artists.clone();
		}
		if manga.artists.is_none() {
			manga.artists = manga.authors.clone();
		}
		manga.tags = Self::text_list(&document, "a[href*='/tag/'] span:first-of-type");
		manga.description = document
			.select_first(".manga-titles h2")
			.and_then(|element| element.text())
			.map(|titles| format!("Alternative titles: {titles}"));
		manga.status = MangaStatus::Completed;
		manga.update_strategy = UpdateStrategy::Never;
		manga.content_rating = ContentRating::NSFW;
		manga.url = Some(url.clone());

		if needs_chapters {
			manga.chapters = Some(vec![Chapter {
				key: manga.key.clone(),
				title: Some("Chapter".into()),
				chapter_number: Some(1.0),
				url: Some(url),
				language: Some("en".into()),
				..Default::default()
			}]);
		}
		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let manga_url = Self::absolute_url(&manga.key);
		let mut reader_urls = Vec::new();
		if chapter.key.contains("/p/") || chapter.key.contains("/english/") {
			reader_urls.push(Self::absolute_url(&chapter.key));
		}

		if let Ok(document) = Request::get(&manga_url)
			.map(|request| request.header("Referer", BASE_URL))
			.and_then(|request| request.html())
		{
			if let Some(elements) = document.select("a[href*='/english/']") {
				for element in elements {
					if let Some(href) = element.attr("abs:href").or_else(|| element.attr("href")) {
						let mut url = Self::absolute_url(&href);
						if !url.contains("/p/") {
							url = format!("{}/p/1/", url.trim_end_matches('/'));
						}
						if !reader_urls.iter().any(|candidate| candidate == &url) {
							reader_urls.push(url);
						}
					}
				}
			}
		}

		for url in [
			format!("{}/english/p/1/", manga_url.trim_end_matches('/')),
			format!("{}/english/", manga_url.trim_end_matches('/')),
		] {
			if !reader_urls.iter().any(|candidate| candidate == &url) {
				reader_urls.push(url);
			}
		}

		for reader_url in reader_urls {
			let Ok(document) = Request::get(&reader_url)
				.map(|request| request.header("Referer", manga_url.as_str()))
				.and_then(|request| request.html())
			else {
				continue;
			};
			let pages = Self::parse_reader_pages(&document, &reader_url);
			if !pages.is_empty() {
				return Ok(pages);
			}
		}

		bail!("No readable pages were returned from the English reader.")
	}
}

impl ListingProvider for HentaiRead {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		match listing.id.as_str() {
			"popular" => Self::browse("views", page),
			"latest" | "recent" => Self::browse("new", page),
			_ => bail!("Unknown listing"),
		}
	}
}

impl Home for HentaiRead {
	fn get_home(&self) -> Result<HomeLayout> {
		Ok(midoku_madara::home_layout(
			Self::browse("views", 1)?.entries,
			Self::browse("new", 1)?.entries,
		))
	}
}

impl ImageRequestProvider for HentaiRead {
	fn get_image_request(
		&self,
		url: String,
		context: Option<aidoku::PageContext>,
	) -> Result<Request> {
		let referer = context
			.as_ref()
			.and_then(|context| context.get("referer"))
			.map(String::as_str)
			.unwrap_or(BASE_URL);
		Ok(Request::get(url)?
			.header("Referer", referer)
			.header("Accept", "image/avif,image/webp,image/apng,image/*,*/*;q=0.8"))
	}
}

impl DeepLinkHandler for HentaiRead {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		Ok(url
			.strip_prefix(BASE_URL)
			.filter(|path| path.contains("/hentai/"))
			.map(|key| DeepLinkResult::Manga { key: key.into() }))
	}
}

register_source!(
	HentaiRead,
	ListingProvider,
	Home,
	ImageRequestProvider,
	DeepLinkHandler
);
