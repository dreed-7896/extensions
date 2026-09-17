//! Generic Mihon host: CatalogueSource defaults + missing Android/Java stubs.
//!
//! Keiyoushi APKs compile against extension-lib (`compileOnly`). Mihon supplies
//! HttpSource/CatalogueSource defaults; dexvm is missing some of those plus a
//! pile of `android.*` / `java.time.*` classes. Heal those as a host, never
//! per-source.

use dexvm::vm::error::JvmError;
use dexvm::vm::object::Native;
use dexvm::vm::value::JValue;
use dexvm::vm::{NatErr, NativeEntry, NativeFn, Vm};

const HTTP_SOURCE: &str = "Leu/kanade/tachiyomi/source/online/HttpSource;";
const CATALOGUE: &str = "Leu/kanade/tachiyomi/source/CatalogueSource;";
const SMANGA_UPDATE: &str = "Leu/kanade/tachiyomi/source/model/SMangaUpdate;";
const CONT: &str = "Lkotlin/coroutines/jvm/internal/ContinuationImpl;";
const GET_MANGA_UPDATE_SIG: &str = "(Leu/kanade/tachiyomi/source/model/SManga;Ljava/util/List;ZZLkotlin/coroutines/Continuation;)Ljava/lang/Object;";
const GET_IMAGE_URL_SIG: &str =
    "(Leu/kanade/tachiyomi/source/model/Page;Lkotlin/coroutines/Continuation;)Ljava/lang/Object;";
const HEADERS_BUILDER: &str = "Lokhttp3/Headers$Builder;";
const REQUEST: &str = "Lokhttp3/Request;";

pub fn install(vm: &mut Vm) -> Result<(), String> {
    for entry in HOST_NATIVES {
        vm.register_native(*entry).map_err(|e| e.to_string())?;
    }
    stub_unresolved(vm)
}

/// If `err` is a missing host class or static, install a stub and return true
/// so the caller can retry the same extension call.
pub fn heal(vm: &mut Vm, err: &str) -> bool {
    if let Some(desc) = parse_class_not_found(err) {
        return stub_class(vm, &desc).is_ok();
    }
    if let Some((name, ty, owner)) = parse_no_static_field(err) {
        return install_missing_static(vm, &owner, &name, &ty).is_ok();
    }
    if let Some((name, sig, class)) = parse_no_method(err) {
        return stub_missing_method(vm, &class, &name, &sig).is_ok();
    }
    false
}

const HOST_NATIVES: &[NativeEntry] = &[
    NativeEntry {
        class: HTTP_SOURCE,
        name: "getMangaUpdate",
        sig: GET_MANGA_UPDATE_SIG,
        instance: true,
        f: http_source_get_manga_update,
    },
    NativeEntry {
        class: CATALOGUE,
        name: "getMangaUpdate",
        sig: GET_MANGA_UPDATE_SIG,
        instance: true,
        f: http_source_get_manga_update,
    },
    NativeEntry {
        class: HTTP_SOURCE,
        name: "getImageUrl",
        sig: GET_IMAGE_URL_SIG,
        instance: true,
        f: http_source_get_image_url,
    },
    NativeEntry {
        class: HTTP_SOURCE,
        name: "headersBuilder",
        sig: "()Lokhttp3/Headers$Builder;",
        instance: true,
        f: http_source_headers_builder,
    },
    NativeEntry {
        class: HTTP_SOURCE,
        name: "getHeaders",
        sig: "()Lokhttp3/Headers;",
        instance: true,
        f: http_source_get_headers,
    },
    NativeEntry {
        class: HTTP_SOURCE,
        name: "imageRequest",
        sig: "(Leu/kanade/tachiyomi/source/model/Page;)Lokhttp3/Request;",
        instance: true,
        f: http_source_image_request,
    },
    NativeEntry {
        class: HTTP_SOURCE,
        name: "mangaDetailsRequest",
        sig: "(Leu/kanade/tachiyomi/source/model/SManga;)Lokhttp3/Request;",
        instance: true,
        f: http_source_get_request,
    },
    NativeEntry {
        class: HTTP_SOURCE,
        name: "chapterListRequest",
        sig: "(Leu/kanade/tachiyomi/source/model/SManga;)Lokhttp3/Request;",
        instance: true,
        f: http_source_get_request,
    },
    NativeEntry {
        class: HTTP_SOURCE,
        name: "pageListRequest",
        sig: "(Leu/kanade/tachiyomi/source/model/SChapter;)Lokhttp3/Request;",
        instance: true,
        f: http_source_get_request,
    },
    NativeEntry {
        class: HTTP_SOURCE,
        name: "relatedMangaListRequest",
        sig: "(Leu/kanade/tachiyomi/source/model/SManga;)Lokhttp3/Request;",
        instance: true,
        f: http_source_get_request,
    },
];

