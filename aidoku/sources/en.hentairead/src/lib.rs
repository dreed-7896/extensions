#![no_std]

use aidoku::{
	alloc::{borrow::ToOwned, format, string::String, vec, vec::Vec},
	helpers::uri::QueryParameters,
	imports::net::Request,
	prelude::*,
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Home, HomeLayout,
	ImageRequestProvider, Listing, ListingProvider, Manga, MangaPageResult, MangaStatus, Page,
	PageContent, Result, Source, UpdateStrategy,
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
				return Some(Self::absolute_url(value.trim()));
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
			.map(|value| Self::absolute_url(value.trim()))
	}

	fn parse_list(document: &aidoku::imports::html::Document) -> MangaPageResult {
		let entries = document
			.select(".manga-item, div.page-item-detail, .c-tabs-item__content")
			.map(|elements| {
				elements
					.filter_map(|element| {
						let link = element.select_first(
							"a.manga-item__link, .post-title a, h3 a, h2 a",
						)?;
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
						let cover = element.select_first("img").and_then(|image| Self::image_url(&image));
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
		MangaPageResult { entries, has_next_page }
	}

	fn browse(order: &str, page: i32) -> Result<MangaPageResult> {
		let path = if page > 1 {
			format!("/hentai/page/{page}/")
		} else {
			"/hentai/".into()
		};
		Ok(Self::parse_list(
			&Request::get(format!("{BASE_URL}{path}?sortby={order}"))?.html()?,
		))
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
		Ok(Self::parse_list(
			&Request::get(format!("{BASE_URL}{path}?{params}"))?.html()?,
		))
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
			.select_first("div.summary_image img, .manga-thumb img")
			.and_then(|image| Self::image_url(&image))
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
			let mut chapters = document
				.select("a[href*='/p/1/'], a[href$='/english/'], a[href$='/japanese/'], a[href$='/chinese/']")
				.map(|elements| {
					elements
						.filter_map(|element| {
							let href = element.attr("abs:href").or_else(|| element.attr("href"))?;
							let mut reader_url = Self::absolute_url(&href);
							if !reader_url.contains("/p/") {
								reader_url = format!("{}/p/1/", reader_url.trim_end_matches('/'));
							}
							let lower = reader_url.to_ascii_lowercase();
							let (label, language) = if lower.contains("/english/") {
								("English", "en")
							} else if lower.contains("/japanese/") {
								("Japanese", "ja")
							} else if lower.contains("/chinese/") {
								("Chinese", "zh")
							} else {
								("Chapter", "en")
							};
							Some(Chapter {
								key: reader_url.clone(),
								title: Some(label.into()),
								url: Some(reader_url),
								language: Some(language.into()),
								..Default::default()
							})
						})
						.fold(Vec::new(), |mut chapters, chapter| {
							if !chapters.iter().any(|existing: &Chapter| existing.key == chapter.key) {
								chapters.push(chapter);
							}
							chapters
						})
				})
				.unwrap_or_default();
			if chapters.is_empty() {
				let reader_url = format!("{}/english/p/1/", url.trim_end_matches('/'));
				chapters.push(Chapter {
					key: reader_url.clone(),
					title: Some("English".into()),
					chapter_number: Some(1.0),
					url: Some(reader_url),
					language: Some("en".into()),
					..Default::default()
				});
			}
			manga.chapters = Some(chapters);
		}
		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let manga_url = Self::absolute_url(&manga.key);
		let reader_url = if chapter.key.contains("/p/") {
			Self::absolute_url(&chapter.key)
		} else {
			format!("{}/english/p/1/", manga_url.trim_end_matches('/'))
		};
		let document = Request::get(&reader_url)?.html()?;

		let page_base = document
			.select_first("#single-chapter-js-extra")
			.and_then(|element| element.data())
			.and_then(|script| {
				let start = script.find('{')?;
				let end = script[start..].find('}')? + start;
				serde_json::from_str::<Value>(&script[start..=end]).ok()
			})
			.and_then(|value| value.get("baseUrl").and_then(Value::as_str).map(String::from))
			.unwrap_or_default();

		let mut pages = Vec::new();
		if let Some(mut encoded) = document
			.select_first("#single-chapter-js-before")
			.and_then(|element| element.data())
			.and_then(|script| Self::extract_base64(&script).map(String::from))
		{
			while encoded.len() % 4 != 0 {
				encoded.push('=');
			}
			if let Ok(decoded) = STANDARD.decode(encoded.as_bytes()).or_else(|_| URL_SAFE.decode(encoded.as_bytes())) {
				if let Ok(value) = serde_json::from_slice::<Value>(&decoded) {
					if let Some(images) = value.pointer("/data/chapter/images").and_then(Value::as_array) {
						pages = images
							.iter()
							.filter_map(|image| image.get("src").and_then(Value::as_str))
							.map(|path| {
								let url = if page_base.is_empty() {
									Self::absolute_url(path)
								} else {
									format!("{}/{}", page_base.trim_end_matches('/'), path.trim_start_matches('/'))
								};
								Page { content: PageContent::url(url), ..Default::default() }
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
						.map(|url| Page { content: PageContent::url(url), ..Default::default() })
						.collect()
				})
				.unwrap_or_default();
		}
		if pages.is_empty() {
			bail!("No readable pages were returned.");
		}
		Ok(pages)
	}
}

impl ListingProvider for HentaiRead {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		match listing.id.as_str() {
			"popular" => Self::browse("views", page),
			"latest" => Self::browse("new", page),
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
		_context: Option<aidoku::PageContext>,
	) -> Result<Request> {
		Ok(Request::get(url)?
			.header("Referer", BASE_URL)
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
