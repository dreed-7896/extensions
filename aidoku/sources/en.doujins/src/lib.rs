#![no_std]

use aidoku::{
	alloc::{borrow::ToOwned, format, string::{String, ToString}, vec, vec::Vec},
	helpers::uri::QueryParameters,
	imports::{net::Request, std::current_date},
	prelude::*,
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, ImageRequestProvider,
	Listing, ListingProvider, Manga, MangaPageResult, MangaStatus, Page, PageContent, Result,
	Source, UpdateStrategy,
};
use midoku_madara::is_blocked;
use serde::Deserialize;

const BASE_URL: &str = "https://doujins.com";
const PAGE_DAYS: i64 = 3;

#[derive(Deserialize)]
struct LatestResponse {
	#[serde(default)]
	folders: Vec<LatestFolder>,
}

#[derive(Deserialize)]
struct LatestFolder {
	link: String,
	name: String,
	#[serde(rename = "artistList")]
	artist_list: Option<String>,
	#[serde(default)]
	tags: Vec<Tag>,
	thumbnail2: Option<String>,
}

#[derive(Deserialize)]
struct Tag {
	tag: String,
}

struct Doujins;

impl Doujins {
	fn absolute_url(value: &str) -> String {
		if value.starts_with("http://") || value.starts_with("https://") {
			value.into()
		} else if value.starts_with('/') {
			format!("{BASE_URL}{value}")
		} else {
			format!("{BASE_URL}/{value}")
		}
	}

	fn parse_gallery(document: &aidoku::imports::html::Document) -> MangaPageResult {
		let entries = document
			.select("div:not(.premium-folder) > .thumbnail-doujin a.gallery-visited-from-favorites")
			.map(|elements| {
				elements
					.filter_map(|element| {
						let title = element
							.select_first("div.title .text")
							.and_then(|item| item.text())?
							.trim()
							.to_owned();
						if title.is_empty() || is_blocked(&title) {
							return None;
						}
						let href = element.attr("abs:href").or_else(|| element.attr("href"))?;
						let cover = element.select_first("img").and_then(|image| {
							image
								.attr("srcset")
								.and_then(|srcset| srcset.split_whitespace().next().map(ToOwned::to_owned))
								.or_else(|| image.attr("abs:src"))
								.or_else(|| image.attr("src"))
						});
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
			.select_first(".pagination li.page-item:last-child:not(.disabled), .pagination a[rel=next]")
			.is_some();
		MangaPageResult { entries, has_next_page }
	}

	fn popular(page: i32) -> Result<MangaPageResult> {
		let url = if page > 1 {
			format!("{BASE_URL}/top/month?page={page}")
		} else {
			format!("{BASE_URL}/top/month")
		};
		Ok(Self::parse_gallery(&Request::get(url)?.html()?))
	}

	fn latest(page: i32) -> Result<MangaPageResult> {
		let end = current_date() + 86_400 - PAGE_DAYS * 86_400 * i64::from(page - 1);
		let start = end - PAGE_DAYS * 86_400;
		let response: LatestResponse = Request::get(format!(
			"{BASE_URL}/folders?start={start}&end={end}"
		))?
		.json_owned()?;
		let entries = response
			.folders
			.into_iter()
			.filter_map(|folder| {
				let tag_text = folder
					.tags
					.iter()
					.map(|tag| tag.tag.as_str())
					.collect::<Vec<_>>()
					.join(" ");
				if is_blocked(&format!("{} {}", folder.name, tag_text)) {
					return None;
				}
				let absolute_url = Self::absolute_url(&folder.link);
				Some(Manga {
					key: absolute_url
						.strip_prefix(BASE_URL)
						.unwrap_or(&folder.link)
						.into(),
					title: folder.name,
					cover: folder.thumbnail2,
					artists: folder.artist_list.map(|artist| vec![artist]),
					tags: Some(folder.tags.into_iter().map(|tag| tag.tag).collect()),
					content_rating: ContentRating::NSFW,
					..Default::default()
				})
			})
			.collect();
		Ok(MangaPageResult {
			entries,
			has_next_page: true,
		})
	}
}

impl Source for Doujins {
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
			return Self::popular(page);
		};
		if is_blocked(&query) {
			bail!("This search term is not supported.");
		}
		let mut params = QueryParameters::new();
		params.push("words", Some(&query));
		params.push("page", Some(&page.to_string()));
		params.push("sort", Some(""));
		let document = Request::get(format!("{BASE_URL}/searches?{params}"))?.html()?;
		Ok(Self::parse_gallery(&document))
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
			.select(".folder-title a")
			.and_then(|mut elements| elements.next_back())
			.and_then(|element| element.text())
			.unwrap_or(manga.title);
		manga.artists = document.select(".gallery-artist a").and_then(|elements| {
			let values = elements.filter_map(|item| item.text()).collect::<Vec<_>>();
			(!values.is_empty()).then_some(values)
		});
		manga.authors = manga.artists.clone();
		manga.tags = document.select(".tag-area a").and_then(|elements| {
			let values = elements.filter_map(|item| item.text()).collect::<Vec<_>>();
			(!values.is_empty()).then_some(values)
		});
		manga.url = Some(url.clone());
		manga.status = MangaStatus::Completed;
		manga.update_strategy = UpdateStrategy::Never;
		manga.content_rating = ContentRating::NSFW;
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

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let url = Self::absolute_url(&chapter.key);
		let document = Request::get(&url)?.html()?;
		let pages = document
			.select(".doujin")
			.map(|elements| {
				elements
					.filter_map(|element| element.attr("data-file"))
					.map(|value| value.replace("amp;", ""))
					.map(|value| Page {
						content: PageContent::url(Self::absolute_url(&value)),
						..Default::default()
					})
					.collect::<Vec<_>>()
			})
			.unwrap_or_default();
		if pages.is_empty() {
			bail!("No pages found. The website layout may have changed.");
		}
		Ok(pages)
	}
}

impl ListingProvider for Doujins {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		match listing.id.as_str() {
			"popular" => Self::popular(page),
			"latest" => Self::latest(page),
			_ => bail!("Unknown listing"),
		}
	}
}

impl ImageRequestProvider for Doujins {
	fn get_image_request(
		&self,
		url: String,
		_context: Option<aidoku::PageContext>,
	) -> Result<Request> {
		Ok(Request::get(url)?.header("Referer", BASE_URL))
	}
}

impl DeepLinkHandler for Doujins {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		Ok(url
			.strip_prefix(BASE_URL)
			.map(|key| DeepLinkResult::Manga { key: key.into() }))
	}
}

register_source!(
	Doujins,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);
