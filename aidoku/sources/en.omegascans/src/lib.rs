#![no_std]

use aidoku::{
	alloc::{format, string::{String, ToString}, vec, vec::Vec},
	helpers::uri::QueryParameters,
	imports::net::Request,
	prelude::*,
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Home, HomeLayout,
	ImageRequestProvider, Listing, ListingProvider, Manga, MangaPageResult, MangaStatus, Page,
	PageContent, Result, Source, UpdateStrategy, Viewer,
};
use serde::Deserialize;

const BASE_URL: &str = "https://omegascans.org";
const API_URL: &str = "https://api.omegascans.org";

#[derive(Deserialize)]
struct QueryResponse {
	#[serde(default)]
	data: Vec<SeriesDto>,
	meta: Option<PageMeta>,
}

#[derive(Deserialize)]
struct PageMeta {
	current_page: i32,
	last_page: i32,
}

#[derive(Deserialize)]
struct SeriesDto {
	id: i64,
	#[serde(rename = "series_slug")]
	slug: String,
	author: Option<String>,
	description: Option<String>,
	studio: Option<String>,
	status: Option<String>,
	thumbnail: String,
	title: String,
	#[serde(default)]
	tags: Vec<TagDto>,
}

#[derive(Deserialize)]
struct TagDto {
	name: String,
}

#[derive(Deserialize)]
struct ChapterResponse {
	#[serde(default)]
	data: Vec<ChapterDto>,
}

#[derive(Deserialize)]
struct ChapterDto {
	#[serde(rename = "chapter_name")]
	name: String,
	#[serde(rename = "chapter_title")]
	title: Option<String>,
	#[serde(rename = "chapter_slug")]
	slug: String,
	price: Option<i32>,
}

#[derive(Deserialize)]
struct PageResponse {
	chapter: PageChapter,
	#[serde(default)]
	paywall: bool,
}

#[derive(Deserialize)]
struct PageChapter {
	#[serde(rename = "chapter_data")]
	data: Option<PageData>,
}

#[derive(Deserialize)]
struct PageData {
	#[serde(default)]
	images: Vec<String>,
}

struct OmegaScans;

impl OmegaScans {
	fn absolute_asset(value: &str) -> String {
		if value.starts_with("http://") || value.starts_with("https://") {
			value.into()
		} else {
			format!("{API_URL}/{}", value.trim_start_matches('/'))
		}
	}

	fn status(value: Option<&str>) -> MangaStatus {
		match value.map(|value| value.to_ascii_lowercase()).as_deref() {
			Some("ongoing") => MangaStatus::Ongoing,
			Some("completed") | Some("finished") => MangaStatus::Completed,
			Some("hiatus") => MangaStatus::Hiatus,
			Some("dropped") | Some("cancelled") | Some("canceled") => MangaStatus::Cancelled,
			_ => MangaStatus::Unknown,
		}
	}

	fn plain_text(value: String) -> String {
		let mut result = String::new();
		let mut inside_tag = false;
		for character in value.chars() {
			match character {
				'<' => inside_tag = true,
				'>' => {
					inside_tag = false;
					if !result.ends_with(' ') {
						result.push(' ');
					}
				}
				_ if !inside_tag => result.push(character),
				_ => {}
			}
		}
		result.trim().to_string()
	}

	fn manga(dto: SeriesDto) -> Option<Manga> {
		let tags = dto.tags.into_iter().map(|tag| tag.name).collect::<Vec<_>>();
		Some(Manga {
			key: format!("{}#{}", dto.slug, dto.id),
			title: dto.title,
			cover: (!dto.thumbnail.is_empty()).then(|| Self::absolute_asset(&dto.thumbnail)),
			authors: dto.author.filter(|value| !value.trim().is_empty()).map(|value| vec![value]),
			artists: dto.studio.filter(|value| !value.trim().is_empty()).map(|value| vec![value]),
			description: dto.description.map(Self::plain_text).filter(|value| !value.is_empty()),
			url: Some(format!("{BASE_URL}/series/{}", dto.slug)),
			tags: (!tags.is_empty()).then_some(tags),
			status: Self::status(dto.status.as_deref()),
			content_rating: ContentRating::NSFW,
			viewer: Viewer::Webtoon,
			update_strategy: UpdateStrategy::Always,
			..Default::default()
		})
	}

	fn slug(key: &str) -> &str {
		key.split('#').next().unwrap_or(key).trim_matches('/')
	}

	fn id(key: &str) -> Option<i64> {
		key.split('#').nth(1)?.parse().ok()
	}

