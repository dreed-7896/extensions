#![no_std]

use aidoku::{
	alloc::{borrow::ToOwned, format, string::{String, ToString}, vec::Vec},
	helpers::uri::QueryParameters,
	imports::net::Request,
	prelude::*,
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Home, HomeLayout,
	ImageRequestProvider, Listing, ListingProvider, Manga, MangaPageResult, MangaStatus, Page,
	PageContent, Result, Source, UpdateStrategy,
};
const BASE_URL: &str = "https://hentai2read.com";
const IMAGE_BASE_URL: &str = "https://static.hentai.direct/hentai";

struct Hentai2Read;

impl Hentai2Read {
	fn absolute_url(value: &str) -> String {
		if value.starts_with("http://") || value.starts_with("https://") {
			value.into()
		} else if value.starts_with('/') {
			format!("{BASE_URL}{value}")
		} else {
			format!("{BASE_URL}/{value}")
		}
	}

	fn key_from_url(url: &str) -> String {
		url.strip_prefix(BASE_URL).unwrap_or(url).into()
	}

	fn image_url(element: &aidoku::imports::html::Element) -> Option<String> {
		for key in ["data-src", "data-lazy-src", "src"] {
			if let Some(value) = element.attr(key).filter(|value| !value.trim().is_empty()) {
				return Some(Self::absolute_url(value.trim()));
			}
		}
		element.attr("abs:src")
	}

	fn parse_cards(document: &aidoku::imports::html::Document, selector: &str) -> Vec<Manga> {
		document
			.select(selector)
			.map(|elements| {
				elements
					.filter_map(|card| {
						let link = card.select_first("a.title, div.overlay-title a")?;
						let title = link
							.select_first("span.title-text")
							.and_then(|element| element.text())
							.or_else(|| link.attr("title"))
							.or_else(|| link.text())?
							.trim()
							.to_owned();
						if title.is_empty() {
							return None;
						}
						let href = link.attr("abs:href").or_else(|| link.attr("href"))?;
						let url = Self::absolute_url(&href);
						let cover = card.select_first("picture img, img").and_then(|image| Self::image_url(&image));
						Some(Manga {
							key: Self::key_from_url(&url),
							title,
							cover,
							url: Some(url),
							content_rating: ContentRating::NSFW,
							..Default::default()
						})
					})
					.collect()
			})
			.unwrap_or_default()
	}

	fn parse_list(document: &aidoku::imports::html::Document) -> MangaPageResult {
		let mut entries = Self::parse_cards(document, "div.book-grid-item-container");
		if entries.is_empty() {
			entries = Self::parse_cards(document, "div.book-grid-item");
		}
		let has_next_page = document.select_first("a#js-linkNext, a[rel=next]").is_some();
		MangaPageResult { entries, has_next_page }
	}

	fn browse(sort: &str, page: i32) -> Result<MangaPageResult> {
		let url = format!("{BASE_URL}/hentai-list/all/any/all/{sort}/{page}/");
		Ok(Self::parse_list(&Request::get(url)?.html()?))
	}

	fn search(query: &str, page: i32) -> Result<MangaPageResult> {
		let url = format!("{BASE_URL}/hentai-list/search/any/all/name-az/{page}/");
		let mut body = QueryParameters::new();
		body.push("cmd_wpm_wgt_mng_sch_sbm", Some("Search"));
		body.push("txt_wpm_wgt_mng_sch_nme", Some(query));
		Ok(Self::parse_list(
			&Request::post(url)?
				.header("Content-Type", "application/x-www-form-urlencoded")
				.body(body.to_string())
				.html()?,
		))
	}

	fn text_list(document: &aidoku::imports::html::Document, selector: &str) -> Option<Vec<String>> {
		document.select(selector).and_then(|elements| {
			let values = elements
				.filter_map(|element| element.text())
				.map(|value| value.trim().to_owned())
				.filter(|value| !value.is_empty() && value != "-")
				.collect::<Vec<_>>();
			(!values.is_empty()).then_some(values)
		})
	}

	fn number(value: &str) -> Option<f32> {
		let mut number = String::new();
		let mut started = false;
		for character in value.chars() {
			if character.is_ascii_digit() || (started && character == '.') {
				number.push(character);
				started = true;
			} else if started {
				break;
			}
		}
		number.parse().ok()
	}
}

