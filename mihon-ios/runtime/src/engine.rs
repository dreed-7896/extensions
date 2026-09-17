use crate::http;
use dexvm::keiyoushi::{Chapter, Keiyoushi, Manga, PageRef, Source};
use dexvm::vm::error::JvmError;
use dexvm::vm::object::Native;
use dexvm::vm::value::JValue;
use serde::Serialize;

const SMANGA: &str = "Leu/kanade/tachiyomi/source/model/SManga;";
const CONT: &str = "Lkotlin/coroutines/jvm/internal/ContinuationImpl;";

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
        let sources = ext.sources().map_err(|e| ext.describe_error(&e))?;
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
        let detailed = try3(
            &mut self.ext,
            |ext| manga_update_details_host(ext, &src, &manga),
            |ext| details_coro(ext, &src, &manga),
            |ext| ext.manga_details(&src, &manga),
        )?;
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
        // 1.6 (MangaDex) implements getMangaUpdate, not chapterListParse.
        let list = try3(
            &mut self.ext,
            |ext| manga_update_chapters_host(ext, &src, &manga),
            |ext| chapters_coro(ext, &src, &manga),
            |ext| ext.chapters(&src, &manga),
        )?;
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
    primary: impl FnOnce(&mut Keiyoushi) -> Result<T, JvmError>,
    secondary: impl FnOnce(&mut Keiyoushi) -> Result<T, JvmError>,
) -> Result<T, String> {
    match primary(ext) {
        Ok(v) => Ok(v),
        Err(e) => {
            let msg = ext.describe_error(&e);
            secondary(ext).map_err(|e2| format!("{msg} / {}", ext.describe_error(&e2)))
        }
    }
}

fn try3<T>(
    ext: &mut Keiyoushi,
    a: impl FnOnce(&mut Keiyoushi) -> Result<T, JvmError>,
    b: impl FnOnce(&mut Keiyoushi) -> Result<T, JvmError>,
    c: impl FnOnce(&mut Keiyoushi) -> Result<T, JvmError>,
) -> Result<T, String> {
    match a(ext) {
        Ok(v) => Ok(v),
        Err(e1) => {
            let m1 = ext.describe_error(&e1);
            match b(ext) {
                Ok(v) => Ok(v),
                Err(e2) => {
                    let m2 = ext.describe_error(&e2);
                    c(ext).map_err(|e3| format!("{m1} / {m2} / {}", ext.describe_error(&e3)))
                }
            }
        }
    }
}

fn manga_update(
    ext: &mut Keiyoushi,
    src: &Source,
    manga: &Manga,
    fetch_details: bool,
    fetch_chapters: bool,
) -> Result<JValue, JvmError> {
    let m = alloc_manga(ext, manga)?;
    let empty_list = {
        let vm = ext.ctx().vm();
        let cid = vm.ensure_class_by_desc("Ljava/util/ArrayList;")?;
        JValue::Obj(
            vm.arena
                .alloc(cid, Vec::new(), Some(Native::List(Vec::new()))),
        )
    };
    let cont = suspend_cont(ext)?;
    ext.ctx().invoke_on(
        src.inst(),
        "getMangaUpdate",
        "(Leu/kanade/tachiyomi/source/model/SManga;Ljava/util/List;ZZLkotlin/coroutines/Continuation;)Ljava/lang/Object;",
        &[
            m,
            empty_list,
            JValue::Int(i32::from(fetch_details)),
            JValue::Int(i32::from(fetch_chapters)),
            cont,
        ],
    )
}

fn manga_update_chapters_host(
    ext: &mut Keiyoushi,
    src: &Source,
    manga: &Manga,
) -> Result<Vec<Chapter>, JvmError> {
    let out = manga_update(ext, src, manga, false, true)?;
    read_chapters(ext, out)
}

fn manga_update_details_host(
    ext: &mut Keiyoushi,
    src: &Source,
    manga: &Manga,
) -> Result<Manga, JvmError> {
    let out = manga_update(ext, src, manga, true, false)?;
    read_manga_value(ext, out)?.ok_or_else(|| {
        JvmError::Resolution(format!(
            "getMangaUpdate: not a SManga ({})",
            payload_kind(ext, out)
        ))
    })
}

/// 1.6 sources override suspend `getChapterList` and stub `chapterListParse`.
fn chapters_coro(
    ext: &mut Keiyoushi,
    src: &Source,
    manga: &Manga,
) -> Result<Vec<Chapter>, JvmError> {
    let m = alloc_manga(ext, manga)?;
    let cont = suspend_cont(ext)?;
    let out = ext.ctx().invoke_on(
        src.inst(),
        "getChapterList",
        "(Leu/kanade/tachiyomi/source/model/SManga;Lkotlin/coroutines/Continuation;)Ljava/lang/Object;",
        &[m, cont],
    )?;
    read_chapters(ext, out)
}

/// 1.6 sources override suspend `getMangaDetails`.
fn details_coro(ext: &mut Keiyoushi, src: &Source, manga: &Manga) -> Result<Manga, JvmError> {
    let m = alloc_manga(ext, manga)?;
    let cont = suspend_cont(ext)?;
    let out = ext.ctx().invoke_on(
        src.inst(),
        "getMangaDetails",
        "(Leu/kanade/tachiyomi/source/model/SManga;Lkotlin/coroutines/Continuation;)Ljava/lang/Object;",
        &[m, cont],
    )?;
    read_manga_value(ext, out)?.ok_or_else(|| {
        JvmError::Resolution(format!(
            "getMangaDetails: not a SManga ({})",
            payload_kind(ext, out)
        ))
    })
}

