#![no_std]

use aidoku::{
	alloc::{format, string::{String, ToString}, vec::Vec},
	helpers::uri::encode_uri_component,
	imports::net::Request,
	prelude::*,
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, ImageRequestProvider,
	Listing, ListingProvider, Manga, MangaPageResult, MangaStatus, Page, PageContent, Result,
	Source, UpdateStrategy, Viewer,
};
use midoku_madara::is_blocked;
use serde::Deserialize;
use serde_json::{json, Value};

const BASE_URL: &str = "https://hiperdex.tv";
const API_HEADER: &str = "yceqt7qgu004";

#[derive(Deserialize)]
struct SearchContent {
	#[serde(default)]
	hits: Vec<MangaDto>,
}

#[derive(Deserialize)]
struct MangaDto {
	id: i64,
	slug: String,
	title: String,
	synopsis: Option<String>,
	#[serde(rename = "coverUrl")]
	cover_url: Option<String>,
	status: Option<String>,
	#[serde(default)]
	genres: Vec<String>,
	#[serde(default)]
	authors: Vec<String>,
	#[serde(default)]
	artists: Vec<String>,
	#[serde(rename = "type")]
	manga_type: Option<String>,
	#[serde(rename = "contentRating")]
	content_rating: Option<String>,
}

#[derive(Deserialize)]
struct ChapterDto {
	id: i64,
	number: f32,
	title: Option<String>,
}

#[derive(Deserialize)]
struct PageDto {
	#[serde(rename = "pageOrder")]
	page_order: i32,
	#[serde(rename = "webpUrl")]
	webp_url: String,
	#[serde(rename = "avifUrl")]
	avif_url: Option<String>,
}

struct HiperDex;

impl HiperDex {
	fn request_json(url: &str) -> Result<Value> {
		let response = Request::get(url)?.header("x-cfg-auth", API_HEADER).send()?;
		if response.status_code() == 401 {
			drop(response);
			let _ = Request::get(BASE_URL)?
				.header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
				.send()?;
			return Ok(Request::get(url)?
				.header("x-cfg-auth", API_HEADER)
				.json_owned()?);
		}
		Ok(response.get_json_owned()?)
	}

	fn endpoint(path: &str, input: Value) -> String {
		format!(
			"{BASE_URL}/api/trpc/{path}?batch=1&input={}",
			encode_uri_component(input.to_string())
		)
	}

	fn result_json(value: &Value, last: bool) -> Option<&Value> {
		let entries = value.as_array()?;
		let entry = if last { entries.last()? } else { entries.first()? };
		entry.get("result")?.get("data")?.get("json")
	}

	fn manga_from_dto(dto: MangaDto) -> Option<Manga> {
		let mut tags = dto.genres;
		if let Some(kind) = dto.manga_type.as_ref() {
			tags.push(kind.clone());
		}
		if let Some(rating) = dto.content_rating.as_ref() {
			tags.push(rating.clone());
		}
		if is_blocked(&format!("{} {}", dto.title, tags.join(" "))) {
			return None;
		}
		let status = match dto.status.as_deref().map(|value| value.to_ascii_lowercase()).as_deref() {
			Some("ongoing") => MangaStatus::Ongoing,
			Some("completed") => MangaStatus::Completed,
			Some("hiatus") => MangaStatus::Hiatus,
			Some("cancelled") | Some("canceled") => MangaStatus::Cancelled,
			_ => MangaStatus::Unknown,
		};
		let viewer = match dto.manga_type.as_deref().map(|value| value.to_ascii_lowercase()).as_deref() {
			Some("manhwa") | Some("manhua") | Some("webtoon") => Viewer::Webtoon,
			_ => Viewer::RightToLeft,
		};
		Some(Manga {
			key: format!("{}#{}", dto.slug, dto.id),
			title: dto.title,
			cover: dto.cover_url,
			artists: (!dto.artists.is_empty()).then_some(dto.artists),
			authors: (!dto.authors.is_empty()).then_some(dto.authors),
			description: dto.synopsis,
			url: Some(format!("{BASE_URL}/manga/{}", dto.slug)),
			tags: (!tags.is_empty()).then_some(tags),
			status,
			content_rating: ContentRating::NSFW,
			viewer,
			update_strategy: UpdateStrategy::Always,
			..Default::default()
		})
	}

	fn search(query: &str, sort: &str, page: i32) -> Result<MangaPageResult> {
		if is_blocked(query) {
			bail!("This search term is not supported.");
		}
		let input = json!({
			"0": {
				"json": {
					"q": query,
					"sort": sort,
					"filters": {
						"genres": null,
						"type": null,
						"status": null,
						"contentRating": null,
						"author": null,
						"artist": null,
						"year": null
					},
					"limit": 30,
					"offset": (page - 1) * 30,
					"maxRating": "pornographic"
				},
				"meta": {"values": {
					"filters.genres": ["undefined"],
					"filters.type": ["undefined"],
					"filters.status": ["undefined"],
					"filters.contentRating": ["undefined"],
					"filters.author": ["undefined"],
					"filters.artist": ["undefined"],
					"filters.year": ["undefined"]
				}}
			}
		});
		let value = Self::request_json(&Self::endpoint("search.query", input))?;
		let data = Self::result_json(&value, false)
			.ok_or_else(|| aidoku::AidokuError::message("Invalid search response"))?;
		let wrapper: SearchContent = serde_json::from_value(data.clone())
			.map_err(|_| aidoku::AidokuError::message("Invalid search response"))?;
		let has_next_page = !wrapper.hits.is_empty();
		Ok(MangaPageResult {
			entries: wrapper
				.hits
				.into_iter()
				.filter_map(Self::manga_from_dto)
				.collect(),
			has_next_page,
		})
	}

