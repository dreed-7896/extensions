use crate::host;
use crate::http;
use dexvm::keiyoushi::{Chapter, Keiyoushi, Manga, PageRef, Source};
use dexvm::vm::error::JvmError;
use dexvm::vm::object::Native;
use dexvm::vm::value::JValue;
use serde::Serialize;

const SMANGA: &str = "Leu/kanade/tachiyomi/source/model/SManga;";
const CONT: &str = "Lkotlin/coroutines/jvm/internal/ContinuationImpl;";
const PAGE: &str = "Leu/kanade/tachiyomi/source/model/Page;";

#[derive(Serialize)]
pub struct SourceInfo {
    pub index: usize,
    pub id: i64,
    pub name: String,
    pub lang: String,
    pub supports_latest: bool,
    pub base_url: String,
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
        ext.set_host_headers(|_| (Some(http::USER_AGENT.to_string()), None));
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
                base_url: self.base_url(src),
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
        let base = self.base_url(&src);
        let pages = mihon_get(
            &mut self.ext,
            |ext| ext.popular_coro(&src, page),
            |ext| ext.popular(&src, page),
        )?;
        Ok((
            pages
                .mangas
                .into_iter()
                .map(|m| manga_out(m, &base))
                .collect(),
            pages.has_next,
        ))
    }

    pub fn latest(&mut self, index: usize, page: i32) -> Result<(Vec<MangaOut>, bool), String> {
        let src = self.src(index)?;
        let base = self.base_url(&src);
        let pages = mihon_get(
            &mut self.ext,
            |ext| ext.latest_coro(&src, page),
            |ext| ext.latest(&src, page),
        )?;
        Ok((
            pages
                .mangas
                .into_iter()
                .map(|m| manga_out(m, &base))
                .collect(),
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
        let base = self.base_url(&src);
        let pages = mihon_get(
            &mut self.ext,
            |ext| ext.search_coro(&src, page, query, &[]),
            |ext| ext.search(&src, page, query, &[]),
        )?;
        Ok((
            pages
                .mangas
                .into_iter()
                .map(|m| manga_out(m, &base))
                .collect(),
            pages.has_next,
        ))
    }

    pub fn details(&mut self, index: usize, url: &str, title: &str) -> Result<MangaOut, String> {
        let src = self.src(index)?;
        let base = self.base_url(&src);
        let manga = Manga {
            url: url.to_string(),
            title: title.to_string(),
            ..Manga::default()
        };
        let detailed = mihon_get3(
            &mut self.ext,
            |ext| details_coro(ext, &src, &manga),
            |ext| ext.manga_update_details(&src, &manga),
            |ext| ext.manga_details(&src, &manga),
        )?;
        Ok(manga_out(detailed, &base))
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
        let list = mihon_get3(
            &mut self.ext,
            |ext| chapters_coro(ext, &src, &manga),
            |ext| manga_update_chapters(ext, &src, &manga),
            |ext| ext.chapters(&src, &manga),
        )?;
        Ok(list.into_iter().map(chapter_out).collect())
    }

    pub fn pages(&mut self, index: usize, url: &str, name: &str) -> Result<Vec<PageOut>, String> {
        let src = self.src(index)?;
        let base = self.base_url(&src);
        let chapter = Chapter {
            url: url.to_string(),
            name: name.to_string(),
            ..Chapter::default()
        };
        let mut list = mihon_get(
            &mut self.ext,
            |ext| ext.pages_coro(&src, &chapter),
            |ext| ext.pages(&src, &chapter),
        )?;
        fill_image_urls(&mut self.ext, &src, &mut list);
        Ok(list
            .into_iter()
            .map(|p| page_out(p, &base))
            .collect())
    }

    /// Fetch image bytes through the extension's OkHttpClient (referer,
    /// decrypt interceptors, cookies) — same path Mihon/TachiManga use.
    ///
    /// `page_url` is `Page.url` from `pages()`. Covers pass only `url`
    /// (thumbnail); that must NOT go through `imageRequest`, because sources
    /// like MangaDex treat `page.url` as an MD@Home token.
    pub fn image(
        &mut self,
        index: usize,
        url: &str,
        page_url: Option<&str>,
    ) -> Result<Vec<u8>, String> {
        let src = self.src(index)?;
        let base = self.base_url(&src);
        let img = url.trim();
        let token = page_url.map(str::trim).unwrap_or("");
        if img.is_empty() && token.is_empty() {
            return Err("empty image url".into());
        }
        if is_page_token(token) {
            let relative = if img.is_empty() { token } else { img };
            return run_healed(&mut self.ext, |ext| {
                image_via_request(ext, &src, token, relative)
            });
        }
        let image = absolutize(&base, if img.is_empty() { token } else { img });
        if image.is_empty() {
            return Err("empty image url".into());
        }
        let use_page = !token.is_empty() && token != img && token != image;
        if use_page {
            match run_healed(&mut self.ext, |ext| {
                image_via_request(ext, &src, token, &image)
            }) {
                Ok(bytes) if !bytes.is_empty() => return Ok(bytes),
                Err(msg) if fallback_ok(&msg) => {}
                Ok(_) => {}
                Err(msg) => return Err(msg),
            }
        }
        match run_healed(&mut self.ext, |ext| image_via_client(ext, &src, &image)) {
            Ok(bytes) if !bytes.is_empty() => Ok(bytes),
            Err(msg) if fallback_ok(&msg) => {
                run_healed(&mut self.ext, |ext| ext.image_data(&src, &image))
                    .map_err(|m2| format!("{msg} / {m2}"))
            }
            Ok(_) => run_healed(&mut self.ext, |ext| ext.image_data(&src, &image)),
            Err(msg) => Err(msg),
        }
    }

    fn base_url(&mut self, src: &Source) -> String {
        match self
            .ext
            .ctx()
            .invoke_on(src.inst(), "getBaseUrl", "()Ljava/lang/String;", &[])
        {
            Ok(v) => match self.ext.ctx().vm().payload_of(v) {
                Some(Native::Str(s)) => s,
                _ => String::new(),
            },
            Err(_) => String::new(),
        }
    }

    fn src(&self, index: usize) -> Result<Source, String> {
        self.sources
            .get(index)
            .copied()
            .ok_or_else(|| format!("source index {index} out of range"))
    }
}

fn run_healed<T>(
    ext: &mut Keiyoushi,
    mut f: impl FnMut(&mut Keiyoushi) -> Result<T, JvmError>,
) -> Result<T, String> {
    let mut last = String::new();
    for _ in 0..32 {
        match f(ext) {
            Ok(v) => return Ok(v),
            Err(e) => {
                let msg = ext.describe_error(&e);
                if host::heal(ext.ctx().vm(), &msg) {
                    last = msg;
                    continue;
                }
                return Err(msg);
            }
        }
    }
    Err(format!("host heal loop: {last}"))
}

/// Mihon-style: suspend/coro first (HttpSource default = request/parse),
/// then classic fetch*/request/parse. Only fall back when the entry point
/// is missing or deliberately stubbed — not on parse/HTTP failures.
fn mihon_get<T>(
    ext: &mut Keiyoushi,
    primary: impl FnMut(&mut Keiyoushi) -> Result<T, JvmError>,
    secondary: impl FnMut(&mut Keiyoushi) -> Result<T, JvmError>,
) -> Result<T, String> {
    match run_healed(ext, primary) {
        Ok(v) => Ok(v),
        Err(msg) if fallback_ok(&msg) => run_healed(ext, secondary).map_err(|m2| format!("{msg} / {m2}")),
        Err(msg) => Err(msg),
    }
}

fn mihon_get3<T>(
    ext: &mut Keiyoushi,
    a: impl FnMut(&mut Keiyoushi) -> Result<T, JvmError>,
    b: impl FnMut(&mut Keiyoushi) -> Result<T, JvmError>,
    c: impl FnMut(&mut Keiyoushi) -> Result<T, JvmError>,
) -> Result<T, String> {
    match mihon_get(ext, a, b) {
        Ok(v) => Ok(v),
        Err(msg) if fallback_ok(&msg) => {
            run_healed(ext, c).map_err(|m3| format!("{msg} / {m3}"))
        }
        Err(msg) => Err(msg),
    }
}

fn fallback_ok(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    // 1.4 sources stub unused suspend/request hooks with UOE. That is the
    // signal to try fetch*/classic parse — not a parse/HTTP failure.
    if m.contains("unsupportedoperation") {
        return true;
    }
    if m.contains("uncaught")
        || m.contains("nullpointer")
        || m.contains("http ")
        || m.contains("cloudflare")
        || m.contains("initreader")
    {
        return false;
    }
    m.contains("resolution error")
        || m.contains("no method")
        || m.contains("class not found")
}

/// Mihon `HttpSource.imageRequest(page)` then the source OkHttp client.
fn image_via_request(
    ext: &mut Keiyoushi,
    src: &Source,
    page_url: &str,
    image_url: &str,
) -> Result<Vec<u8>, JvmError> {
    let page = {
        let vm = ext.ctx().vm();
        let cid = vm.ensure_class_by_desc(PAGE)?;
        JValue::Obj(vm.arena.alloc(
            cid,
            Vec::new(),
            Some(Native::SPPage {
                index: 0,
                name: String::new(),
                url: page_url.to_string(),
                image_url: image_url.to_string(),
            }),
        ))
    };
    let req = ext.ctx().invoke_on(
        src.inst(),
        "imageRequest",
        "(Leu/kanade/tachiyomi/source/model/Page;)Lokhttp3/Request;",
        &[page],
    )?;
    execute_call(ext, src, req)
}

/// Cover / standalone URL: `GET(url, headers)` on the source client.
/// Never `imageRequest` — several sources overload that for page tokens.
fn image_via_client(ext: &mut Keiyoushi, src: &Source, url: &str) -> Result<Vec<u8>, JvmError> {
    let headers = ext.ctx().invoke_on(
        src.inst(),
        "getHeaders",
        "()Lokhttp3/Headers;",
        &[],
    )?;
    let pairs = match ext.ctx().vm().payload_of(headers) {
        Some(Native::Headers(h)) => h,
        _ => Vec::new(),
    };
    let req = {
        let vm = ext.ctx().vm();
        let cid = vm.ensure_class_by_desc("Lokhttp3/Request;")?;
        JValue::Obj(vm.arena.alloc(
            cid,
            Vec::new(),
            Some(Native::Request {
                url: url.to_string(),
                method: "GET".into(),
                headers: pairs,
                body: None,
            }),
        ))
    };
    execute_call(ext, src, req)
}

fn execute_call(ext: &mut Keiyoushi, src: &Source, req: JValue) -> Result<Vec<u8>, JvmError> {
    let client = ext
        .ctx()
        .invoke_on(src.inst(), "getClient", "()Lokhttp3/OkHttpClient;", &[])?;
    let call = ext.ctx().invoke_on(
        client.as_obj(),
        "newCall",
        "(Lokhttp3/Request;)Lokhttp3/Call;",
        &[req],
    )?;
    let resp = ext
        .ctx()
        .invoke_on(call.as_obj(), "execute", "()Lokhttp3/Response;", &[])?;
    match ext.ctx().vm().payload_of(resp) {
        Some(Native::Response {
            body: Some(b),
            code,
            ..
        }) if (200..300).contains(&code) => Ok(b),
        Some(Native::Response { code, .. }) => Err(JvmError::Resolution(format!("HTTP {code}"))),
        _ => Err(JvmError::Resolution(
            "image request did not yield a Response".into(),
        )),
    }
}

fn manga_update_chapters(
    ext: &mut Keiyoushi,
    src: &Source,
    manga: &Manga,
) -> Result<Vec<Chapter>, JvmError> {
    let out = ext.manga_update_coro(src, manga, false, true)?;
    read_chapters(ext, out)
}

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

fn fill_image_urls(ext: &mut Keiyoushi, src: &Source, pages: &mut [PageRef]) {
    for page in pages.iter_mut() {
        // Mihon HttpPageLoader: getImageUrl only when imageUrl is empty.
        if !page.image_url.is_empty() {
            continue;
        }
        match resolve_image_url(ext, src, page) {
            Ok(url) if !url.is_empty() => page.image_url = url,
            _ => {
                if page.image_url.is_empty() {
                    page.image_url = page.url.clone();
                }
            }
        }
    }
}

fn resolve_image_url(
    ext: &mut Keiyoushi,
    src: &Source,
    page: &PageRef,
) -> Result<String, JvmError> {
    let obj = {
        let vm = ext.ctx().vm();
        let cid = vm.ensure_class_by_desc(PAGE)?;
        JValue::Obj(vm.arena.alloc(
            cid,
            Vec::new(),
            Some(Native::SPPage {
                index: page.index,
                name: page.name.clone(),
                url: page.url.clone(),
                image_url: page.image_url.clone(),
            }),
        ))
    };
    let cont = suspend_cont(ext)?;
    let out = ext.ctx().invoke_on(
        src.inst(),
        "getImageUrl",
        "(Leu/kanade/tachiyomi/source/model/Page;Lkotlin/coroutines/Continuation;)Ljava/lang/Object;",
        &[obj, cont],
    )?;
    match ext.ctx().vm().payload_of(out) {
        Some(Native::Str(s)) => Ok(s),
        _ => Ok(String::new()),
    }
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

fn is_absolute_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("data:")
}

/// Page.url used as a token bag (MD@Home: `host,tokenUrl,ts`) rather than a
/// fetchable image URL. Keep image_url relative so imageRequest can join it.
fn is_page_token(url: &str) -> bool {
    let mut parts = url.split(',');
    let host = parts.next().unwrap_or("");
    parts.next().is_some() && (host.starts_with("http://") || host.starts_with("https://"))
}

fn absolutize(base: &str, url: &str) -> String {
    let url = url.trim();
    if url.is_empty() {
        return String::new();
    }
    if is_absolute_url(url) {
        return url.to_string();
    }
    if url.starts_with("//") {
        return format!("https:{url}");
    }
    let Some(origin) = origin_of(base) else {
        return url.to_string();
    };
    if url.starts_with('/') {
        return format!("{origin}{url}");
    }
    let prefix = base.rsplit_once('/').map(|(a, _)| a).unwrap_or(base);
    format!("{prefix}/{url}")
}

fn origin_of(base: &str) -> Option<String> {
    let scheme = if base.starts_with("https://") {
        "https"
    } else if base.starts_with("http://") {
        "http"
    } else {
        return None;
    };
    let rest = base.split("://").nth(1)?;
    let host = rest.split('/').next()?;
    Some(format!("{scheme}://{host}"))
}

fn manga_out(m: Manga, base: &str) -> MangaOut {
    MangaOut {
        title: m.title,
        url: m.url,
        thumbnail_url: absolutize(base, &m.thumbnail_url),
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

fn page_out(p: PageRef, base: &str) -> PageOut {
    let image = if p.image_url.is_empty() {
        p.url.clone()
    } else {
        p.image_url
    };
    let image_url = if is_absolute_url(&image) || is_page_token(&p.url) {
        image
    } else {
        absolutize(base, &image)
    };
    PageOut {
        index: p.index,
        url: p.url,
        image_url,
    }
}