fn jvm_to_nat(e: JvmError) -> NatErr {
    match e {
        JvmError::Uncaught(id) => NatErr::Throw(id),
        other => NatErr::Fatal(other),
    }
}

fn jbool(v: JValue) -> bool {
    match v {
        JValue::Int(i) => i != 0,
        JValue::Long(l) => l != 0,
        JValue::Float(f) => f != 0.0,
        JValue::Double(d) => d != 0.0,
        JValue::Null => false,
        JValue::Obj(_) => true,
    }
}

fn continuation(vm: &mut Vm, supplied: JValue) -> Result<JValue, NatErr> {
    if !supplied.is_null_ref() {
        return Ok(supplied);
    }
    let cid = vm.ensure_class_by_desc(CONT).map_err(NatErr::Fatal)?;
    Ok(JValue::Obj(vm.alloc_instance(cid).map_err(NatErr::Fatal)?))
}

/// Mihon `CatalogueSource.getMangaUpdate`: details/chapters are independent
/// suspend calls. 1.6 sources override this; 1.4 inherit this default.
fn http_source_get_manga_update(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let this = args.first().copied().unwrap_or(JValue::Null);
    let manga = args.get(1).copied().unwrap_or(JValue::Null);
    let chapters = args.get(2).copied().unwrap_or(JValue::Null);
    let fetch_details = args.get(3).copied().map(jbool).unwrap_or(false);
    let fetch_chapters = args.get(4).copied().map(jbool).unwrap_or(false);
    let cont = continuation(vm, args.get(5).copied().unwrap_or(JValue::Null))?;

    let manga_out = if fetch_details {
        vm.invoke_virtual_args(
            this,
            "getMangaDetails",
            "(Leu/kanade/tachiyomi/source/model/SManga;Lkotlin/coroutines/Continuation;)Ljava/lang/Object;",
            vec![manga, cont],
        )
        .map_err(jvm_to_nat)?
    } else {
        manga
    };
    let chapters_out = if fetch_chapters {
        vm.invoke_virtual_args(
            this,
            "getChapterList",
            "(Leu/kanade/tachiyomi/source/model/SManga;Lkotlin/coroutines/Continuation;)Ljava/lang/Object;",
            vec![manga, cont],
        )
        .map_err(jvm_to_nat)?
    } else {
        chapters
    };
    let cid = vm
        .ensure_class_by_desc(SMANGA_UPDATE)
        .map_err(NatErr::Fatal)?;
    Ok(JValue::Obj(vm.arena.alloc(
        cid,
        Vec::new(),
        Some(Native::SMangaUpdate {
            manga: manga_out,
            chapters: chapters_out,
        }),
    )))
}

/// Mihon `HttpSource.getImageUrl` default: already-set `imageUrl`, else
/// `imageUrlRequest` + parse.
fn http_source_get_image_url(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let this = args.first().copied().unwrap_or(JValue::Null);
    let page = args.get(1).copied().unwrap_or(JValue::Null);
    if let Some(Native::SPPage { image_url, .. }) = vm.payload_of(page) {
        if !image_url.is_empty() {
            return Ok(vm.alloc_string(&image_url));
        }
    }
    let request = vm
        .invoke_virtual_args(
            this,
            "imageUrlRequest",
            "(Leu/kanade/tachiyomi/source/model/Page;)Lokhttp3/Request;",
            vec![page],
        )
        .map_err(jvm_to_nat)?;
    let response = vm
        .invoke_static(
            "Leu/kanade/tachiyomi/network/RequestsKt;",
            "__host_execute",
            "(Lokhttp3/Request;)Lokhttp3/Response;",
            vec![request],
        )
        .map_err(jvm_to_nat)?;
    vm.invoke_virtual_args(
        this,
        "imageUrlParse",
        "(Lokhttp3/Response;)Ljava/lang/String;",
        vec![response],
    )
    .map_err(jvm_to_nat)
}