	fn slug(key: &str) -> &str {
		key.split('#').next().unwrap_or(key).trim_matches('/')
	}

	fn id(key: &str) -> Option<i64> {
		key.split('#').nth(1)?.parse().ok()
	}

	fn details(slug: &str) -> Result<Manga> {
		let input = json!({
			"0": {"json": null, "meta": {"values": ["undefined"]}},
			"1": {"json": {"slug": slug}}
		});
		let value = Self::request_json(&Self::endpoint(
			"auth.me,series.bySlugWithGenres",
			input,
		))?;
		let data = Self::result_json(&value, true)
			.ok_or_else(|| aidoku::AidokuError::message("Invalid title response"))?;
		let dto: MangaDto = serde_json::from_value(data.clone())
			.map_err(|_| aidoku::AidokuError::message("Invalid title response"))?;
		Self::manga_from_dto(dto)
			.ok_or_else(|| aidoku::AidokuError::message("This title is not supported."))
	}

	fn chapters(series_id: i64, slug: &str) -> Result<Vec<Chapter>> {
		let input = json!({
			"0": {"json": {"values": ["undefined"]}},
			"1": {
				"json": {
					"seriesId": series_id,
					"chapterId": null,
					"sort": "best",
					"page": 1,
					"limit": 200
				},
				"meta": {"values": {"chapterId": ["undefined"]}}
			},
			"2": {"json": {"seriesId": series_id}}
		});
		let value = Self::request_json(&Self::endpoint("auth.me,series.chapters", input))?;
		let data = Self::result_json(&value, true)
			.ok_or_else(|| aidoku::AidokuError::message("Invalid chapter response"))?;
		let chapters: Vec<ChapterDto> = serde_json::from_value(data.clone())
			.map_err(|_| aidoku::AidokuError::message("Invalid chapter response"))?;
		Ok(chapters
			.into_iter()
			.map(|chapter| Chapter {
				key: format!("{slug}#{}#{}", chapter.number, chapter.id),
				title: chapter.title,
				chapter_number: Some(chapter.number),
				url: Some(format!(
					"{BASE_URL}/manga/{slug}/{}",
					chapter.number.to_string().trim_end_matches(".0")
				)),
				language: Some("en".into()),
				..Default::default()
			})
			.collect())
	}
}

impl Source for HiperDex {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		Self::search(query.as_deref().unwrap_or(""), "relevance", page)
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
		let series_id = Self::id(&updated.key)
			.ok_or_else(|| aidoku::AidokuError::message("Missing series ID"))?;
		if needs_chapters {
			updated.chapters = Some(Self::chapters(series_id, &slug)?);
		}
		Ok(updated)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let mut parts = chapter.key.split('#');
		let slug = parts.next().unwrap_or_default();
		let chapter_number = parts.next().unwrap_or("1");
		let chapter_id = parts.next().and_then(|value| value.parse::<i64>().ok());
		let input = json!({
			"0": {"json": null, "meta": {"values": ["undefined"]}},
			"1": {"json": {"slug": slug}},
			"2": {"json": {
				"seriesSlug": slug,
				"chapterNumber": chapter_number.parse::<f32>().unwrap_or(1.0),
				"chapterId": chapter_id
			}},
			"3": {"json": {"position": "footer_bottom"}}
		});
		let value = Self::request_json(&Self::endpoint(
			"auth.me,series.bySlug,reader.chapterPages",
			input,
		))?;
		let data = Self::result_json(&value, true)
			.ok_or_else(|| aidoku::AidokuError::message("Invalid page response"))?;
		let mut pages: Vec<PageDto> = serde_json::from_value(data.clone())
			.map_err(|_| aidoku::AidokuError::message("Invalid page response"))?;
		pages.sort_by_key(|page| page.page_order);
		Ok(pages
			.into_iter()
			.map(|page| Page {
				content: PageContent::url(page.avif_url.unwrap_or(page.webp_url)),
				..Default::default()
			})
			.collect())
	}
}

impl ListingProvider for HiperDex {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		match listing.id.as_str() {
			"popular" => Self::search("", "popular", page),
			"latest" => Self::search("", "recent", page),
			_ => bail!("Unknown listing"),
		}
	}
}

impl ImageRequestProvider for HiperDex {
	fn get_image_request(
		&self,
		url: String,
		_context: Option<aidoku::PageContext>,
	) -> Result<Request> {
		Ok(Request::get(url)?.header("Referer", BASE_URL))
	}
}

impl DeepLinkHandler for HiperDex {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url.strip_prefix(&format!("{BASE_URL}/manga/")) else {
			return Ok(None);
		};
		let slug = path.trim_matches('/').split('/').next().unwrap_or_default();
		Ok((!slug.is_empty()).then_some(DeepLinkResult::Manga { key: slug.into() }))
	}
}

register_source!(
	HiperDex,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);
