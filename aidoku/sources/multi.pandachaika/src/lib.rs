#![no_std]

use aidoku::{
	alloc::{borrow::ToOwned, format, string::{String, ToString}, vec, vec::Vec},
	helpers::uri::QueryParameters,
	imports::net::Request,
	prelude::*,
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Home, HomeLayout,
	ImageRequestProvider, Listing, ListingProvider, Manga, MangaPageResult, MangaStatus, Page,
	PageContent, Result, Source, UpdateStrategy,
};
use serde::Deserialize;

const BASE_URL: &str = "https://panda.chaika.moe";

#[derive(Deserialize)]
struct ArchiveResponse {
	#[serde(default)]
	archives: Vec<LongArchive>,
	#[serde(default, rename = "has_next")]
	has_next: bool,
}

#[derive(Deserialize)]
struct LongArchive {
	id: i64,
	title: String,
	thumbnail: Option<String>,
	#[serde(default)]
	tags: Vec<String>,
	filecount: Option<i32>,
	filesize: Option<f64>,
	#[serde(rename = "title_jpn")]
	title_japanese: Option<String>,
	uploader: Option<String>,
}

#[derive(Deserialize)]
struct Archive {
	download: String,
	posted: Option<i64>,
	title: Option<String>,
}

struct PandaChaika;

impl PandaChaika {
	fn absolute_url(value: &str) -> String {
		if value.starts_with("http://") || value.starts_with("https://") {
			value.into()
		} else if value.starts_with('/') {
			format!("{BASE_URL}{value}")
		} else {
			format!("{BASE_URL}/{value}")
		}
	}

	fn cleaned_tag(value: String) -> String {
		value
			.split_once(':')
			.map(|(_, tag)| tag)
			.unwrap_or(&value)
			.replace('_', " ")
	}

	fn to_manga(archive: LongArchive) -> Option<Manga> {
		let artists = archive
			.tags
			.iter()
			.filter_map(|tag| tag.strip_prefix("artist:").map(|value| value.replace('_', " ")))
			.collect::<Vec<_>>();
		let groups = archive
			.tags
			.iter()
			.filter_map(|tag| tag.strip_prefix("group:").map(|value| value.replace('_', " ")))
			.collect::<Vec<_>>();
		let tags = archive
			.tags
			.into_iter()
			.filter(|tag| {
				!tag.starts_with("artist:")
					&& !tag.starts_with("group:")
					&& !tag.starts_with("publisher:")
			})
			.map(Self::cleaned_tag)
			.collect::<Vec<_>>();
		let description = format!(
			"Uploader: {}\nPages: {}\nFile size: {} MB{}",
			archive.uploader.as_deref().unwrap_or("Anonymous"),
			archive.filecount.unwrap_or(0),
			archive.filesize.unwrap_or(0.0) / 1_000_000.0,
			archive
				.title_japanese
				.as_ref()
				.map(|title| format!("\nJapanese title: {title}"))
				.unwrap_or_default()
		);
		Some(Manga {
			key: archive.id.to_string(),
			title: archive.title,
			cover: archive.thumbnail.map(|url| Self::absolute_url(&url)),
			artists: (!artists.is_empty()).then_some(artists),
			authors: (!groups.is_empty()).then_some(groups),
			description: Some(description),
			url: Some(format!("{BASE_URL}/archive/{}/", archive.id)),
			tags: (!tags.is_empty()).then_some(tags),
			status: MangaStatus::Completed,
			content_rating: ContentRating::NSFW,
			update_strategy: UpdateStrategy::Never,
			..Default::default()
		})
	}

	fn search(query: &str, sort: &str, page: i32) -> Result<MangaPageResult> {
		let mut params = QueryParameters::new();
		params.push("title", Some(query));
		params.push("sort", Some(sort));
		params.push("page", Some(&page.to_string()));
		params.push("apply", Some(""));
		params.push("json", Some(""));
		let response: ArchiveResponse =
			Request::get(format!("{BASE_URL}/search/?{params}"))?.json_owned()?;
		Ok(MangaPageResult {
			entries: response
				.archives
				.into_iter()
				.filter_map(Self::to_manga)
				.collect(),
			has_next_page: response.has_next,
		})
	}

