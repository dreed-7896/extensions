#![no_std]

use aidoku::{
	alloc::{borrow::ToOwned, format, string::String, vec, vec::Vec},
	helpers::uri::QueryParameters,
	imports::net::Request,
	prelude::*,
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Home, HomeLayout,
	ImageRequestProvider, Listing, ListingProvider, Manga, MangaPageResult, MangaStatus, Page,
	PageContent, PageContext, Result, Source, UpdateStrategy,
};
use base64::{engine::general_purpose::{STANDARD, URL_SAFE}, Engine as _};
use midoku_madara::is_blocked;
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
						if title.is_empty() || is_blocked(&title) {
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
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let Some(query) = query.filter(|query| !query.trim().is_empty()) else {
			return Self::browse("new", page);
		};
		if is_blocked(&query) {
			bail!("This search term is not supported.");
		}
		let path = if page > 1 {
			format!("/page/{page}/")
		} else {
			"/".into()
		};
		let mut params = QueryParameters::new();
		params.push("s", Some(&query));
		params.push("title-type", Some("contains"));
		Self::parse_list(&Request::get(format!("{BASE_URL}{path}?{params}"))?.html()?)
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

		let tags = manga.tags.as_ref().map(|value| value.join(" ")).unwrap_or_default();
		if is_blocked(&format!("{} {tags}", manga.title)) {
			bail!("This title is not supported.");
		}

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
