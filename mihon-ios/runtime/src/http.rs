use dexvm::keiyoushi::{HttpData, HttpResp};
use serde::{Deserialize, Serialize};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Same UA Mihon/TachiManga use for WebView + OkHttp. CF clearance is bound
/// to it; mobile Safari HTML also breaks desktop-oriented source selectors.
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
     AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

pub type HttpCallback = unsafe extern "C" fn(*const c_char) -> *mut c_char;

static HOST_HTTP: Mutex<Option<HttpCallback>> = Mutex::new(None);

pub fn set_host_http(cb: Option<HttpCallback>) {
    *HOST_HTTP.lock().expect("http cb lock") = cb;
}

pub fn execute(req: &HttpData) -> HttpResp {
    if let Some(cb) = *HOST_HTTP.lock().expect("http cb lock") {
        return execute_host(cb, req);
    }
    execute_ureq(req)
}

fn execute_host(cb: HttpCallback, req: &HttpData) -> HttpResp {
    #[derive(Serialize)]
    struct WireReq<'a> {
        method: &'a str,
        url: &'a str,
        headers: &'a [(String, String)],
        body: Option<&'a str>,
    }
    #[derive(Deserialize)]
    struct WireResp {
        code: i32,
        #[serde(default)]
        message: String,
        #[serde(default)]
        headers: Vec<(String, String)>,
        #[serde(default, alias = "bodyB64", alias = "body_b64")]
        body_b64: Option<String>,
    }

    let payload = WireReq {
        method: &req.method,
        url: &req.url,
        headers: &req.headers,
        body: req.body.as_deref(),
    };
    let json = match serde_json::to_string(&payload) {
        Ok(s) => s,
        Err(e) => {
            return HttpResp {
                code: 0,
                message: e.to_string(),
                headers: Vec::new(),
                body: None,
            }
        }
    };
    let cstr = match CString::new(json) {
        Ok(s) => s,
        Err(e) => {
            return HttpResp {
                code: 0,
                message: e.to_string(),
                headers: Vec::new(),
                body: None,
            }
        }
    };
    let raw = unsafe { cb(cstr.as_ptr()) };
    if raw.is_null() {
        return HttpResp {
            code: 0,
            message: "host http returned null".into(),
            headers: Vec::new(),
            body: None,
        };
    }
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { libc_free(raw as *mut c_void) };
    match serde_json::from_str::<WireResp>(&text) {
        Ok(r) => {
            let body = r.body_b64.and_then(|b| {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.decode(b).ok()
            });
            mark_challenge(HttpResp {
                code: r.code,
                message: if r.message.is_empty() {
                    "OK".into()
                } else {
                    r.message
                },
                headers: r.headers,
                body,
            })
        }
        Err(e) => HttpResp {
            code: 0,
            message: format!("host http json: {e}"),
            headers: Vec::new(),
            body: None,
        },
    }
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .build()
            .into()
    })
}

fn execute_ureq(req: &HttpData) -> HttpResp {
    let method = req.method.to_ascii_uppercase();
    let result = match method.as_str() {
        "POST" | "PUT" | "PATCH" => {
            let mut request = match method.as_str() {
                "PUT" => agent().put(&req.url),
                "PATCH" => agent().patch(&req.url),
                _ => agent().post(&req.url),
            };
            for (k, v) in &req.headers {
                request = request.header(k, v);
            }
            request.send(req.body.as_deref().unwrap_or(""))
        }
        other => {
            let mut request = match other {
                "HEAD" => agent().head(&req.url),
                "DELETE" => agent().delete(&req.url),
                _ => agent().get(&req.url),
            };
            for (k, v) in &req.headers {
                request = request.header(k, v);
            }
            request.call()
        }
    };
    match result {
        Ok(resp) => {
            let out = mark_challenge(to_resp(resp));
            http_debug(&req.method, &req.url, out.code);
            out
        }
        Err(e) => {
            http_debug(&req.method, &req.url, 0);
            HttpResp {
                code: 0,
                message: e.to_string(),
                headers: Vec::new(),
                body: None,
            }
        }
    }
}

fn http_debug(method: &str, url: &str, code: i32) {
    if std::env::var_os("MIHON_HTTP_DEBUG").is_some() {
        eprintln!("http {method} {code} {url}");
    }
}

/// Challenge HTML often arrives as HTTP 200. Parse that as a catalog and you
/// get zero titles — force a 403 so awaitSuccess throws instead.
pub fn is_challenge(code: i32, headers: &[(String, String)], body: &[u8]) -> bool {
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.to_ascii_lowercase())
            .unwrap_or_default()
    };
    if header("cf-mitigated") == "challenge" {
        return true;
    }
    let n = body.len().min(16_384);
    let text = String::from_utf8_lossy(&body[..n]).to_ascii_lowercase();
    // Interstitial pages only — real catalogs often mention turnstile/CF in JS.
    if text.contains("just a moment")
        || text.contains("challenge-platform")
        || text.contains("cf-chl")
        || text.contains("_cf_chl")
        || text.contains("enable javascript and cookies to continue")
        || text.contains("checking your browser before accessing")
        || text.contains("cdn-cgi/challenge-platform")
        || text.contains("challenge-error-title")
        || text.contains("cf-browser-verification")
        || (text.contains("cf-turnstile") && text.contains("cdn-cgi"))
    {
        return true;
    }
    let server = header("server");
    (code == 403 || code == 503 || code == 429)
        && (server.contains("cloudflare")
            || server.contains("ddos-guard")
            || text.contains("cloudflare")
            || text.contains("ddos-guard"))
}

fn mark_challenge(resp: HttpResp) -> HttpResp {
    let body = resp.body.as_deref().unwrap_or(&[]);
    if is_challenge(resp.code, &resp.headers, body) {
        HttpResp {
            code: 403,
            message: "cloudflare challenge".into(),
            headers: resp.headers,
            body: resp.body,
        }
    } else {
        resp
    }
}

fn to_resp(resp: ureq::http::Response<ureq::Body>) -> HttpResp {
    let code = resp.status().as_u16() as i32;
    let headers = resp
        .headers()
        .iter()
        .filter_map(|(k, v)| v.to_str().ok().map(|val| (k.to_string(), val.to_string())))
        .collect();
    let body = resp.into_body().read_to_vec().ok();
    HttpResp {
        code,
        message: "OK".into(),
        headers,
        body,
    }
}

extern "C" {
    fn free(ptr: *mut c_void);
}

unsafe fn libc_free(ptr: *mut c_void) {
    if !ptr.is_null() {
        free(ptr);
    }
}
