#![no_std]

mod parser;

use aidoku::{
    Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, Home, HomeLayout, ImageRequestProvider,
    Listing, ListingProvider, Manga, MangaPageResult, MangaStatus, Page, PageContent, PageContext,
    Result, Source, UpdateStrategy, Viewer,
    alloc::{borrow::ToOwned, format, string::String, vec, vec::Vec},
    helpers::uri::QueryParameters,
    imports::{
        defaults::defaults_get,
        html::{Document, Element},
        net::Request,
        std::parse_date_with_options,
    },
    prelude::*,
};

struct ManhwaRead;

impl ManhwaRead {
    fn base_url() -> &'static str {
        match defaults_get::<String>("domain").as_deref() {
            Some("https://manhwaread.org") => parser::DOMAINS[1],
            _ => parser::DOMAINS[0],
        }
    }

    fn url(key: &str) -> String {
        parser::absolute_url(Self::base_url(), parser::path(key).unwrap_or(key))
    }

    fn document(url: &str) -> Result<Document> {
        let document = Request::get(url)?
            .header("Referer", Self::base_url())
            .html()?;
        if document
            .select_first("title")
            .and_then(|title| title.text())
            .map(|title| title.contains("Site Unavailable") || title.contains("Just a moment"))
            .unwrap_or(false)
        {
            bail!(
                "ManhwaRead is unavailable. Try the other domain in source settings, or open the website and retry."
            );
        }
        Ok(document)
    }

    fn image_url(element: &Element) -> Option<String> {
        ["data-src", "data-lazy-src", "src"]
            .into_iter()
            .find_map(|key| {
                element
                    .attr(key)
                    .filter(|value| !value.trim().is_empty() && !value.starts_with("data:"))
            })
            .map(|value| parser::absolute_url(Self::base_url(), &value))
    }

    fn parse_list(document: &Document) -> MangaPageResult {
        let entries = document
            .select(".main-container .manga-item")
            .map(|elements| {
                elements
                    .filter_map(|element| {
                        let link = element.select_first("a.manga-item__link")?;
                        let title = link.text()?.trim().to_owned();
                        if title.is_empty() {
                            return None;
                        }
                        let key = parser::manga_key(&link.attr("href")?)?;
                        Some(Manga {
                            key,
                            title,
                            cover: element
                                .select_first(".manga-item__img img")
                                .and_then(|image| Self::image_url(&image)),
                            viewer: Viewer::Webtoon,
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        MangaPageResult {
            entries,
            has_next_page: document
                .select_first(".wp-pagenavi a.last, .wp-pagenavi a.nextpostslink, a[rel=next]")
                .is_some(),
        }
    }

    fn search_url(query: Option<&str>, page: i32, filters: Vec<FilterValue>) -> String {
        let mut params = QueryParameters::new();
        params.push("s", Some(query.unwrap_or("")));
        // Keiyoushi's default search sorts by daily views, descending.
        let mut sort = "daily_top";
        let mut ascending = false;
        for filter in filters {
            match filter {
                FilterValue::Sort {
                    id,
                    index,
                    ascending: asc,
                } if id == "sort" => {
                    sort = match index {
                        0 => "release",
                        1 => "new",
                        2 => "alphabet",
                        3 => "rating",
                        4 => "bookmarks",
                        5 => "chapters",
                        6 => "comments",
                        8 => "weekly_top",
                        9 => "monthly_top",
                        10 => "yearly_top",
                        11 => "all_top",
                        _ => "daily_top",
                    };
                    ascending = asc;
                }
                FilterValue::Select { id, value } if !value.is_empty() => {
                    let key = match id.as_str() {
                        "keyword_mode" => "keyword_mode",
                        "tag_mode" => "s_mode",
                        "status" => "status",
                        _ => continue,
                    };
                    params.push(key, Some(&value));
                }
                FilterValue::MultiSelect {
                    id,
                    included,
                    excluded,
                } => {
                    let key = match id.as_str() {
                        "artists" => "artists[]",
                        "authors" => "authors[]",
                        "publishers" => "publishers[]",
                        "genres" => "genres[]",
                        "tags" => "including[]",
                        _ => continue,
                    };
                    for value in included {
                        params.push(key, Some(&value));
                    }
                    if id == "tags" {
                        for value in excluded {
                            params.push("excluding[]", Some(&value));
                        }
                    }
                }
                FilterValue::Text { id, value } if !value.trim().is_empty() => {
                    let key = match id.as_str() {
                        "year_range" => "year_range",
                        "chapter_range" => "chapter_range",
                        _ => continue,
                    };
                    params.push(key, Some(value.trim()));
                }
                _ => {}
            }
        }
        params.push("sortby", Some(sort));
        params.push("order", Some(if ascending { "asc" } else { "desc" }));
        let base = Self::base_url();
        if page > 1 {
            format!("{base}/page/{page}/?{params}")
        } else {
            format!("{base}/?{params}")
        }
    }

    fn browse(index: i32, page: i32) -> Result<MangaPageResult> {
        let url = Self::search_url(
            None,
            page,
            vec![FilterValue::Sort {
                id: "sort".into(),
                index,
                ascending: false,
            }],
        );
        Ok(Self::parse_list(&Self::document(&url)?))
    }

    fn text_list(document: &Document, selector: &str) -> Option<Vec<String>> {
        let values = document
            .select(selector)?
            .filter_map(|element| element.text())
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>();
        (!values.is_empty()).then_some(values)
    }

    fn creator_names(document: &Document, role: &str) -> Option<Vec<String>> {
        // Match taxonomy links directly. Labels, sibling wrappers and the first
        // child of a creator link vary between the site's detail layouts.
        let selector = match role {
            "author" => {
                "#mangaSummary a[href*='/author/'], #mangaSummary a[href*='/manga-author/'], #mangaSummary a[href*='/manga_author/'], #mangaSummary .text-primary:contains(Author) + .flex a"
            }
            "artist" => {
                "#mangaSummary a[href*='/artist/'], #mangaSummary a[href*='/manga-artist/'], #mangaSummary a[href*='/manga_artist/'], #mangaSummary .text-primary:contains(Artist) + .flex a"
            }
            _ => return None,
        };
        let mut names = Vec::new();
        for link in document.select(selector)? {
            // Read the name span separately so entry-count badges aren't names.
            // Allow an icon or an empty span before the name, and plain-text links.
            let name = link
                .select("span")
                .and_then(|spans| {
                    spans
                        .filter_map(|span| span.text())
                        .find(|text| !text.trim().is_empty())
                })
                .or_else(|| link.text())
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty());
            if let Some(name) = name {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        (!names.is_empty()).then_some(names)
    }

    fn chapters(document: &Document) -> Vec<Chapter> {
        let mut chapters = document
            .select("#chaptersList > a.chapter-item")
            .map(|elements| {
                elements
                    .filter_map(|element| {
                        let href = element.attr("href")?;
                        let key = parser::path(&href)?.to_owned();
                        let title = element
                            .select_first("span.chapter-item__name")?
                            .text()?
                            .trim()
                            .to_owned();
                        let date_uploaded = element
                            .select_first("span.chapter-item__date")
                            .and_then(|date| date.text())
                            .and_then(|date| {
                                parse_date_with_options(
                                    date.trim(),
                                    "d/M/yyyy",
                                    "en_US_POSIX",
                                    "UTC",
                                )
                            });
                        Some(Chapter {
                            url: Some(Self::url(&key)),
                            key,
                            chapter_number: parser::chapter_number(&title),
                            title: Some(title),
                            date_uploaded,
                            language: Some("en".into()),
                            ..Default::default()
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        // The website lists oldest first; Aidoku expects newest first.
        chapters.reverse();
        chapters
    }
}

impl Source for ManhwaRead {
    fn new() -> Self {
        Self
    }

    fn get_search_manga_list(
        &self,
        query: Option<String>,
        page: i32,
        filters: Vec<FilterValue>,
    ) -> Result<MangaPageResult> {
        if let Some(key) = query.as_deref().and_then(parser::manga_key) {
            let manga = self.get_manga_update(
                Manga {
                    key,
                    ..Default::default()
                },
                true,
                false,
            )?;
            return Ok(MangaPageResult {
                entries: vec![manga],
                has_next_page: false,
            });
        }
        let url = Self::search_url(query.as_deref(), page, filters);
        Ok(Self::parse_list(&Self::document(&url)?))
    }

    fn get_manga_update(
        &self,
        mut manga: Manga,
        needs_details: bool,
        needs_chapters: bool,
    ) -> Result<Manga> {
        if !needs_details && !needs_chapters {
            return Ok(manga);
        }
        let url = Self::url(&manga.key);
        let document = Self::document(&url)?;
        if document
            .select_first("#mangaSummary .manga-titles h1")
            .is_none()
        {
            bail!(
                "ManhwaRead title not found. The website may be unavailable or its layout has changed."
            );
        }
        if needs_details {
            manga.title = document
                .select_first("#mangaSummary .manga-titles h1")
                .and_then(|element| element.text())
                .unwrap_or(manga.title);
            manga.cover = document
                .select_first("head meta[property='og:image']")
                .and_then(|element| element.attr("content"))
                .map(|value| parser::absolute_url(Self::base_url(), &value))
                .or(manga.cover);
            let mut description = document
                .select_first("#mangaDesc > .manga-desc__content")
                .and_then(|element| element.text())
                .unwrap_or_default();
            if let Some(alternatives) = document
                .select_first("#mangaSummary .manga-titles h2")
                .and_then(|element| element.text())
                .filter(|text| !text.trim().is_empty())
            {
                if !description.is_empty() {
                    description.push_str("\n\n");
                }
                description.push_str("Alternative titles:\n");
                description.push_str(
                    &alternatives
                        .split('|')
                        .map(str::trim)
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
            manga.description = (!description.is_empty()).then_some(description);
            manga.tags = Self::text_list(
                &document,
                "#mangaSummary .manga-genres a, #mangaSummary .text-primary:contains(Tags:) + .flex a span:first-child",
            );
            manga.status = match document
                .select_first("#mangaSummary .manga-status")
                .and_then(|element| element.attr("data-status"))
                .as_deref()
            {
                Some("ongoing") => MangaStatus::Ongoing,
                Some("completed") => MangaStatus::Completed,
                Some("canceled") => MangaStatus::Cancelled,
                Some("on-hold") => MangaStatus::Hiatus,
                _ => MangaStatus::Unknown,
            };
            manga.update_strategy = UpdateStrategy::Always;
            manga.viewer = Viewer::Webtoon;
            manga.url = Some(url);
        }
        // A chapter refresh also loads this detail page, so repair missing
        // creator metadata without requiring the title to be removed/re-added.
        manga.authors = Self::creator_names(&document, "author").or(manga.authors);
        manga.artists = Self::creator_names(&document, "artist").or(manga.artists);
        if needs_chapters {
            let chapters = Self::chapters(&document);
            // Keep stored chapters intact if a broken/challenged page omits the list.
            if chapters.is_empty() {
                bail!("ManhwaRead returned no chapters. Open the website and retry.");
            }
            manga.chapters = Some(chapters);
        }
        Ok(manga)
    }

    fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
        let url = Self::url(&chapter.key);
        let document = Self::document(&url)?;
        let scripts = document
            .select("script")
            .map(|elements| {
                elements
                    .filter_map(|element| element.data().or_else(|| element.html()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let script = scripts
            .iter()
            .find(|script| parser::chapter_data(script).is_some())
            .ok_or_else(|| aidoku::AidokuError::message("ManhwaRead chapter data not found"))?;
        let images = parser::image_urls(script).map_err(aidoku::AidokuError::message)?;
        Ok(images
            .into_iter()
            .map(|image| {
                let mut context = PageContext::new();
                context.insert("referer".into(), url.clone());
                Page {
                    content: PageContent::url_context(image, context),
                    ..Default::default()
                }
            })
            .collect())
    }
}

impl ListingProvider for ManhwaRead {
    fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
        match listing.id.as_str() {
            "popular" => Self::browse(8, page),
            "latest" | "recent" => Self::browse(0, page),
            _ => {
                bail!("Unknown listing");
            }
        }
    }
}

impl Home for ManhwaRead {
    fn get_home(&self) -> Result<HomeLayout> {
        Ok(midoku_madara::home_layout(
            Self::browse(8, 1)?.entries,
            Self::browse(0, 1)?.entries,
        ))
    }
}

impl ImageRequestProvider for ManhwaRead {
    fn get_image_request(&self, url: String, context: Option<PageContext>) -> Result<Request> {
        let referer = context
            .as_ref()
            .and_then(|context| context.get("referer"))
            .map(String::as_str)
            .unwrap_or(Self::base_url());
        Ok(Request::get(url)?.header("Referer", referer).header(
            "Accept",
            "image/avif,image/webp,image/apng,image/*,*/*;q=0.8",
        ))
    }
}

impl DeepLinkHandler for ManhwaRead {
    fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
        Ok(parser::manga_key(&url).map(|key| DeepLinkResult::Manga { key }))
    }
}

register_source!(
    ManhwaRead,
    ListingProvider,
    Home,
    ImageRequestProvider,
    DeepLinkHandler
);
