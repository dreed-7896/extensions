#![no_std]

mod metadata;
mod s3;
mod sigv4;
mod util;
mod zip;

use aidoku::{
	alloc::{borrow::ToOwned, format, string::String, vec, vec::Vec},
	imports::defaults::defaults_get,
	prelude::*,
	Chapter, ContentRating, FilterValue, Home, HomeComponent, HomeComponentValue, HomeLayout,
	Link, Listing, ListingProvider, Manga, MangaPageResult, Page, PageContent, Result, Source,
	UpdateStrategy,
};

use metadata::{Metadata, RemoteChapter};
use s3::{list_all, object_text, S3Listing, S3Object};
use sigv4::R2Config;
use util::{chapter_number, file_name, is_absolute_url, is_archive, is_cover, is_hidden, is_image, natural_cmp};
use zip::archive_pages;

const PAGE_SIZE: usize = 30;
const MAX_METADATA_BYTES: u64 = 1024 * 1024;
const REMOTE_PAGES_MARKER: &str = "#r2-pages=";

struct R2Library;

#[derive(Clone)]
struct Series {
	name: String,
	updated: i64,
}

#[derive(Clone)]
struct RawChapter {
	key: String,
	label: String,
	date: Option<i64>,
	number: Option<f32>,
	scanlator: Option<String>,
}

impl R2Library {
	fn all_series(config: &R2Config) -> Result<Vec<Series>> {
		let listing = list_all(config, &config.root_prefix, Some("/"), 50)?;
		let mut series = listing
			.prefixes
			.into_iter()
			.filter_map(|prefix| {
				let name = prefix.strip_prefix(&config.root_prefix)?.trim_end_matches('/');
				(!name.is_empty() && !name.starts_with('.') && !name.eq_ignore_ascii_case("upload"))
					.then(|| Series { name: name.to_owned(), updated: 0 })
			})
			.collect::<Vec<_>>();
		series.sort_by(|left, right| natural_cmp(&left.name, &right.name));
		if series.is_empty() {
			bail!("No series folders were found. Expected <series>/<chapter>/<page>.jpg in the configured bucket folder.");
		}
		Ok(series)
	}

	fn recent_series(config: &R2Config) -> Result<Vec<Series>> {
		let max_pages = defaults_get::<String>("r2_latest_pages")
			.and_then(|value| value.parse::<usize>().ok())
			.unwrap_or(10)
			.clamp(1, 100);
		let listing = list_all(config, &config.root_prefix, None, max_pages)?;
		let mut series: Vec<Series> = Vec::new();
		for object in listing.objects {
			if is_hidden(&object.key) {
				continue;
			}
			let Some(relative) = object.key.strip_prefix(&config.root_prefix) else { continue };
			let name = relative.split('/').next().unwrap_or("");
			if name.is_empty() || name.starts_with('.') || name.eq_ignore_ascii_case("upload") {
				continue;
			}
			if let Some(existing) = series.iter_mut().find(|entry| entry.name == name) {
				existing.updated = existing.updated.max(object.last_modified);
			} else {
				series.push(Series { name: name.into(), updated: object.last_modified });
			}
		}
		series.sort_by(|left, right| right.updated.cmp(&left.updated).then_with(|| natural_cmp(&left.name, &right.name)));
		if series.is_empty() {
			bail!("No library objects were found in the configured bucket folder.");
		}
		Ok(series)
	}

	fn series_prefix(config: &R2Config, name: &str) -> String {
		let mut name = name.trim_matches('/');
		if !config.root_prefix.is_empty() && name.starts_with(config.root_prefix.as_str()) {
			name = name.strip_prefix(config.root_prefix.as_str()).unwrap_or(name);
		}
		format!("{}{}/", config.root_prefix, name)
	}

	fn cards(config: &R2Config, series: &[Series]) -> Vec<Manga> {
		series
			.iter()
			.map(|entry| {
				let prefix = Self::series_prefix(config, &entry.name);
				let cover = list_all(config, &prefix, Some("/"), 1)
					.ok()
					.and_then(|listing| {
						Self::cover_object(&prefix, &listing)
							.map(|object| config.content_url(&object.key))
					});
				Manga {
					key: entry.name.clone(),
					title: entry.name.clone(),
					cover,
					content_rating: ContentRating::NSFW,
					update_strategy: UpdateStrategy::Always,
					..Default::default()
				}
			})
			.collect()
	}