fn alloc_native(vm: &mut Vm, desc: &str, native: Native) -> Result<JValue, NatErr> {
    let cid = vm.ensure_class_by_desc(desc).map_err(NatErr::Fatal)?;
    Ok(JValue::Obj(vm.arena.alloc(cid, Vec::new(), Some(native))))
}

/// Mihon `headersBuilder()`: at least a User-Agent. Source overrides still
/// win via invoke-virtual; `super.headersBuilder()` lands here.
fn http_source_headers_builder(vm: &mut Vm, _args: &[JValue]) -> Result<JValue, NatErr> {
    alloc_native(
        vm,
        HEADERS_BUILDER,
        Native::Headers(vec![("User-Agent".into(), crate::http::USER_AGENT.into())]),
    )
}

/// Mihon `headers` lazy property: `headersBuilder().build()`.
fn http_source_get_headers(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let this = args.first().copied().unwrap_or(JValue::Null);
    let builder = vm
        .invoke_virtual_args(this, "headersBuilder", "()Lokhttp3/Headers$Builder;", vec![])
        .map_err(jvm_to_nat)?;
    vm.invoke_virtual_args(builder, "build", "()Lokhttp3/Headers;", vec![])
        .map_err(jvm_to_nat)
}

fn header_pairs(vm: &Vm, headers: JValue) -> Vec<(String, String)> {
    match vm.payload_of(headers) {
        Some(Native::Headers(h)) => h,
        _ => Vec::new(),
    }
}

/// Mihon `imageRequest(page)`: `GET(page.imageUrl, headers)`.
fn http_source_image_request(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let this = args.first().copied().unwrap_or(JValue::Null);
    let page = args.get(1).copied().unwrap_or(JValue::Null);
    let url = match vm.payload_of(page) {
        Some(Native::SPPage {
            image_url, url, ..
        }) => {
            if image_url.is_empty() {
                url
            } else {
                image_url
            }
        }
        _ => String::new(),
    };
    if url.is_empty() {
        return Err(NatErr::Throw(vm.err_npe()));
    }
    let headers = vm
        .invoke_virtual_args(this, "getHeaders", "()Lokhttp3/Headers;", vec![])
        .map_err(jvm_to_nat)?;
    alloc_native(
        vm,
        REQUEST,
        Native::Request {
            url,
            method: "GET".into(),
            headers: header_pairs(vm, headers),
            body: None,
        },
    )
}

fn obj_url(vm: &Vm, v: JValue) -> Option<String> {
    match vm.payload_of(v) {
        Some(Native::SManga { url, .. } | Native::SChapter { url, .. }) => Some(url),
        Some(Native::Str(s)) => Some(s),
        _ => None,
    }
}

