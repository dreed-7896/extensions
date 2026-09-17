use crate::engine::Engine;
use crate::http::{self, HttpCallback};
use serde::Deserialize;
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;

#[derive(Deserialize, Default)]
struct Args {
    #[serde(default)]
    source: usize,
    #[serde(default = "one")]
    page: i32,
    #[serde(default)]
    query: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    name: String,
    #[serde(default, alias = "pageUrl", alias = "page_url")]
    page_url: String,
}

fn one() -> i32 {
    1
}

fn cstr_to_str<'a>(p: *const c_char) -> Result<&'a str, String> {
    if p.is_null() {
        return Err("null string".into());
    }
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .map_err(|e| e.to_string())
}

fn set_err(err: *mut *mut c_char, msg: String) {
    if err.is_null() {
        return;
    }
    unsafe {
        *err = CString::new(msg.replace('\0', "")).map_or(ptr::null_mut(), CString::into_raw);
    }
}

fn to_c_json(v: Value) -> Result<*mut c_char, String> {
    let s = serde_json::to_string(&v).map_err(|e| e.to_string())?;
    CString::new(s)
        .map(CString::into_raw)
        .map_err(|e| e.to_string())
}

#[no_mangle]
pub extern "C" fn mihon_set_http(cb: Option<HttpCallback>) {
    http::set_host_http(cb);
}

#[no_mangle]
pub extern "C" fn mihon_set_js(cb: Option<crate::extra_shims::JsCallback>) {
    crate::extra_shims::set_host_js(cb);
}

#[no_mangle]
pub extern "C" fn mihon_open_file(apk_path: *const c_char, err: *mut *mut c_char) -> *mut Engine {
    match cstr_to_str(apk_path).and_then(Engine::open_file) {
        Ok(engine) => Box::into_raw(Box::new(engine)),
        Err(e) => {
            set_err(err, e);
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn mihon_close(engine: *mut Engine) {
    if !engine.is_null() {
        unsafe {
            drop(Box::from_raw(engine));
        }
    }
}

#[no_mangle]
pub extern "C" fn mihon_call(
    engine: *mut Engine,
    op: *const c_char,
    args_json: *const c_char,
    err: *mut *mut c_char,
) -> *mut c_char {
    if engine.is_null() {
        set_err(err, "null engine".into());
        return ptr::null_mut();
    }
    let engine = unsafe { &mut *engine };
    let op = match cstr_to_str(op) {
        Ok(s) => s,
        Err(e) => {
            set_err(err, e);
            return ptr::null_mut();
        }
    };
    let args_raw = if args_json.is_null() {
        "{}"
    } else {
        match cstr_to_str(args_json) {
            Ok(s) if s.is_empty() => "{}",
            Ok(s) => s,
            Err(e) => {
                set_err(err, e);
                return ptr::null_mut();
            }
        }
    };
    let args: Args = serde_json::from_str(args_raw).unwrap_or_default();
    let result = dispatch(engine, op, args);
    match result {
        Ok(v) => match to_c_json(v) {
            Ok(p) => p,
            Err(e) => {
                set_err(err, e);
                ptr::null_mut()
            }
        },
        Err(e) => {
            set_err(err, e);
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn mihon_string_free(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            drop(CString::from_raw(s));
        }
    }
}

fn dispatch(engine: &mut Engine, op: &str, args: Args) -> Result<Value, String> {
    match op {
        "sources" => Ok(json!({ "sources": engine.sources()? })),
        "popular" => {
            let (entries, has_next) = engine.popular(args.source, args.page)?;
            Ok(json!({ "entries": entries, "hasNext": has_next }))
        }
        "latest" => {
            let (entries, has_next) = engine.latest(args.source, args.page)?;
            Ok(json!({ "entries": entries, "hasNext": has_next }))
        }
        "search" => {
            let (entries, has_next) = engine.search(args.source, args.page, &args.query)?;
            Ok(json!({ "entries": entries, "hasNext": has_next }))
        }
        "details" => Ok(json!(engine.details(args.source, &args.url, &args.title)?)),
        "chapters" => Ok(json!({
            "chapters": engine.chapters(args.source, &args.url, &args.title)?
        })),
        "pages" => Ok(json!({
            "pages": engine.pages(args.source, &args.url, &args.name)?
        })),
        "image" => {
            let page_url = if args.page_url.is_empty() {
                None
            } else {
                Some(args.page_url.as_str())
            };
            let bytes = engine.image(args.source, &args.url, page_url)?;
            let body_b64 = {
                use base64::Engine as _;
                base64::engine::general_purpose::STANDARD.encode(bytes)
            };
            Ok(json!({ "bodyB64": body_b64 }))
        }
        other => Err(format!("unknown op '{other}'")),
    }
}