	fn cover_object<'a>(prefix: &str, listing: &'a S3Listing) -> Option<&'a S3Object> {
		let direct = listing.objects.iter().filter(|object| {
			object.key.strip_prefix(prefix).map(|value| !value.contains('/')).unwrap_or(false) && is_image(&object.key)
		});
		direct.clone().find(|object| is_cover(&object.key)).or_else(|| direct.min_by(|left, right| natural_cmp(&left.key, &right.key)))
	}

	fn page(series: Vec<Series>, page: i32, config: &R2Config) -> MangaPageResult {
		let page = page.max(1) as usize;
		let start = (page - 1) * PAGE_SIZE;
		let slice = series.iter().skip(start).take(PAGE_SIZE).cloned().collect::<Vec<_>>();
		MangaPageResult {
			entries: Self::cards(config, &slice),
			has_next_page: series.len() > start + slice.len(),
		}
	}

	fn browse(query: Option<&str>, page: i32, filters: Vec<FilterValue>) -> Result<MangaPageResult> {
		let config = R2Config::load()?;
		let mut series = Self::all_series(&config)?;
		if let Some(query) = query.map(str::trim).filter(|value| !value.is_empty()) {
			let query = query.to_ascii_lowercase();
			series.retain(|entry| entry.name.to_ascii_lowercase().contains(&query));
		}
		let ascending = filters.into_iter().find_map(|filter| match filter {
			FilterValue::Sort { id, ascending, .. } if id == "sort" => Some(ascending),
			_ => None,
		}).unwrap_or(true);
		if !ascending {
			series.reverse();
		}
		Ok(Self::page(series, page, &config))
	}

	fn read_metadata(config: &R2Config, listing: &S3Listing) -> Option<Metadata> {
		let details = listing.objects.iter().find(|object| file_name(&object.key).eq_ignore_ascii_case("details.json"));
		if let Some(object) = details {
			if let Ok(raw) = object_text(config, object, MAX_METADATA_BYTES) {
				if let Some(metadata) = metadata::details_json(&raw) {
					return Some(metadata);
				}
			}
		}
		let comic_info = listing.objects.iter().find(|object| file_name(&object.key).eq_ignore_ascii_case("ComicInfo.xml"));
		comic_info
			.and_then(|object| object_text(config, object, MAX_METADATA_BYTES).ok())
			.and_then(|raw| metadata::comic_info(&raw))
	}

	fn chapters(config: &R2Config, prefix: &str, listing: &S3Listing) -> Result<Vec<Chapter>> {
		let mut folder_dates: Vec<(String, i64)> = Vec::new();
		let mut archives: Vec<&S3Object> = Vec::new();
		let mut loose_images: Vec<&S3Object> = Vec::new();
		for object in &listing.objects {
			if is_hidden(&object.key) {
				continue;
			}
			if is_archive(&object.key) {
				archives.push(object);
				continue;
			}
			if !is_image(&object.key) {
				continue;
			}
			let relative = object.key.strip_prefix(prefix).unwrap_or(&object.key);
			if !relative.contains('/') {
				loose_images.push(object);
			} else if let Some((folder, _)) = object.key.rsplit_once('/') {
				let folder = format!("{folder}/");
				if let Some((_, date)) = folder_dates.iter_mut().find(|(key, _)| *key == folder) {
					*date = (*date).max(object.last_modified);
				} else {
					folder_dates.push((folder, object.last_modified));
				}
			}
		}

		let series_name = prefix.trim_end_matches('/').rsplit('/').next().unwrap_or(prefix);
		let mut raw = folder_dates
			.into_iter()
			.map(|(key, date)| RawChapter {
				label: key.strip_prefix(prefix).unwrap_or(&key).trim_end_matches('/').replace('/', " – "),
				key,
				date: (date > 0).then_some(date),
				number: None,
				scanlator: None,
			})
			.collect::<Vec<_>>();
		raw.extend(archives.into_iter().map(|object| RawChapter {
			key: object.key.clone(),
			label: object.key.strip_prefix(prefix).unwrap_or(&object.key).rsplit_once('.').map(|(name, _)| name).unwrap_or(file_name(&object.key)).replace('/', " – "),
			date: (object.last_modified > 0).then_some(object.last_modified),
			number: None,
			scanlator: None,
		}));
		let flat_pages = loose_images.iter().filter(|object| !is_cover(&object.key)).collect::<Vec<_>>();
		if raw.is_empty() && !flat_pages.is_empty() {
			raw.push(RawChapter {
				key: prefix.into(),
				label: series_name.into(),
				date: flat_pages.iter().map(|object| object.last_modified).max().filter(|date| *date > 0),
				number: Some(1.0),
				scanlator: None,
			});
		}

		if let Some(object) = listing.objects.iter().find(|object| file_name(&object.key).eq_ignore_ascii_case("chapters.json")) {
			if let Ok(text) = object_text(config, object, MAX_METADATA_BYTES) {
				for remote in metadata::chapter_list(&text) {
					raw.push(Self::remote_raw(object, remote));
				}
			}
		}
		if raw.is_empty() {
			bail!("No chapters were found in {series_name}. Use image folders, CBZ/ZIP files, or chapters.json.");
		}
		raw.sort_by(|left, right| natural_cmp(&left.label, &right.label));
		raw.dedup_by(|left, right| left.key == right.key);
		let mut chapters = raw
			.into_iter()
			.enumerate()
			.map(|(index, raw)| Chapter {
				key: raw.key,
				title: Some(raw.label.clone()),
				chapter_number: raw.number.or_else(|| chapter_number(&raw.label)).or(Some((index + 1) as f32)),
				date_uploaded: raw.date,
				scanlators: raw.scanlator.map(|value| vec![value]),
				language: Some("en".into()),
				..Default::default()
			})
			.collect::<Vec<_>>();
		chapters.reverse();
		Ok(chapters)
	}

	fn remote_raw(object: &S3Object, remote: RemoteChapter) -> RawChapter {
		let key = remote.archive_url.clone().unwrap_or_else(|| {
			format!("{}{}{}", object.key, REMOTE_PAGES_MARKER, remote.name)
		});
		RawChapter {
			key,
			label: remote.name,
			date: remote.date_uploaded,
			number: remote.number,
			scanlator: remote.scanlator,
		}
	}

	fn declared_pages(config: &R2Config, chapter_key: &str) -> Result<Vec<Page>> {
		let Some((file, name)) = chapter_key.split_once(REMOTE_PAGES_MARKER) else {
			bail!("Invalid chapters.json entry.");
		};
		let object = S3Object { key: file.into(), size: 0, last_modified: 0 };
		let text = object_text(config, &object, MAX_METADATA_BYTES)?;
		let remote = metadata::chapter_list(&text).into_iter().find(|chapter| chapter.name == name)
			.ok_or_else(|| aidoku::AidokuError::message("This chapter is no longer in chapters.json"))?;
		Ok(remote.pages.into_iter().map(|url| Page { content: PageContent::url(url), ..Default::default() }).collect())
	}
}

