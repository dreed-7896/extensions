#![no_std]

use aidoku::{
	alloc::{borrow::ToOwned, format, string::String, vec, vec::Vec},
	helpers::uri::{encode_uri_component, QueryParameters},
	imports::{net::Request, std::parse_date},
	prelude::*,
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Home, HomeLayout,
	ImageRequestProvider, Listing, ListingProvider, Manga, MangaPageResult, MangaStatus, Page,
	PageContent, Result, Source, UpdateStrategy,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use midoku_madara::is_blocked;
use serde_json::Value;

const BASE_URL: &str = "https://hentainexus.com";

struct HentaiNexus;

impl HentaiNexus {
	fn absolute_url(value: &str) -> String {
		if value.starts_with("http://") || value.starts_with("https://") {
			value.into()
		} else if value.starts_with('/') {
			format!("{BASE_URL}{value}")
		} else {
			format!("{BASE_URL}/{value}")
		}
	}

	fn parse_list(document: &aidoku::imports::html::Document) -> MangaPageResult {
		let entries = document
			.select(".container .column")
			.map(|elements| {
				elements
					.filter_map(|element| {
						let link = element.select_first("a")?;
						let href = link.attr("abs:href").or_else(|| link.attr("href"))?;
						let title = element
							.select_first(".card-header-title")?
							.text()?
							.trim()
							.to_owned();
						if title.is_empty() || is_blocked(&title) {
							return None;
						}
						let cover = element
							.select_first(".card-image img")
							.and_then(|image| image.attr("abs:src").or_else(|| image.attr("src")))
							.map(|url| Self::absolute_url(&url));
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
		let has_next_page = document.select_first("a.pagination-next[href]").is_some();
		MangaPageResult { entries, has_next_page }
	}

	fn list(path: &str) -> Result<MangaPageResult> {
		Ok(Self::parse_list(&Request::get(format!("{BASE_URL}{path}"))?.html()?))
	}

	fn decrypt_data(encoded: &str) -> Result<String> {
		let mut data = STANDARD
			.decode(encoded.as_bytes())
			.map_err(|_| aidoku::AidokuError::message("Invalid reader data"))?;
		let hostname = b"hentainexus.com";
		if data.len() < 64 || data.len() < hostname.len() {
			bail!("Invalid reader data");
		}
		for (index, value) in hostname.iter().enumerate() {
			data[index] ^= value;
		}

		let key_stream = &data[..64];
		let ciphertext = &data[64..];
		let mut digest = (0u16..=255).map(|value| value as u8).collect::<Vec<_>>();
		let primes = [2usize, 3, 5, 7, 11, 13, 17, 19];
		let mut prime_index = 0u8;
		for value in key_stream {
			prime_index ^= value;
			for _ in 0..8 {
				prime_index = if prime_index & 1 != 0 {
					(prime_index >> 1) ^ 12
				} else {
					prime_index >> 1
				};
			}
		}
		let q = primes[(prime_index & 7) as usize];

		let mut key = 0usize;
		for index in 0..256usize {
			key = (key + digest[index] as usize + key_stream[index % 64] as usize) % 256;
			digest.swap(index, key);
		}

		let mut k = 0usize;
		let mut n = 0usize;
		let mut p = 0usize;
		let mut xor_key = 0usize;
		let mut output = Vec::with_capacity(ciphertext.len());
		for value in ciphertext {
			k = (k + q) % 256;
			n = (p + digest[(n + digest[k] as usize) % 256] as usize) % 256;
			p = (p + k + digest[k] as usize) % 256;
			digest.swap(k, n);
			xor_key = digest[
				(n + digest[(k + digest[(xor_key + p) % 256] as usize) % 256] as usize) % 256
			] as usize;
			output.push(value ^ xor_key as u8);
		}
		String::from_utf8(output)
			.map_err(|_| aidoku::AidokuError::message("Invalid decrypted reader data"))
	}
}

impl Source for HentaiNexus {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let query = query.unwrap_or_default();
		if is_blocked(&query) {
			bail!("This search term is not supported.");
		}
		let page_path = if page > 1 {
			format!("/page/{page}")
		} else {
			String::new()
		};
		let mut params = QueryParameters::new();
		params.push("q", Some(query.trim()));
		Self::list(&format!("{page_path}?{params}"))
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
			.select_first("h1.title")
			.and_then(|element| element.text())
			.unwrap_or(manga.title);
		manga.cover = document
			.select_first("figure.image img")
			.and_then(|element| element.attr("abs:src").or_else(|| element.attr("src")))
			.map(|value| Self::absolute_url(&value))
			.or(manga.cover);

		let table = document
			.select_first(".view-page-details")
			.ok_or_else(|| aidoku::AidokuError::message("Missing title details"))?;
		let artists = table
			.select("td.viewcolumn:contains(Artist) + td a")
			.map(|elements| elements.filter_map(|element| element.text()).collect::<Vec<_>>())
			.unwrap_or_default();
		let authors = table
			.select("td.viewcolumn:contains(Author) + td a")
			.map(|elements| elements.filter_map(|element| element.text()).collect::<Vec<_>>())
			.unwrap_or_default();
		let mut creators = authors;
		for artist in artists {
			if !creators.contains(&artist) {
				creators.push(artist);
			}
		}
		manga.authors = (!creators.is_empty()).then_some(creators);
		manga.tags = table.select("span.tag a").and_then(|elements| {
			let tags = elements
				.filter_map(|element| element.text())
				.map(|tag| tag.split(" (").next().unwrap_or(&tag).trim().to_owned())
				.collect::<Vec<_>>();
			(!tags.is_empty()).then_some(tags)
		});
		manga.description = table
			.select_first("td.viewcolumn:contains(Description) + td")
			.and_then(|element| element.text());
		manga.status = MangaStatus::Completed;
		manga.update_strategy = UpdateStrategy::Never;
		manga.content_rating = ContentRating::NSFW;
		manga.url = Some(url);

		let tags = manga.tags.as_ref().map(|value| value.join(" ")).unwrap_or_default();
		if is_blocked(&format!("{} {tags}", manga.title)) {
			bail!("This title is not supported.");
		}

		if needs_chapters {
			let id = manga.key.trim_matches('/').split('/').next_back().unwrap_or(&manga.key);
			let published = table
				.select_first("td.viewcolumn:contains(Published) + td")
				.and_then(|element| element.text())
				.and_then(|date| parse_date(&date, "d MMMM yyyy"));
			manga.chapters = Some(vec![Chapter {
				key: format!("/read/{id}"),
				title: Some("Chapter".into()),
				chapter_number: Some(1.0),
				date_uploaded: published,
				url: Some(format!("{BASE_URL}/read/{id}")),
				language: Some("en".into()),
				..Default::default()
			}]);
		}
		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let document = Request::get(Self::absolute_url(&chapter.key))?.html()?;
		let script = document
			.select_first("script:containsData(initReader)")
			.and_then(|element| element.data())
			.ok_or_else(|| aidoku::AidokuError::message("Reader data was not found"))?;
		let encoded = script
			.split("initReader(\"")
			.nth(1)
			.and_then(|value| value.split("\",").next())
			.ok_or_else(|| aidoku::AidokuError::message("Reader data was not found"))?;
		let decrypted = Self::decrypt_data(encoded)?;
		let value: Value = serde_json::from_str(&decrypted)
			.map_err(|_| aidoku::AidokuError::message("Invalid reader data"))?;
		let images = value
			.as_array()
			.ok_or_else(|| aidoku::AidokuError::message("Invalid reader image list"))?;
		let pages = images
			.iter()
			.filter(|item| item.get("type").and_then(Value::as_str) == Some("image"))
			.filter_map(|item| {
				item.get("image_fallback")
					.or_else(|| item.get("image_avif"))
					.or_else(|| item.get("image_source"))
					.and_then(Value::as_str)
			})
			.map(|url| Page {
				content: PageContent::url(url),
				..Default::default()
			})
			.collect::<Vec<_>>();
		if pages.is_empty() {
			bail!("No readable pages were returned.");
		}
		Ok(pages)
	}
}

impl ListingProvider for HentaiNexus {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		match listing.id.as_str() {
			"latest" => {
				let path = if page > 1 { format!("/page/{page}") } else { String::new() };
				Self::list(&path)
			}
			"popular" if page == 1 => Self::list("/explore/hot"),
			"popular" => Self::list(&format!(
				"/page/{}?q={}",
				page - 1,
				encode_uri_component("sort:popular")
			)),
			_ => bail!("Unknown listing"),
		}
	}
}

impl Home for HentaiNexus {
	fn get_home(&self) -> Result<HomeLayout> {
		Ok(midoku_madara::home_layout(
			self.get_manga_list(Listing { id: "popular".into(), name: "Popular".into(), ..Default::default() }, 1)?.entries,
			self.get_manga_list(Listing { id: "latest".into(), name: "Latest".into(), ..Default::default() }, 1)?.entries,
		))
	}
}

impl ImageRequestProvider for HentaiNexus {
	fn get_image_request(
		&self,
		url: String,
		_context: Option<aidoku::PageContext>,
	) -> Result<Request> {
		Ok(Request::get(url)?.header("Referer", BASE_URL))
	}
}

impl DeepLinkHandler for HentaiNexus {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		Ok(url
			.strip_prefix(BASE_URL)
			.filter(|path| path.starts_with("/view/"))
			.map(|key| DeepLinkResult::Manga { key: key.into() }))
	}
}

register_source!(
	HentaiNexus,
	ListingProvider,
	Home,
	ImageRequestProvider,
	DeepLinkHandler
);