	fn query(query: &str, order_by: &str, page: i32) -> Result<MangaPageResult> {
		let mut params = QueryParameters::new();
		params.push("query_string", Some(query));
		params.push("status", Some("All"));
		params.push("order", Some("desc"));
		params.push("orderBy", Some(order_by));
		params.push("series_type", Some("Comic"));
		params.push("page", Some(&page.to_string()));
		params.push("perPage", Some("12"));
		params.push("tags_ids", Some("[]"));
		params.push("adult", Some("true"));
		let response: QueryResponse = Request::get(format!("{API_URL}/query?{params}"))?.json_owned()?;
		let has_next_page = response.meta
			.map(|meta| meta.current_page < meta.last_page)
			.unwrap_or(false);
		Ok(MangaPageResult {
			entries: response.data.into_iter().filter_map(Self::manga).collect(),
			has_next_page,
		})
	}

	fn details(slug: &str) -> Result<Manga> {
		let dto: SeriesDto = Request::get(format!("{API_URL}/series/{slug}"))?.json_owned()?;
		Self::manga(dto).ok_or_else(|| aidoku::AidokuError::message("This title is not supported."))
	}

	fn chapters(series_id: i64, series_slug: &str) -> Result<Vec<Chapter>> {
		let mut params = QueryParameters::new();
		params.push("page", Some("1"));
		params.push("perPage", Some("1000"));
		params.push("series_id", Some(&series_id.to_string()));
		let response: ChapterResponse = Request::get(format!("{API_URL}/chapter/query?{params}"))?.json_owned()?;
		Ok(response.data.into_iter()
			.filter(|chapter| chapter.price.unwrap_or(0) == 0)
			.map(|chapter| {
				let number = chapter.name
					.split(|character: char| !character.is_ascii_digit() && character != '.')
					.find_map(|part| part.parse::<f32>().ok());
				let title = chapter.title
					.filter(|title| !title.trim().is_empty())
					.map(|title| format!("{} - {title}", chapter.name))
					.or_else(|| Some(chapter.name));
				Chapter {
					key: format!("/chapter/{series_slug}/{}", chapter.slug),
					title,
					chapter_number: number,
					url: Some(format!("{BASE_URL}/series/{series_slug}/{}", chapter.slug)),
					language: Some("en".into()),
					..Default::default()
				}
			})
			.collect())
	}
}

impl Source for OmegaScans {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		Self::query(query.as_deref().unwrap_or(""), "total_views", page)
	}

	fn get_manga_update(
		&self,
		manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let slug = Self::slug(&manga.key).to_string();
		let mut updated = if needs_details || Self::id(&manga.key).is_none() {
			Self::details(&slug)?
		} else {
			manga
		};
		if needs_chapters {
			let id = Self::id(&updated.key)
				.ok_or_else(|| aidoku::AidokuError::message("Missing series ID"))?;
			updated.chapters = Some(Self::chapters(id, &slug)?);
		}
		Ok(updated)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let response: PageResponse = Request::get(format!("{API_URL}{}", chapter.key))?.json_owned()?;
		if response.paywall && response.chapter.data.is_none() {
			bail!("Paid chapter unavailable.");
		}
		let pages = response.chapter.data
			.map(|data| data.images.into_iter().map(|image| Page {
				content: PageContent::url(Self::absolute_asset(&image)),
				..Default::default()
			}).collect::<Vec<_>>())
			.unwrap_or_default();
		if pages.is_empty() {
			bail!("No readable pages were returned.");
		}
		Ok(pages)
	}
}

impl ListingProvider for OmegaScans {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		match listing.id.as_str() {
			"popular" => Self::query("", "total_views", page),
			"latest" => Self::query("", "latest", page),
			_ => bail!("Unknown listing"),
		}
	}
}

impl Home for OmegaScans {
	fn get_home(&self) -> Result<HomeLayout> {
		Ok(midoku_madara::home_layout(
			Self::query("", "total_views", 1)?.entries,
			Self::query("", "latest", 1)?.entries,
		))
	}
}

impl ImageRequestProvider for OmegaScans {
	fn get_image_request(&self, url: String, _context: Option<aidoku::PageContext>) -> Result<Request> {
		Ok(Request::get(url)?
			.header("Referer", BASE_URL)
			.header("Accept", "image/avif,image/webp,image/apng,image/*,*/*;q=0.8"))
	}
}

impl DeepLinkHandler for OmegaScans {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url.strip_prefix(&format!("{BASE_URL}/series/")) else {
			return Ok(None);
		};
		let slug = path.trim_matches('/').split('/').next().unwrap_or_default();
		Ok((!slug.is_empty()).then_some(DeepLinkResult::Manga { key: slug.into() }))
	}
}

register_source!(
	OmegaScans,
	ListingProvider,
	Home,
	ImageRequestProvider,
	DeepLinkHandler
);