impl Source for Hentai2Read {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		match query.filter(|query| !query.trim().is_empty()) {
			Some(query) => Self::search(&query, page),
			None => Self::browse("last-updated", page),
		}
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
			.select_first("h3.block-title > a, h1")
			.and_then(|element| element.text())
			.map(|title| title.trim().to_owned())
			.unwrap_or(manga.title);
		manga.cover = document
			.select_first("a#js-linkNext img, .book-cover img, picture img")
			.and_then(|element| Self::image_url(&element))
			.or(manga.cover);
		manga.authors = Self::text_list(&document, "li:contains(Author) > a");
		manga.artists = Self::text_list(&document, "li:contains(Artist) > a");
		manga.tags = Self::text_list(&document, "li:contains(Category) > a, li:contains(Content) > a");
		manga.description = document
			.select_first("li:contains(Storyline) > p, .storyline")
			.and_then(|element| element.text())
			.filter(|value| !value.trim().is_empty());
		manga.status = document
			.select_first("li:contains(Status) > a")
			.and_then(|element| element.text())
			.map(|status| {
				if status.to_ascii_lowercase().contains("completed") {
					MangaStatus::Completed
				} else if status.to_ascii_lowercase().contains("ongoing") {
					MangaStatus::Ongoing
				} else {
					MangaStatus::Unknown
				}
			})
			.unwrap_or(MangaStatus::Unknown);
		manga.url = Some(url);
		manga.content_rating = ContentRating::NSFW;
		manga.update_strategy = UpdateStrategy::Always;
		if needs_chapters {
			manga.chapters = Some(document
				.select("ul.nav-chapters > li > div.media > a, ul.nav-chapters a.pull-left")
				.map(|elements| {
					elements.filter_map(|element| {
						let href = element.attr("abs:href").or_else(|| element.attr("href"))?;
						let title = element.text().map(|value| value.trim().to_owned());
						let url = Self::absolute_url(&href);
						Some(Chapter {
							key: url.clone(),
							chapter_number: title.as_deref().and_then(Self::number),
							title,
							url: Some(url),
							language: Some("en".into()),
							..Default::default()
						})
					}).collect()
				})
				.unwrap_or_default());
		}
		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let document = Request::get(Self::absolute_url(&chapter.key))?.html()?;
		let script = document
			.select("script")
			.and_then(|elements| elements.filter_map(|element| element.data()).find(|data| data.contains("var gData") && data.contains("images")))
			.ok_or_else(|| aidoku::AidokuError::message("Reader data was not found"))?;
		let images_key = script
			.find("'images'")
			.or_else(|| script.find("\"images\""))
			.ok_or_else(|| aidoku::AidokuError::message("Reader images were not found"))?;
		let array_start = script[images_key..]
			.find('[')
			.map(|index| index + images_key)
			.ok_or_else(|| aidoku::AidokuError::message("Reader images were not found"))?;
		let array_end = script[array_start..]
			.find(']')
			.map(|index| index + array_start)
			.ok_or_else(|| aidoku::AidokuError::message("Reader images were not found"))?;
		let images: Vec<String> = serde_json::from_str(&script[array_start..=array_end])
			.map_err(|_| aidoku::AidokuError::message("Invalid reader data"))?;
		let pages = images.into_iter().map(|path| {
			let url = if path.starts_with("http://") || path.starts_with("https://") {
				path
			} else {
				format!("{IMAGE_BASE_URL}{}", path.replace("\\/", "/"))
			};
			Page { content: PageContent::url(url), ..Default::default() }
		}).collect::<Vec<_>>();
		if pages.is_empty() {
			bail!("No readable pages were returned.");
		}
		Ok(pages)
	}
}

impl ListingProvider for Hentai2Read {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		match listing.id.as_str() {
			"popular" => Self::browse("most-popular", page),
			"latest" => Self::browse("last-updated", page),
			_ => bail!("Unknown listing"),
		}
	}
}

impl Home for Hentai2Read {
	fn get_home(&self) -> Result<HomeLayout> {
		Ok(midoku_madara::home_layout(
			Self::browse("most-popular", 1)?.entries,
			Self::browse("last-updated", 1)?.entries,
		))
	}
}

impl ImageRequestProvider for Hentai2Read {
	fn get_image_request(&self, url: String, _context: Option<aidoku::PageContext>) -> Result<Request> {
		Ok(Request::get(url)?.header("Referer", BASE_URL))
	}
}

impl DeepLinkHandler for Hentai2Read {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url.strip_prefix(BASE_URL) else {
			return Ok(None);
		};
		let Some(slug) = path.trim_matches('/').split('/').next() else {
			return Ok(None);
		};
		let blocked_path = matches!(slug, "hentai-list" | "hentai-search" | "tag" | "category");
		Ok((!slug.is_empty() && !blocked_path).then_some(DeepLinkResult::Manga {
			key: format!("/{slug}/"),
		}))
	}
}

register_source!(
	Hentai2Read,
	ListingProvider,
	Home,
	ImageRequestProvider,
	DeepLinkHandler
);
