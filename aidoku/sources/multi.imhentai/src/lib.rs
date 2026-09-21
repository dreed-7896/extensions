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
	imports::{defaults::defaults_get, html::Document, net::Request},
	prelude::*,
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Home, HomeLayout,
	ImageRequestProvider, Listing, ListingProvider, Manga, MangaPageResult, MangaStatus, Page,
	PageContent, PageContext, Result, Source, UpdateStrategy,
};
use midoku_madara::is_blocked;
use serde_json::Value;

const BASE_URL: &str = "https://imhentai.xxx";
const CATEGORY_IDS: [&str; 6] = ["m", "d", "w", "i", "a", "g"];
const LANGUAGE_FLAGS: [(&str, &str); 7] = [
	("en", "en"),
	("ja", "jp"),
	("es", "es"),
	("fr", "fr"),
	("ko", "kr"),
	("de", "de"),
	("ru", "ru"),
];

struct IMHentai;

impl IMHentai {
	fn absolute_url(value: &str) -> String {
		let value = value.trim();
		if value.starts_with("http://") || value.starts_with("https://") {
			value.into()
		} else if value.starts_with("//") {
			format!("https:{value}")
		} else if value.starts_with('/') {
			format!("{BASE_URL}{value}")
		} else {
			format!("{BASE_URL}/{value}")
		}
	}

	fn key_from_url(url: &str) -> String {
		url.strip_prefix(BASE_URL).unwrap_or(url).into()
	}

	fn selected_language_code() -> Option<String> {
		defaults_get::<String>("language")
			.filter(|language| LANGUAGE_FLAGS.iter().any(|(code, _)| language == code))
	}

