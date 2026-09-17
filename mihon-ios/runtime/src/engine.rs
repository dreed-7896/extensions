use crate::http;
use dexvm::keiyoushi::{Chapter, Keiyoushi, Manga, PageRef, Source};
use serde::Serialize;

#[derive(Serialize)]
pub struct SourceInfo {
    pub index: usize,
    pub id: i64,
    pub name: String,
    pub lang: String,
    pub supports_latest: bool,
}

#[derive(Serialize)]
pub struct MangaOut {
    pub title: String,
    pub url: String,
    pub thumbnail_url: String,
    pub author: String,
    pub artist: String,
    pub description: String,
    pub genre: String,
    pub status: i32,
}

#[derive(Serialize)]
pub struct ChapterOut {
    pub name: String,
    pub url: String,
    pub date_upload: i64,
    pub scanlator: String,
}

#[derive(Serialize)]
pub struct PageOut {
    pub index: i32,
    pub url: String,
    pub image_url: String,
}

pub struct Engine {
    ext: Keiyoushi,
    sources: Vec<Source>,
}

impl Engine {
    pub fn open_file(path: &str) -> Result<Self, String> {
        let mut ext = Keiyoushi::open(path).map_err(|e| e.to_string())?;
        crate::extra_shims::install(ext.ctx().vm())?;
        ext.set_http(http::execute);
        let sources = ext.sources().map_err(|e| ext_err(&mut ext, &e))?;
        if sources.is_empty() {
            return Err("apk exposed no sources".into());
        }
        Ok(Self { ext, sources })
    }

    pub fn sources(&mut self) -> Result<Vec<SourceInfo>, String> {
        let mut out = Vec::new();
        for (index, src) in self.sources.clone().iter().enumerate() {
            out.push(SourceInfo {
                index,
                id: self.ext.source_id(src).unwrap_or(0),
                name: self.ext.source_name(src).unwrap_or_else(|_| "?".into()),
                lang: self.ext.source_lang(src).unwrap_or_else(|_| "?".into()),
                supports_latest: self.ext.supports_latest(src).unwrap_or(false),
            });
        }
        out.sort_by(|a, b| {
            let rank = |lang: &str| match lang {
                "all" => 0,
                "en" => 1,
                "ja" => 2,
                _ => 3,
            };
            rank(&a.lang)
                .cmp(&rank(&b.lang))
                .then_with(|| a.lang.cmp(&b.lang))
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(out)
    }

    pub fn popular(&mut self, index: usize, page: i32) -> Result<(Vec<MangaOut>, bool), String> {
        let src = self.src(index)?;
        let pages = fallback(
            &mut self.ext,
            |ext| ext.popular_coro(&src, page),
            |ext| ext.popular(&src, page),
        )?;
        Ok((
            pages.mangas.into_iter().map(manga_out).collect(),
            pages.has_next,
        ))
    }

    pub fn latest(&mut self, index: usize, page: i32) -> Result<(Vec<MangaOut>, bool), String> {
        let src = self.src(index)?;
        let pages = fallback(
            &mut self.ext,
            |ext| ext.latest_coro(&src, page),
            |ext| ext.latest(&src, page),
        )?;
        Ok((
            pages.mangas.into_iter().map(manga_out).collect(),
            pages.has_next,
        ))
    }

    pub fn search(
        &mut self,
        index: usize,
        page: i32,
        query: &str,
    ) -> Result<(Vec<MangaOut>, bool), String> {
        let src = self.src(index)?;
        let pages = fallback(
            &mut self.ext,
            |ext| ext.search_coro(&src, page, query, &[]),
            |ext| ext.search(&src, page, query, &[]),
        )?;
        Ok((
            pages.mangas.into_iter().map(manga_out).collect(),
            pages.has_next,
        ))
    }

    pub fn details(&mut self, index: usize, url: &str, title: &str) -> Result<MangaOut, String> {
        let src = self.src(index)?;
        let manga = Manga {
            url: url.to_string(),
            title: title.to_string(),
            ..Manga::default()
        };
        let detailed = match self.ext.manga_details(&src, &manga) {
            Ok(m) => m,
            Err(e) => {
                let msg = ext_err(&mut self.ext, &e);
                self.ext
                    .manga_update_details(&src, &manga)
                    .map_err(|e2| format!("{msg} / {}", ext_err(&mut self.ext, &e2)))?
            }
        };
        Ok(manga_out(detailed))
    }

    pub fn chapters(
        &mut self,
        index: usize,
        url: &str,
        title: &str,
    ) -> Result<Vec<ChapterOut>, String> {
        let src = self.src(index)?;
        let manga = Manga {
            url: url.to_string(),
            title: title.to_string(),
            ..Manga::default()
        };
        let list = match self.ext.chapters(&src, &manga) {
            Ok(c) => c,
            Err(e) => {
                let msg = ext_err(&mut self.ext, &e);
                self.ext
                    .manga_update_chapters(&src, &manga)
                    .map_err(|e2| format!("{msg} / {}", ext_err(&mut self.ext, &e2)))?
            }
        };
        Ok(list.into_iter().map(chapter_out).collect())
    }

    pub fn pages(&mut self, index: usize, url: &str, name: &str) -> Result<Vec<PageOut>, String> {
        let src = self.src(index)?;
        let chapter = Chapter {
            url: url.to_string(),
            name: name.to_string(),
            ..Chapter::default()
        };
        let list = fallback(
            &mut self.ext,
            |ext| ext.pages_coro(&src, &chapter),
            |ext| ext.pages(&src, &chapter),
        )?;
        Ok(list.into_iter().map(page_out).collect())
    }

    fn src(&self, index: usize) -> Result<Source, String> {
        self.sources
            .get(index)
            .copied()
            .ok_or_else(|| format!("source index {index} out of range"))
    }
}

fn fallback<T>(
    ext: &mut Keiyoushi,
    primary: impl FnOnce(&mut Keiyoushi) -> Result<T, dexvm::vm::error::JvmError>,
    secondary: impl FnOnce(&mut Keiyoushi) -> Result<T, dexvm::vm::error::JvmError>,
) -> Result<T, String> {
    match primary(ext) {
        Ok(v) => Ok(v),
        Err(e) => {
            let msg = ext.describe_error(&e);
            secondary(ext).map_err(|e2| format!("{msg} / {}", ext.describe_error(&e2)))
        }
    }
}

fn manga_out(m: Manga) -> MangaOut {
    MangaOut {
        title: m.title,
        url: m.url,
        thumbnail_url: m.thumbnail_url,
        author: m.author,
        artist: m.artist,
        description: m.description,
        genre: m.genre,
        status: m.status,
    }
}

fn chapter_out(c: Chapter) -> ChapterOut {
    ChapterOut {
        name: c.name,
        url: c.url,
        date_upload: c.date_upload,
        scanlator: c.scanlator,
    }
}

fn page_out(p: PageRef) -> PageOut {
    PageOut {
        index: p.index,
        url: p.url,
        image_url: p.image_url,
    }
}

fn ext_err(ext: &mut Keiyoushi, e: &dexvm::vm::error::JvmError) -> String {
    ext.describe_error(e)
}