	fn zip_entries(url: &str) -> Result<Vec<String>> {
		const TAIL_SIZE: usize = 131_072;
		let tail_range = format!("bytes=-{TAIL_SIZE}");
		let tail_request = Request::get(url)?.header("Range", tail_range.as_str());
		let tail_response = tail_request.send()?;
		let content_range = tail_response.get_header("Content-Range");
		let tail = tail_response.get_data()?;
		let tail_start = content_range
			.as_deref()
			.and_then(|value| value.strip_prefix("bytes "))
			.and_then(|value| value.split('-').next())
			.and_then(|value| value.parse::<u64>().ok())
			.unwrap_or(0);

		let eocd = tail
			.windows(4)
			.rposition(|window| window == [0x50, 0x4b, 0x05, 0x06])
			.ok_or_else(|| aidoku::AidokuError::message("ZIP directory was not found"))?;
		if eocd + 22 > tail.len() {
			bail!("Invalid ZIP directory");
		}
		let entries_count = read_u16(&tail, eocd + 10)? as usize;
		let directory_size = read_u32(&tail, eocd + 12)? as u64;
		let directory_offset = read_u32(&tail, eocd + 16)? as u64;
		let directory_end = directory_offset + directory_size;
		let tail_end = tail_start + tail.len() as u64;

		let directory = if directory_offset >= tail_start && directory_end <= tail_end {
			let start = (directory_offset - tail_start) as usize;
			let end = start + directory_size as usize;
			tail[start..end].to_vec()
		} else {
			let directory_range = format!(
				"bytes={}-{}",
				directory_offset,
				directory_end.saturating_sub(1)
			);
			Request::get(url)?
				.header("Range", directory_range.as_str())
				.data()?
		};

		let mut offset = 0usize;
		let mut files = Vec::with_capacity(entries_count);
		for _ in 0..entries_count {
			if offset + 46 > directory.len()
				|| directory[offset..offset + 4] != [0x50, 0x4b, 0x01, 0x02]
			{
				break;
			}
			let name_length = read_u16(&directory, offset + 28)? as usize;
			let extra_length = read_u16(&directory, offset + 30)? as usize;
			let comment_length = read_u16(&directory, offset + 32)? as usize;
			let name_start = offset + 46;
			let name_end = name_start + name_length;
			if name_end > directory.len() {
				break;
			}
			if let Ok(name) = String::from_utf8(directory[name_start..name_end].to_vec()) {
				let lower = name.to_ascii_lowercase();
				if !name.ends_with('/')
					&& [".jpg", ".jpeg", ".png", ".webp", ".gif", ".avif"]
						.iter()
						.any(|extension| lower.ends_with(extension))
				{
					files.push(name);
				}
			}
			offset = name_end + extra_length + comment_length;
		}
		files.sort_by(|left, right| left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase()));
		Ok(files)
	}
}

impl Source for PandaChaika {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		Self::search(query.as_deref().unwrap_or(""), "public_date", page)
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		_needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		manga.content_rating = ContentRating::NSFW;
		manga.status = MangaStatus::Completed;
		manga.update_strategy = UpdateStrategy::Never;
		manga.url = Some(format!("{BASE_URL}/archive/{}/", manga.key));
		if needs_chapters {
			let archive: Archive =
				Request::get(format!("{BASE_URL}/api?archive={}", manga.key))?.json_owned()?;
			let download_base = archive.download.split("/download/").next().unwrap_or(&archive.download);
			manga.chapters = Some(vec![Chapter {
				key: Self::absolute_url(download_base),
				title: archive.title.or_else(|| Some("Chapter".into())),
				chapter_number: Some(1.0),
				date_uploaded: archive.posted,
				url: Some(format!("{BASE_URL}/archive/{}/", manga.key)),
				language: Some("multi".into()),
				..Default::default()
			}]);
		}
		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let zip_url = format!("{}/download/", chapter.key.trim_end_matches('/'));
		let entries = Self::zip_entries(&zip_url)?;
		if entries.is_empty() {
			bail!("No images were found in the archive.");
		}
		Ok(entries
			.into_iter()
			.map(|name| Page {
				content: PageContent::Zip(zip_url.clone(), name),
				..Default::default()
			})
			.collect())
	}
}

impl ListingProvider for PandaChaika {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		match listing.id.as_str() {
			"popular" => Self::search("", "rating", page),
			"latest" => Self::search("", "public_date", page),
			_ => bail!("Unknown listing"),
		}
	}
}

impl Home for PandaChaika {
	fn get_home(&self) -> Result<HomeLayout> {
		Ok(midoku_madara::home_layout(
			Self::search("", "rating", 1)?.entries,
			Self::search("", "public_date", 1)?.entries,
		))
	}
}

impl ImageRequestProvider for PandaChaika {
	fn get_image_request(
		&self,
		url: String,
		_context: Option<aidoku::PageContext>,
	) -> Result<Request> {
		Ok(Request::get(url)?.header("Referer", BASE_URL))
	}
}

impl DeepLinkHandler for PandaChaika {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url.strip_prefix(BASE_URL) else {
			return Ok(None);
		};
		let Some(id) = path.strip_prefix("/archive/").map(|value| value.trim_matches('/')) else {
			return Ok(None);
		};
		Ok((!id.is_empty()).then_some(DeepLinkResult::Manga { key: id.into() }))
	}
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
	let data = bytes
		.get(offset..offset + 2)
		.ok_or_else(|| aidoku::AidokuError::message("Invalid ZIP directory"))?;
	Ok(u16::from_le_bytes([data[0], data[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
	let data = bytes
		.get(offset..offset + 4)
		.ok_or_else(|| aidoku::AidokuError::message("Invalid ZIP directory"))?;
	Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

register_source!(
	PandaChaika,
	ListingProvider,
	Home,
	ImageRequestProvider,
	DeepLinkHandler
);