	fn language_slug() -> Option<&'static str> {
		let language = Self::selected_language_code()?;
		match language.as_str() {
			"en" => Some("english"),
			"ja" => Some("japanese"),
			"es" => Some("spanish"),
			"fr" => Some("french"),
			"ko" => Some("korean"),
			"de" => Some("german"),
			"ru" => Some("russian"),
			_ => None,
		}
	}

	fn image_url(element: &aidoku::imports::html::Element) -> Option<String> {
		for attribute in ["data-src", "data-original", "data-lazy-src", "src"] {
			if let Some(value) = element
				.attr(attribute)
				.filter(|value| !value.trim().is_empty())
			{
				return Some(Self::absolute_url(&value));
			}
		}
		element.attr("abs:src")
	}

	fn card_cover(element: &aidoku::imports::html::Element) -> Option<String> {
		for selector in [
			".inner_thumb img[data-src]:not(.thumb_flag)",
			".inner_thumb img[data-original]:not(.thumb_flag)",
			".inner_thumb img[data-lazy-src]:not(.thumb_flag)",
			".inner_thumb a img:not(.thumb_flag)",
			".inner_thumb img:not(.thumb_flag)",
		] {
			let Some(image) = element.select_first(selector) else {
				continue;
			};
			let Some(url) = Self::image_url(&image) else {
				continue;
			};
			let lowercase_url = url.to_ascii_lowercase();
			if !lowercase_url.contains("/flags/")
				&& !lowercase_url.contains("/flag/")
				&& !lowercase_url.contains("thumb_flag")
			{
				return Some(url);
			}
		}
		None
	}

	fn parse_cards(document: &Document) -> MangaPageResult {
		let entries = document
			.select("div.thumb")
			.map(|elements| {
				elements
					.filter_map(|element| {
						let link = element.select_first(".inner_thumb a")?;
						let title = element
							.select_first(".caption")
							.and_then(|caption| caption.text())
							.or_else(|| link.attr("title"))?
							.trim()
							.to_owned();
						if title.is_empty() || is_blocked(&title) {
							return None;
						}
						let href = link.attr("abs:href").or_else(|| link.attr("href"))?;
						let url = Self::absolute_url(&href);
						let cover = Self::card_cover(&element);
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
			.unwrap_or_default();
		let has_next_page = document
			.select_first(".pagination li.active + li:not(.disabled), a[rel=next]")
			.is_some();
		MangaPageResult {
			entries,
			has_next_page,
		}
	}

	fn browse(popular: bool, page: i32) -> Result<MangaPageResult> {
		let mut url = String::from(BASE_URL);
		url.push('/');
		if let Some(language) = Self::language_slug() {
			url.push_str(&format!("language/{language}/"));
		}
		if popular {
			url.push_str("popular/");
		}
		url.push_str(&format!("?page={page}"));
		Ok(Self::parse_cards(&Request::get(url)?.html()?))
	}

	fn push_advanced_terms(target: &mut Vec<String>, namespace: &str, value: &str) {
		for raw_term in value.split(',') {
			let raw_term = raw_term.trim();
			if raw_term.is_empty() {
				continue;
			}
			let excluded = raw_term.starts_with('-');
			let term = raw_term.trim_start_matches('-').trim();
			if !term.is_empty() {
				target.push(format!(
					"{}{namespace}:\"{term}\"",
					if excluded { "-" } else { "+" }
				));
			}
		}
	}

	fn search(
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let query = query.unwrap_or_default();
		if is_blocked(&query) {
			bail!("This search term is not supported.");
		}

		let mut sort_index = 0;
		let mut selected_categories: Option<Vec<String>> = None;
		let mut advanced_terms = Vec::new();
		let mut speechless = false;
		for filter in filters {
			match filter {
				FilterValue::Sort { id, index, .. } if id == "sort" => sort_index = index,
				FilterValue::MultiSelect { id, included, .. } if id == "categories" => {
					selected_categories = Some(included);
				}
				FilterValue::Text { id, value } if !value.trim().is_empty() => {
					let namespace = match id.as_str() {
						"tags" => Some("tag"),
						"parodies" => Some("parody"),
						"artists" => Some("artist"),
						"characters" => Some("character"),
						"groups" => Some("group"),
						_ => None,
					};
					if let Some(namespace) = namespace {
						Self::push_advanced_terms(&mut advanced_terms, namespace, &value);
					}
				}
				FilterValue::Select { id, value } if id == "speechless" => {
					speechless = value == "on";
				}
				_ => {}
			}
		}

		if speechless {
			let path = if sort_index == 0 { "popular/" } else { "" };
			return Ok(Self::parse_cards(
				&Request::get(format!(
					"{BASE_URL}/language/speechless/{path}?page={page}"
				))?
				.html()?,
			));
		}

		let mut parameters = QueryParameters::new();
		for (index, flag) in ["pp", "lt", "dl", "tr"].iter().enumerate() {
			parameters.push(
				flag,
				Some(if sort_index == index as i32 { "1" } else { "0" }),
			);
		}

		let categories = selected_categories.unwrap_or_else(|| {
			CATEGORY_IDS
				.iter()
				.map(|value| (*value).to_string())
				.collect()
		});
		for category in CATEGORY_IDS {
			parameters.push(
				category,
				Some(if categories.iter().any(|value| value == category) {
					"1"
				} else {
					"0"
				}),
			);
		}

		let language = Self::selected_language_code();
		for (code, flag) in LANGUAGE_FLAGS {
			parameters.push(
				flag,
				Some(
					if language
						.as_deref()
						.map(|selected| selected == code)
						.unwrap_or(true)
					{
						"1"
					} else {
						"0"
					},
				),
			);
		}

		let key = if advanced_terms.is_empty() {
			query.trim().to_owned()
		} else {
			advanced_terms.join(" ")
		};
		parameters.push("key", Some(&key));
		parameters.push("page", Some(&page.to_string()));

		let url = format!("{BASE_URL}/search/?{parameters}");
		let document = Request::get(url)?.html()?;
		let result = Self::parse_cards(&document);
		if result.entries.is_empty() {
			let overloaded = document
				.select_first("body")
				.and_then(|body| body.text())
				.map(|text| text.contains("Overload... Please use the advanced search"))
				.unwrap_or(false);
			if overloaded {
				bail!(
					"IMHentai search is overloaded. Try again later or use advanced filters."
				);
			}
		}
		Ok(result)
	}

	fn text_list(
		root: &aidoku::imports::html::Element,
		selector: &str,
	) -> Option<Vec<String>> {
		root.select(selector).and_then(|elements| {
			let values = elements
				.filter_map(|element| element.text())
				.map(|value| value.trim().to_owned())
				.filter(|value| !value.is_empty())
				.collect::<Vec<_>>();
			(!values.is_empty()).then_some(values)
		})
	}

	fn input_value(document: &Document, id: &str) -> Option<String> {
		document
			.select_first(&format!("input#{id}"))
			.and_then(|element| element.attr("value"))
			.filter(|value| !value.is_empty())
	}

	fn host_from_url(url: &str) -> Option<&str> {
		url.split_once("://")?.1.split('/').next()
	}

	fn full_image_from_thumbnail(url: String) -> String {
		let Some((stem, extension)) = url.rsplit_once('.') else {
			return url;
		};
		if let Some(stem) = stem.strip_suffix('t') {
			format!("{stem}.{extension}")
		} else {
			url
		}
	}
}

impl Source for IMHentai {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		Self::search(query, page, filters)
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		_needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let url = Self::absolute_url(&manga.key);
		let document = Request::get(&url)?.html()?;
		let info = document
			.select_first(".gallery_first")
			.ok_or_else(|| {
				aidoku::AidokuError::message("IMHentai gallery details were not found")
			})?;

		manga.title = info
			.select_first("h1")
			.and_then(|element| element.text())
			.map(|title| title.trim().to_owned())
			.unwrap_or(manga.title);
		manga.cover = info
			.select_first(".left_cover img, .cover img")
			.and_then(|image| Self::image_url(&image))
			.or(manga.cover);
		manga.artists = Self::text_list(&info, "li:contains(Artists:) a.tag");
		manga.authors = Self::text_list(&info, "li:contains(Groups:) a.tag");
		manga.tags = Self::text_list(&info, "li:contains(Tags:) a.tag");
		let mut description_lines = Vec::new();
		for (label, selector) in [
			("Parodies", "li:contains(Parodies:) a.tag"),
			("Characters", "li:contains(Characters:) a.tag"),
			("Groups", "li:contains(Groups:) a.tag"),
			("Languages", "li:contains(Languages:) a.tag"),
			("Categories", "li:contains(Categories:) a.tag"),
		] {
			if let Some(values) = Self::text_list(&info, selector) {
				description_lines.push(format!("{label}: {}", values.join(", ")));
			}
		}
		manga.description =
			(!description_lines.is_empty()).then_some(description_lines.join("\n"));
		manga.status = MangaStatus::Completed;
		manga.url = Some(url.clone());
		manga.content_rating = ContentRating::NSFW;
		manga.update_strategy = UpdateStrategy::Never;

		let searchable = format!(
			"{} {}",
			manga.title,
			manga
				.tags
				.as_ref()
				.map(|tags| tags.join(" "))
				.unwrap_or_default()
		);
		if is_blocked(&searchable) {
			bail!("This title is not supported.");
		}

		if needs_chapters {
			manga.chapters = Some(vec![Chapter {
				key: manga.key.clone(),
				title: Some("Chapter".into()),
				chapter_number: Some(1.0),
				url: Some(url),
				language: Self::selected_language_code(),
				..Default::default()
			}]);
		}
		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let document = Request::get(Self::absolute_url(&chapter.key))?.html()?;
		let script = document.select("script").and_then(|elements| {
			elements
				.filter_map(|element| element.data())
				.find(|data| data.contains("parseJSON"))
		});

		if let Some(script) = script {
			if let Some(marker_start) = script.find("$.parseJSON('") {
				let json_start = marker_start + "$.parseJSON('".len();
				if let Some(relative_end) = script[json_start..].find("');") {
					let json = &script[json_start..json_start + relative_end];
					if let Ok(value) = serde_json::from_str::<Value>(json) {
						if let Some(images) = value.as_object() {
							let load_dir = Self::input_value(&document, "load_dir");
							let load_id = Self::input_value(&document, "load_id");
							let gallery_id = Self::input_value(&document, "gallery_id");
							let cover = document
								.select_first(".left_cover img, .cover img")
								.and_then(|image| Self::image_url(&image));
							let server = Self::input_value(&document, "load_server")
								.map(|number| format!("m{number}.imhentai.xxx"))
								.or_else(|| {
									cover
										.as_deref()
										.and_then(Self::host_from_url)
										.map(str::to_owned)
								});
							if let (
								Some(load_dir),
								Some(load_id),
								Some(_gallery_id),
								Some(server),
							) = (load_dir, load_id, gallery_id, server)
							{
								let mut parsed = images
									.iter()
									.filter_map(|(index, data)| {
										let index_number = index.parse::<u32>().ok()?;
										let kind = data.as_str()?.split(',').next()?;
										let extension = match kind {
											"p" => "png",
											"b" => "bmp",
											"g" => "gif",
											"w" => "webp",
											_ => "jpg",
										};
										Some((
											index_number,
											format!(
												"https://{server}/{load_dir}/{load_id}/{index}.{extension}"
											),
										))
									})
									.collect::<Vec<_>>();
								parsed.sort_by_key(|(index, _)| *index);
								let pages = parsed
									.into_iter()
									.map(|(_, url)| Page {
										content: PageContent::url(url),
										..Default::default()
									})
									.collect::<Vec<_>>();
								if !pages.is_empty() {
									return Ok(pages);
								}
							}
						}
					}
				}
			}
		}

		let pages = document
			.select(".gthumb img, .gallery_thumb img")
			.map(|elements| {
				elements
					.filter_map(|image| Self::image_url(&image))
					.map(Self::full_image_from_thumbnail)
					.map(|url| Page {
						content: PageContent::url(url),
						..Default::default()
					})
					.collect::<Vec<_>>()
			})
			.unwrap_or_default();
		if pages.is_empty() {
			bail!("IMHentai reader images were not found.");
		}
		Ok(pages)
	}
}

impl ListingProvider for IMHentai {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		match listing.id.as_str() {
			"popular" => Self::browse(true, page),
			"latest" => Self::browse(false, page),
			_ => bail!("Unknown listing"),
		}
	}
}

impl Home for IMHentai {
	fn get_home(&self) -> Result<HomeLayout> {
		Ok(midoku_madara::home_layout(
			Self::browse(true, 1)?.entries,
			Self::browse(false, 1)?.entries,
		))
	}
}

impl ImageRequestProvider for IMHentai {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		Ok(Request::get(url)?.header("Referer", BASE_URL))
	}
}

impl DeepLinkHandler for IMHentai {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url.strip_prefix(BASE_URL) else {
			return Ok(None);
		};
		let mut components = path.trim_matches('/').split('/');
		if components.next() != Some("gallery") {
			return Ok(None);
		}
		let Some(id) = components.next().filter(|id| !id.is_empty()) else {
			return Ok(None);
		};
		Ok(Some(DeepLinkResult::Manga {
			key: format!("/gallery/{id}/"),
		}))
	}
}

register_source!(
	IMHentai,
	ListingProvider,
	Home,
	ImageRequestProvider,
	DeepLinkHandler
);
