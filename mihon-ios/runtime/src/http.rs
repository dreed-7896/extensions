use dexvm::keiyoushi::{HttpData, HttpResp};
use serde::{Deserialize, Serialize};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) \
     AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1";

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
            HttpResp {
                code: r.code,
                message: if r.message.is_empty() {
                    "OK".into()
                } else {
                    r.message
                },
                headers: r.headers,
                body,
            }
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
            .user_agent(UA)
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .build()
            .into()
    })
}

fn execute_ureq(req: &HttpData) -> HttpResp {
    let result =
        if req.method.eq_ignore_ascii_case("POST") || req.method.eq_ignore_ascii_case("PUT") {
            let mut request = if req.method.eq_ignore_ascii_case("PUT") {
                agent().put(&req.url)
            } else {
                agent().post(&req.url)
            };
            for (k, v) in &req.headers {
                request = request.header(k, v);
            }
            request.send(req.body.as_deref().unwrap_or(""))
        } else {
            let mut request = if req.method.eq_ignore_ascii_case("HEAD") {
                agent().head(&req.url)
            } else {
                agent().get(&req.url)
            };
            for (k, v) in &req.headers {
                request = request.header(k, v);
            }
            request.call()
        };
    match result {
        Ok(resp) => {
            let out = to_resp(resp);
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