fn join_url(base: &str, url: &str) -> String {
    let url = url.trim();
    if url.is_empty() {
        return base.to_string();
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    if url.starts_with("//") {
        return format!("https:{url}");
    }
    let base = base.trim_end_matches('/');
    if url.starts_with('/') {
        format!("{base}{url}")
    } else {
        format!("{base}/{url}")
    }
}

fn source_base_url(vm: &mut Vm, src: JValue) -> Result<String, NatErr> {
    let v = vm
        .invoke_virtual(src, "getBaseUrl", "()Ljava/lang/String;")
        .map_err(jvm_to_nat)?;
    match vm.payload_of(v) {
        Some(Native::Str(s)) => Ok(s),
        _ => Ok(String::new()),
    }
}

/// `GET(baseUrl + manga/chapter.url, headers)` with slash joining.
fn http_source_get_request(vm: &mut Vm, args: &[JValue]) -> Result<JValue, NatErr> {
    let this = args.first().copied().unwrap_or(JValue::Null);
    let obj = args.get(1).copied().unwrap_or(JValue::Null);
    let rel = obj_url(vm, obj).unwrap_or_default();
    if rel.is_empty() {
        return Err(NatErr::Throw(vm.err_npe()));
    }
    let full = if rel.starts_with("http://") || rel.starts_with("https://") {
        rel
    } else {
        join_url(&source_base_url(vm, this)?, &rel)
    };
    let headers = vm
        .invoke_virtual_args(this, "getHeaders", "()Lokhttp3/Headers;", vec![])
        .map_err(jvm_to_nat)?;
    alloc_native(
        vm,
        REQUEST,
        Native::Request {
            url: full,
            method: "GET".into(),
            headers: header_pairs(vm, headers),
            body: None,
        },
    )
}

fn stub_unresolved(vm: &mut Vm) -> Result<(), String> {
    let mut descs: Vec<String> = Vec::new();
    for dex in &vm.dexes {
        for tid in 0..dex.types.len() as u32 {
            let desc = dex.type_descriptor(tid).to_string();
            if should_pre_stub(&desc) {
                descs.push(desc);
            }
        }
    }
    descs.sort();
    descs.dedup();
    for desc in descs {
        if vm.ensure_class_by_desc(&desc).is_ok() {
            continue;
        }
        let _ = stub_class(vm, &desc);
    }
    Ok(())
}

fn should_pre_stub(desc: &str) -> bool {
    if desc.starts_with('[') {
        return false;
    }
    desc.starts_with("Landroid/")
        || desc.starts_with("Landroidx/")
        || desc.starts_with("Ldalvik/")
        || desc.starts_with("Ljava/time/")
        || desc.starts_with("Lkotlin/time/")
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn stub_ctor(_vm: &mut Vm, _args: &[JValue]) -> Result<JValue, NatErr> {
    Ok(JValue::Null)
}

fn stub_class(vm: &mut Vm, desc: &str) -> Result<(), String> {
    if desc.is_empty() || desc.contains('\0') {
        return Err("bad descriptor".into());
    }
    if vm.ensure_class_by_desc(desc).is_ok() {
        return Ok(());
    }
    let class = leak(desc.to_string());
    vm.register_native(NativeEntry {
        class,
        name: "<init>",
        sig: "()V",
        instance: true,
        f: stub_ctor,
    })
    .map_err(|e| e.to_string())?;
    vm.ensure_class_by_desc(desc).map(|_| ()).map_err(|e| e.to_string())
}

fn parse_no_method(err: &str) -> Option<(String, String, String)> {
    let rest = err.split("no method ").nth(1)?;
    let found = rest.find(" found (on ")?;
    let head = rest[..found].trim();
    let class_part = rest[found + " found (on ".len()..].trim();
    let class = class_part.split_whitespace().next()?.trim_matches(',').to_string();
    let class = if class.ends_with(';') {
        class
    } else if class.starts_with('L') {
        format!("{class};")
    } else {
        class
    };
    let name_end = head.find(' ')?;
    let name = head[..name_end].to_string();
    let sig = head[name_end + 1..].trim().to_string();
    if name.is_empty() || !sig.starts_with('(') {
        return None;
    }
    Some((name, sig, class))
}

fn is_host_class(desc: &str) -> bool {
    desc.starts_with("Lkotlinx/")
        || desc.starts_with("Lkotlin/")
        || desc.starts_with("Ljava/")
        || desc.starts_with("Landroid/")
        || desc.starts_with("Landroidx/")
        || desc.starts_with("Lokhttp3/")
        || desc.starts_with("Lokio/")
        || desc.starts_with("Lorg/jsoup/")
        || desc.starts_with("Ldalvik/")
        || desc.starts_with("Lapp/cash/quickjs/")
}

fn stub_fn_for_ret(ret: &str) -> NativeFn {
    match ret {
        "V" => stub_void,
        "Z" | "B" | "C" | "S" | "I" => stub_int0,
        "J" => stub_long0,
        "F" => stub_float0,
        "D" => stub_double0,
        "Ljava/lang/String;" => stub_empty_string,
        _ => stub_null,
    }
}

fn stub_void(_vm: &mut Vm, _args: &[JValue]) -> Result<JValue, NatErr> {
    Ok(JValue::Null)
}
fn stub_int0(_vm: &mut Vm, _args: &[JValue]) -> Result<JValue, NatErr> {
    Ok(JValue::Int(0))
}
fn stub_long0(_vm: &mut Vm, _args: &[JValue]) -> Result<JValue, NatErr> {
    Ok(JValue::Long(0))
}
fn stub_float0(_vm: &mut Vm, _args: &[JValue]) -> Result<JValue, NatErr> {
    Ok(JValue::Float(0.0))
}
fn stub_double0(_vm: &mut Vm, _args: &[JValue]) -> Result<JValue, NatErr> {
    Ok(JValue::Double(0.0))
}
fn stub_empty_string(vm: &mut Vm, _args: &[JValue]) -> Result<JValue, NatErr> {
    Ok(vm.alloc_string(""))
}
fn stub_null(_vm: &mut Vm, _args: &[JValue]) -> Result<JValue, NatErr> {
    Ok(JValue::Null)
}

fn stub_missing_method(vm: &mut Vm, class: &str, name: &str, sig: &str) -> Result<(), String> {
    if !is_host_class(class) {
        return Err("not a host class".into());
    }
    let ret = sig.rsplit(')').next().unwrap_or("V");
    vm.register_native(NativeEntry {
        class: leak(class.to_string()),
        name: leak(name.to_string()),
        sig: leak(sig.to_string()),
        instance: true,
        f: stub_fn_for_ret(ret),
    })
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn parse_class_not_found(err: &str) -> Option<String> {
    let key = "class not found: ";
    let rest = err.split(key).nth(1)?;
    let desc = rest
        .split(|c: char| c.is_whitespace() || c == ')' || c == ',' || c == '"' || c == '\'')
        .next()?
        .trim();
    if desc.is_empty() {
        return None;
    }
    Some(desc.to_string())
}

fn parse_no_static_field(err: &str) -> Option<(String, String, String)> {
    let key = "no static field ";
    let rest = err.split(key).nth(1)?;
    let mut parts = rest.split_whitespace();
    let name = parts.next()?.to_string();
    let ty = parts.next()?.to_string();
    if parts.next()? != "in" {
        return None;
    }
    let owner = parts
        .next()?
        .trim_matches(|c: char| c == '"' || c == '\'' || c == ',')
        .to_string();
    Some((name, ty, owner))
}

fn install_missing_static(vm: &mut Vm, owner: &str, name: &str, ty: &str) -> Result<(), String> {
    if stub_class(vm, owner).is_err() {
        let _ = vm.ensure_class_by_desc(owner);
    }
    let cid = vm.ensure_class_by_desc(owner).map_err(|e| e.to_string())?;
    let value = well_known_static(vm, owner, name, ty)?;
    crate::extra_shims::install_static(vm, cid, name, ty, value);
    Ok(())
}

fn well_known_static(
    vm: &mut Vm,
    owner: &str,
    name: &str,
    ty: &str,
) -> Result<JValue, String> {
    match (owner, name, ty) {
        ("Landroid/os/Build$VERSION;", "SDK_INT", "I") => Ok(JValue::Int(34)),
        ("Landroid/os/Build$VERSION;", "SDK_INT", "Ljava/lang/Integer;") => Ok(JValue::Int(34)),
        ("Landroid/os/Build$VERSION;", "RELEASE", "Ljava/lang/String;") => {
            Ok(vm.alloc_string("14"))
        }
        ("Landroid/os/Build$VERSION;", "CODENAME", "Ljava/lang/String;") => {
            Ok(vm.alloc_string("REL"))
        }
        ("Landroid/os/Build$VERSION;", "SDK", "Ljava/lang/String;") => Ok(vm.alloc_string("34")),
        ("Ljava/time/ZoneOffset;", "UTC", "Ljava/time/ZoneOffset;")
        | ("Ljava/time/ZoneOffset;", "MIN", "Ljava/time/ZoneOffset;")
        | ("Ljava/time/ZoneOffset;", "MAX", "Ljava/time/ZoneOffset;") => vm
            .alloc_native("Ljava/time/ZoneOffset;", Native::IntBox(0))
            .map_err(|e| e.to_string()),
        _ => default_static(vm, name, ty),
    }
}

fn default_static(vm: &mut Vm, name: &str, ty: &str) -> Result<JValue, String> {
    match ty {
        "I" | "Z" | "B" | "C" | "S" => Ok(JValue::Int(0)),
        "F" => Ok(JValue::Float(0.0)),
        "J" => Ok(JValue::Long(0)),
        "D" => Ok(JValue::Double(0.0)),
        "Ljava/lang/String;" => Ok(vm.alloc_string("")),
        desc if desc.starts_with('L') && desc.ends_with(';') => {
            let _ = stub_class(vm, desc);
            let cid = vm.ensure_class_by_desc(desc).map_err(|e| e.to_string())?;
            if name == "Companion" || name == "INSTANCE" || name == "Default" {
                return vm
                    .alloc_instance(cid)
                    .map(JValue::Obj)
                    .map_err(|e| e.to_string());
            }
            vm.alloc_instance(cid)
                .map(JValue::Obj)
                .map_err(|e| e.to_string())
        }
        _ => Ok(JValue::Null),
    }
}
