#![no_std]

use aidoku::{
	alloc::{borrow::ToOwned, format, string::{String, ToString}, vec, vec::Vec},
	imports::{html::Element, net::Request},
	prelude::*,
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, ImageRequestProvider,
	Home, HomeComponent, HomeComponentValue, HomeLayout, Listing, ListingProvider, Manga,
	MangaPageResult, MangaStatus, Page, PageContent, Result, Source, UpdateStrategy,
};
use core::marker::PhantomData;

#[derive(Clone, Copy)]
pub struct Params {
	pub base_url: &'static str,
	pub manga_path: &'static str,
	pub popular_order: &'static str,
	pub latest_order: &'static str,
	pub ajax_chapters: bool,
}

pub trait Impl {
	fn new() -> Self;
	fn params(&self) -> Params;
}

pub struct Madara<I: Impl> {
	inner: I,
	_marker: PhantomData<I>,
}

impl<I: Impl> Madara<I> {
	fn params(&self) -> Params {
		self.inner.params()
	}

	fn absolute_url(&self, value: &str) -> String {
		if value.starts_with("http://") || value.starts_with("https://") {
			value.into()
		} else if value.starts_with('/') {
			format!("{}{}", self.params().base_url, value)
		} else {
			format!("{}/{}", self.params().base_url, value)
		}
	}

	fn manga_key(&self, url: String) -> String {
		url.strip_prefix(self.params().base_url)
			.map(ToOwned::to_owned)
			.unwrap_or(url)
	}

	fn image_url(&self, element: &Element) -> Option<String> {
		for key in ["data-src", "data-lazy-src", "data-cfsrc", "data-manga-src"] {
			if let Some(value) = element.attr(key) {
				let value = value.trim();
				if !value.is_empty() {
					return Some(self.absolute_url(value));
				}
			}
		}
		if let Some(srcset) = element.attr("srcset") {
			if let Some(candidate) = srcset
				.split(',')
				.filter_map(|part| part.split_whitespace().next())
				.filter(|part| !part.is_empty())
				.next_back()
			{
				return Some(self.absolute_url(candidate));
			}
		}
		element
			.attr("abs:src")
			.or_else(|| element.attr("src"))
			.filter(|value| !value.trim().is_empty())
			.map(|value| self.absolute_url(value.trim()))
	}

	fn parse_cards(&self, document: &aidoku::imports::html::Document) -> Vec<Manga> {
		document
			.select("div.page-item-detail, .manga__item, .c-tabs-item__content, .manga-item")
			.map(|elements| {
				elements
					.filter_map(|element| {
						let link = element
							.select_first(".post-title a, a.manga-item__link, h3 a, h2 a")?;
						let title = link.text()?.trim().to_owned();
						if title.is_empty() || is_blocked(&title) {
							return None;
						}
						let href = link.attr("abs:href").or_else(|| link.attr("href"))?;
						let cover = element.select_first("img").and_then(|image| self.image_url(&image));
						Some(Manga {
							key: self.manga_key(self.absolute_url(&href)),
							title,
							cover,
							content_rating: ContentRating::NSFW,
							..Default::default()
						})
					})
					.collect()
			})
			.unwrap_or_default()
	}

	fn browse(&self, order: &str, page: i32, query: Option<&str>) -> Result<MangaPageResult> {
		let params = self.params();
		let mut url = if query.is_some() {
			if page > 1 {
				format!("{}/page/{page}/", params.base_url)
			} else {
				format!("{}/", params.base_url)
			}
		} else if page > 1 {
			format!("{}/{}/page/{page}/", params.base_url, params.manga_path)
		} else {
			format!("{}/{}/", params.base_url, params.manga_path)
		};

		let mut query_params = aidoku::helpers::uri::QueryParameters::new();
		if let Some(query) = query.filter(|query| !query.trim().is_empty()) {
			if is_blocked(query) {
				bail!("This search term is not supported.");
			}
			query_params.push("s", Some(query));
			query_params.push("post_type", Some("wp-manga"));
		}
		if !order.is_empty() {
			query_params.push("m_orderby", Some(order));
		}
		if !query_params.is_empty() {
			url.push('?');
			url.push_str(&query_params.to_string());
		}

		let document = Request::get(url)?.html()?;
		let entries = self.parse_cards(&document);
		let has_next_page = document
			.select_first("div.nav-previous, a.nextpostslink, a[rel=next], .pagination .next")
			.is_some();
		Ok(MangaPageResult { entries, has_next_page })
	}