fn alloc_manga(ext: &mut Keiyoushi, m: &Manga) -> Result<JValue, JvmError> {
    let vm = ext.ctx().vm();
    let cid = vm.ensure_class_by_desc(SMANGA)?;
    let payload = Native::SManga {
        title: m.title.clone(),
        author: Some(m.author.clone()).filter(|s| !s.is_empty()),
        artist: Some(m.artist.clone()).filter(|s| !s.is_empty()),
        description: Some(m.description.clone()).filter(|s| !s.is_empty()),
        genre: Some(m.genre.clone()).filter(|s| !s.is_empty()),
        status: m.status,
        thumbnail_url: m.thumbnail_url.clone(),
        url: m.url.clone(),
        update_strategy: JValue::Null,
        memo: JValue::Null,
    };
    Ok(JValue::Obj(vm.arena.alloc(cid, Vec::new(), Some(payload))))
}

fn suspend_cont(ext: &mut Keiyoushi) -> Result<JValue, JvmError> {
    let vm = ext.ctx().vm();
    let cid = vm.ensure_class_by_desc(CONT)?;
    Ok(JValue::Obj(vm.alloc_instance(cid)?))
}

fn read_chapters(ext: &mut Keiyoushi, v: JValue) -> Result<Vec<Chapter>, JvmError> {
    let items = list_items(ext, v)?;
    let mut out = Vec::with_capacity(items.len());
    for c in items {
        if let Some(Native::SChapter {
            name,
            url,
            date_upload,
            scanlator,
            ..
        }) = ext.ctx().vm().payload_of(c)
        {
            out.push(Chapter {
                name,
                url,
                date_upload,
                scanlator,
            });
        }
    }
    Ok(out)
}

fn list_items(ext: &mut Keiyoushi, v: JValue) -> Result<Vec<JValue>, JvmError> {
    let vm = ext.ctx().vm();
    match collect_list(vm, v) {
        Some(items) => Ok(items),
        None => match vm.payload_of(v) {
            Some(Native::SMangaUpdate { chapters, .. }) => {
                collect_list(vm, chapters).ok_or_else(|| {
                    JvmError::Resolution(format!(
                        "getMangaUpdate.chapters not a List ({})",
                        payload_kind_vm(vm, chapters)
                    ))
                })
            }
            _ => Err(JvmError::Resolution(format!(
                "chapter list not a List ({})",
                payload_kind_vm(vm, v)
            ))),
        },
    }
}

fn collect_list(vm: &dexvm::Vm, v: JValue) -> Option<Vec<JValue>> {
    match vm.payload_of(v) {
        Some(Native::List(items) | Native::Set(items) | Native::ArrayDeque(items)) => Some(items),
        Some(Native::Array(data)) => Some((0..data.len()).map(|i| data.get(i)).collect()),
        _ => None,
    }
}

fn payload_kind(ext: &mut Keiyoushi, v: JValue) -> String {
    payload_kind_vm(ext.ctx().vm(), v)
}

fn payload_kind_vm(vm: &dexvm::Vm, v: JValue) -> String {
    let class = class_of(vm, v);
    match vm.payload_of(v) {
        None if v.is_null() => "null".into(),
        None => format!("no-native {class}"),
        Some(Native::List(items)) => format!("List(len={}, {class})", items.len()),
        Some(Native::Set(items)) => format!("Set(len={}, {class})", items.len()),
        Some(Native::ArrayDeque(items)) => format!("ArrayDeque(len={}, {class})", items.len()),
        Some(Native::Array(data)) => format!("Array(len={}, {class})", data.len()),
        Some(Native::SMangaUpdate { manga, chapters }) => format!(
            "SMangaUpdate({class}, manga={}, chapters={})",
            payload_kind_vm(vm, manga),
            payload_kind_vm(vm, chapters)
        ),
        Some(Native::SManga { .. }) => format!("SManga {class}"),
        Some(Native::SChapter { .. }) => format!("SChapter {class}"),
        Some(Native::Map(_)) => format!("Map {class}"),
        Some(Native::Str(s)) => format!("Str({s:?}) {class}"),
        Some(Native::Opaque) => format!("Opaque {class}"),
        Some(_) => format!("other {class}"),
    }
}

fn read_manga_value(ext: &mut Keiyoushi, v: JValue) -> Result<Option<Manga>, JvmError> {
    let vm = ext.ctx().vm();
    match vm.payload_of(v) {
        Some(Native::SManga {
            title,
            author,
            artist,
            description,
            genre,
            status,
            thumbnail_url,
            url,
            ..
        }) => Ok(Some(Manga {
            title,
            author: author.unwrap_or_default(),
            artist: artist.unwrap_or_default(),
            description: description.unwrap_or_default(),
            genre: genre.unwrap_or_default(),
            status,
            thumbnail_url,
            url,
        })),
        Some(Native::SMangaUpdate { manga, .. }) => read_manga_value(ext, manga),
        _ => Ok(None),
    }
}

fn class_of(vm: &dexvm::Vm, v: JValue) -> String {
    if let JValue::Obj(id) = v {
        if (id as usize) < vm.arena.objects.len() {
            return vm.class_desc_str(vm.arena.objects[id as usize].class);
        }
    }
    format!("{v:?}")
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