impl Source for R2Library {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		Self::browse(query.as_deref(), page, filters)
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		_needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let config = R2Config::load()?;
		let prefix = Self::series_prefix(&config, &manga.key);
		let listing = list_all(&config, &prefix, None, 50)?;
		let metadata = Self::read_metadata(&config, &listing);
		if let Some(metadata) = metadata {
			manga.title = metadata.title.unwrap_or(manga.title);
			manga.authors = metadata.author.map(|value| vec![value]);
			manga.artists = metadata.artist.map(|value| vec![value]);
			manga.description = metadata.description;
			manga.tags = metadata.genres;
			manga.status = metadata.status;
		}
		manga.cover = Self::cover_object(&prefix, &listing)
			.map(|object| config.content_url(&object.key))
			.or(manga.cover);
		manga.content_rating = ContentRating::NSFW;
		manga.update_strategy = UpdateStrategy::Always;
		if needs_chapters {
			manga.chapters = Some(Self::chapters(&config, &prefix, &listing)?);
		}
		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let config = R2Config::load()?;
		if chapter.key.contains(REMOTE_PAGES_MARKER) {
			let pages = Self::declared_pages(&config, &chapter.key)?;
			if pages.is_empty() { bail!("No pages were listed for this chapter."); }
			return Ok(pages);
		}
		if chapter.key.ends_with('/') {
			let listing = list_all(&config, &chapter.key, None, 50)?;
			let mut images = listing.objects.into_iter().filter(|object| is_image(&object.key)).collect::<Vec<_>>();
			images.sort_by(|left, right| natural_cmp(&left.key, &right.key));
			if images.len() > 1 {
				images.retain(|object| !is_cover(&object.key));
			}
			let pages = images.into_iter().map(|object| Page {
				content: PageContent::url(config.content_url(&object.key)),
				..Default::default()
			}).collect::<Vec<_>>();
			if pages.is_empty() { bail!("No images were found in this chapter folder."); }
			return Ok(pages);
		}
		if is_archive(&chapter.key) || is_absolute_url(&chapter.key) {
			let (url, entries) = archive_pages(&config, &chapter.key)?;
			return Ok(entries.into_iter().map(|path| Page {
				content: PageContent::Zip(url.clone(), path),
				..Default::default()
			}).collect());
		}
		bail!("This chapter is not an image folder or ZIP/CBZ archive.")
	}
}

impl ListingProvider for R2Library {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let config = R2Config::load()?;
		match listing.id.as_str() {
			"library" => Ok(Self::page(Self::all_series(&config)?, page, &config)),
			"recent" => Ok(Self::page(Self::recent_series(&config)?, page, &config)),
			_ => bail!("Unknown listing."),
		}
	}
}

impl Home for R2Library {
	fn get_home(&self) -> Result<HomeLayout> {
		let config = R2Config::load()?;
		let recent_series = Self::recent_series(&config)?.into_iter().take(8).collect::<Vec<_>>();
		let library_series = Self::all_series(&config)?.into_iter().take(12).collect::<Vec<_>>();
		let recent = Self::cards(&config, &recent_series);
		let library = Self::cards(&config, &library_series);
		Ok(HomeLayout {
			components: vec![
				HomeComponent {
					title: Some("Recently Updated".into()),
					subtitle: None,
					value: HomeComponentValue::BigScroller { entries: recent, auto_scroll_interval: None },
				},
				HomeComponent {
					title: Some("Library".into()),
					subtitle: None,
					value: HomeComponentValue::MangaList {
						ranking: false,
						page_size: Some(12),
						entries: library.into_iter().map(Link::from).collect(),
						listing: Some(Listing { id: "library".into(), name: "Library".into(), ..Default::default() }),
					},
				},
			],
		})
	}
}

register_source!(R2Library, ListingProvider, Home);