	fn parse_chapters(
		&self,
		document: &aidoku::imports::html::Document,
		manga_url: &str,
	) -> Vec<Chapter> {
		document
			.select("li.wp-manga-chapter")
			.map(|elements| {
				elements
					.filter_map(|element| {
						let link = element.select_first("a")?;
						let href = link.attr("abs:href").or_else(|| link.attr("href"))?;
						let title = link.text().map(|text| text.trim().to_owned());
						let chapter_number = title.as_ref().and_then(|text| extract_number(text));
						Some(Chapter {
							key: self.absolute_url(&href),
							title,
							chapter_number,
							url: Some(self.absolute_url(&href)),
							language: Some("en".into()),
							..Default::default()
						})
					})
					.collect::<Vec<_>>()
			})
			.unwrap_or_else(|| {
				vec![Chapter {
					key: manga_url.into(),
					title: Some("Chapter".into()),
					chapter_number: Some(1.0),
					url: Some(manga_url.into()),
					language: Some("en".into()),
					..Default::default()
				}]
			})
	}
}

impl<I: Impl> Source for Madara<I> {
	fn new() -> Self {
		Self {
			inner: I::new(),
			_marker: PhantomData,
		}
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		self.browse("", page, query.as_deref())
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		_needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let manga_url = self.absolute_url(&manga.key);
		let document = Request::get(&manga_url)?.html()?;

		manga.title = document
			.select_first("div.post-title h3, div.post-title h1, #manga-title > h1, h1")
			.and_then(|element| element.text())
			.unwrap_or(manga.title);
		manga.cover = document
			.select_first("div.summary_image img, .summary_image img, .manga-thumb img")
			.and_then(|element| self.image_url(&element))
			.or(manga.cover);
		manga.authors = collect_text(
			&document,
			"div.author-content > a, div.manga-authors > a, a[href*='/author/']",
		);
		manga.artists = collect_text(
			&document,
			"div.artist-content > a, a[href*='/artist/']",
		);
		manga.description = document
			.select_first("div.description-summary div.summary__content, div.summary_content div.manga-excerpt, .description-summary")
			.and_then(|element| element.text())
			.filter(|value| !value.trim().is_empty());
		manga.tags = collect_text(
			&document,
			"div.genres-content a, div.tags-content a, a[href*='/manga-genre/'], a[href*='/tag/']",
		);
		manga.status = document
			.select("div.summary-content, div.summary-heading:contains(Status) + div")
			.and_then(|mut elements| elements.next_back())
			.and_then(|element| element.text())
			.map(|status| status_from_text(&status))
			.unwrap_or(MangaStatus::Unknown);
		manga.url = Some(manga_url.clone());
		manga.content_rating = ContentRating::NSFW;

		let tags_text = manga.tags.as_ref().map(|tags| tags.join(" ")).unwrap_or_default();
		if is_blocked(&format!(
			"{} {} {}",
			manga.title,
			manga.description.as_deref().unwrap_or(""),
			tags_text
		)) {
			bail!("This title is not supported.");
		}

		if needs_chapters {
			let chapter_document = if self.params().ajax_chapters {
				Request::post(format!("{}/ajax/chapters/", manga_url.trim_end_matches('/')))?
					.header("X-Requested-With", "XMLHttpRequest")
					.header("Referer", manga_url.as_str())
					.body("")
					.html()?
			} else {
				document
			};
			manga.chapters = Some(self.parse_chapters(&chapter_document, &manga_url));
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let chapter_url = self.absolute_url(&chapter.key);
		let document = Request::get(&chapter_url)?.html()?;
		let pages = document
			.select("div.page-break img, li.blocks-gallery-item img, .reading-content img, .chapter-content img")
			.map(|elements| {
				elements
					.filter_map(|element| self.image_url(&element))
					.map(|url| Page {
						content: PageContent::url(url),
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

impl<I: Impl> ListingProvider for Madara<I> {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		match listing.id.as_str() {
			"popular" => self.browse(self.params().popular_order, page, None),
			"latest" => self.browse(self.params().latest_order, page, None),
			_ => bail!("Unknown listing"),
		}
	}
}

impl<I: Impl> Home for Madara<I> {
	fn get_home(&self) -> Result<HomeLayout> {
		let params = self.params();
		let popular = self.browse(params.popular_order, 1, None)?.entries;
		let latest = self.browse(params.latest_order, 1, None)?.entries;
		Ok(home_layout(popular, latest))
	}
}

pub fn home_layout(popular: Vec<Manga>, latest: Vec<Manga>) -> HomeLayout {
	HomeLayout {
		components: vec![
			HomeComponent {
				title: Some("Popular".into()),
				subtitle: None,
				value: HomeComponentValue::BigScroller {
					entries: popular,
					auto_scroll_interval: Some(6.0),
				},
			},
			HomeComponent {
				title: Some("Latest Updates".into()),
				subtitle: None,
				value: HomeComponentValue::MangaList {
					ranking: false,
					page_size: Some(20),
					entries: latest.into_iter().map(Into::into).collect(),
					listing: Some(Listing {
						id: "latest".into(),
						name: "Latest".into(),
						..Default::default()
					}),
				},
			},
		],
	}
}

impl<I: Impl> ImageRequestProvider for Madara<I> {
	fn get_image_request(
		&self,
		url: String,
		_context: Option<aidoku::PageContext>,
	) -> Result<Request> {
		Ok(Request::get(url)?.header("Referer", self.params().base_url))
	}
}

impl<I: Impl> DeepLinkHandler for Madara<I> {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		Ok(url
			.strip_prefix(self.params().base_url)
			.filter(|path| path.contains(&format!("/{}/", self.params().manga_path)))
			.map(|path| DeepLinkResult::Manga { key: path.into() }))
	}
}

fn collect_text(
	document: &aidoku::imports::html::Document,
	selector: &str,
) -> Option<Vec<String>> {
	document.select(selector).and_then(|elements| {
		let values = elements
			.filter_map(|element| element.text())
			.map(|text| text.trim().to_owned())
			.filter(|text| !text.is_empty())
			.collect::<Vec<_>>();
		(!values.is_empty()).then_some(values)
	})
}

fn status_from_text(value: &str) -> MangaStatus {
	let value = value.to_ascii_lowercase();
	if value.contains("completed") || value.contains("complete") {
		MangaStatus::Completed
	} else if value.contains("ongoing") || value.contains("on-going") {
		MangaStatus::Ongoing
	} else if value.contains("hiatus") || value.contains("hold") {
		MangaStatus::Hiatus
	} else if value.contains("cancel") {
		MangaStatus::Cancelled
	} else {
		MangaStatus::Unknown
	}
}

fn extract_number(value: &str) -> Option<f32> {
	let mut found = String::new();
	let mut started = false;
	for ch in value.chars() {
		if ch.is_ascii_digit() || (started && ch == '.') {
			found.push(ch);
			started = true;
		} else if started {
			break;
		}
	}
	found.parse().ok()
}

pub fn is_blocked(value: &str) -> bool {
	let value = value.to_ascii_lowercase();
	[
		"lolicon",
		"shotacon",
		" loli ",
		" shota ",
		"underage",
		"minor character",
	]
	.iter()
	.any(|term| value.contains(term))
}
