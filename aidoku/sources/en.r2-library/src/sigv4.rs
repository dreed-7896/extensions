use aidoku::{
	alloc::{format, string::{String, ToString}, vec, vec::Vec},
	imports::{defaults::defaults_get, net::Request, std::current_date},
	prelude::*,
	Result,
};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::util::{percent_encode, unix_date_parts};

type HmacSha256 = Hmac<Sha256>;

const SERVICE: &str = "s3";
const ALGORITHM: &str = "AWS4-HMAC-SHA256";
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

#[derive(Clone)]
pub struct R2Config {
	pub endpoint: String,
	pub host: String,
	pub bucket: String,
	pub access_key: String,
	pub secret_key: String,
	pub region: String,
	pub root_prefix: String,
	pub public_base: Option<String>,
}

impl R2Config {
	pub fn load() -> Result<Self> {
		let raw_endpoint = setting("r2_endpoint");
		let bucket = setting("r2_bucket");
		let access_key = setting("r2_access_key_id");
		let secret_key = setting("r2_secret_access_key");
		if raw_endpoint.is_empty() || bucket.is_empty() || access_key.is_empty() || secret_key.is_empty() {
			bail!("R2 is not configured. Open R2 Library settings and enter the account ID, bucket, Access Key ID, and Secret Access Key.");
		}

		let endpoint = if !raw_endpoint.contains('.') && !raw_endpoint.contains('/') {
			format!("https://{raw_endpoint}.r2.cloudflarestorage.com")
		} else if raw_endpoint.starts_with("https://") || raw_endpoint.starts_with("http://") {
			raw_endpoint.trim_end_matches('/').to_string()
		} else {
			format!("https://{}", raw_endpoint.trim_end_matches('/'))
		};
		let authority = endpoint
			.split_once("://")
			.map(|(_, rest)| rest)
			.unwrap_or(endpoint.as_str())
			.split('/')
			.next()
			.unwrap_or("");
		if authority.is_empty() {
			bail!("The R2 endpoint is invalid.");
		}
		let scheme = if endpoint.starts_with("http://") { "http" } else { "https" };
		let endpoint = format!("{scheme}://{authority}");
		let root = setting("r2_root_prefix").trim_matches('/').to_string();
		let public_base = setting("r2_public_base_url");
		let public_base = if public_base.is_empty() {
			None
		} else if public_base.starts_with("https://") || public_base.starts_with("http://") {
			Some(public_base.trim_end_matches('/').to_string())
		} else {
			Some(format!("https://{}", public_base.trim_end_matches('/')))
		};

		Ok(Self {
			endpoint,
			host: authority.to_string(),
			bucket,
			access_key,
			secret_key,
			region: {
				let value = setting("r2_region");
				if value.is_empty() { "auto".into() } else { value }
			},
			root_prefix: if root.is_empty() { String::new() } else { format!("{root}/") },
			public_base,
		})
	}

	pub fn object_path(&self, key: &str) -> String {
		format!("/{}/{}", percent_encode(&self.bucket, true), percent_encode(key, false))
	}

	pub fn content_url(&self, key: &str) -> String {
		if let Some(base) = &self.public_base {
			format!("{base}/{}", percent_encode(key, false))
		} else {
			self.presigned_object_url(key, 604_800)
		}
	}

	pub fn signed_get(&self, path: &str, params: Vec<(String, String)>, range: Option<&str>) -> Result<Request> {
		let timestamp = current_date();
		let (amz_date, date_stamp) = formatted_date(timestamp);
		let query = canonical_query(params);
		let url = if query.is_empty() {
			format!("{}{path}", self.endpoint)
		} else {
			format!("{}{path}?{query}", self.endpoint)
		};
		let canonical_headers = format!(
			"host:{}\nx-amz-content-sha256:{}\nx-amz-date:{}\n",
			self.host, EMPTY_SHA256, amz_date,
		);
		let signed_headers = "host;x-amz-content-sha256;x-amz-date";
		let canonical_request = format!(
			"GET\n{path}\n{query}\n{canonical_headers}\n{signed_headers}\n{EMPTY_SHA256}",
		);
		let authorization = self.authorization(&date_stamp, &amz_date, signed_headers, &canonical_request);
		let mut request = Request::get(url)?;
		request.set_header("x-amz-date".to_string(), amz_date);
		request.set_header("x-amz-content-sha256".to_string(), EMPTY_SHA256.to_string());
		request.set_header("Authorization".to_string(), authorization);
		if let Some(range) = range {
			request.set_header("Range".to_string(), range.to_string());
		}
		Ok(request)
	}

	pub fn signed_object_get(&self, key: &str, range: Option<&str>) -> Result<Request> {
		self.signed_get(&self.object_path(key), Vec::new(), range)
	}

	pub fn presigned_object_url(&self, key: &str, expires: i64) -> String {
		let timestamp = current_date();
		let (amz_date, date_stamp) = formatted_date(timestamp);
		let scope = format!("{date_stamp}/{}/{SERVICE}/aws4_request", self.region);
		let path = self.object_path(key);
		let query = canonical_query(vec![
			("X-Amz-Algorithm".into(), ALGORITHM.into()),
			("X-Amz-Credential".into(), format!("{}/{}", self.access_key, scope)),
			("X-Amz-Date".into(), amz_date.clone()),
			("X-Amz-Expires".into(), expires.clamp(60, 604_800).to_string()),
			("X-Amz-SignedHeaders".into(), "host".into()),
		]);
		let canonical_request = format!(
			"GET\n{path}\n{query}\nhost:{}\n\nhost\nUNSIGNED-PAYLOAD",
			self.host,
		);
		let string_to_sign = string_to_sign(&amz_date, &scope, &canonical_request);
		let key = signing_key(&self.secret_key, &date_stamp, &self.region);
		let signature = hex(&hmac(&key, string_to_sign.as_bytes()));
		format!("{}{path}?{query}&X-Amz-Signature={signature}", self.endpoint)
	}

	fn authorization(&self, date_stamp: &str, amz_date: &str, signed_headers: &str, canonical_request: &str) -> String {
		let scope = format!("{date_stamp}/{}/{SERVICE}/aws4_request", self.region);
		let string_to_sign = string_to_sign(amz_date, &scope, canonical_request);
		let key = signing_key(&self.secret_key, date_stamp, &self.region);
		let signature = hex(&hmac(&key, string_to_sign.as_bytes()));
		format!(
			"{ALGORITHM} Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
			self.access_key,
		)
	}
}

fn setting(key: &str) -> String {
	defaults_get::<String>(key).unwrap_or_default().trim().to_string()
}

fn canonical_query(mut params: Vec<(String, String)>) -> String {
	let mut encoded = params
		.drain(..)
		.map(|(key, value)| (percent_encode(&key, true), percent_encode(&value, true)))
		.collect::<Vec<_>>();
	encoded.sort();
	encoded
		.into_iter()
		.map(|(key, value)| format!("{key}={value}"))
		.collect::<Vec<_>>()
		.join("&")
}

fn formatted_date(timestamp: i64) -> (String, String) {
	let (year, month, day, hour, minute, second) = unix_date_parts(timestamp);
	(
		format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z"),
		format!("{year:04}{month:02}{day:02}"),
	)
}

fn string_to_sign(amz_date: &str, scope: &str, canonical_request: &str) -> String {
	format!(
		"{ALGORITHM}\n{amz_date}\n{scope}\n{}",
		hex(&Sha256::digest(canonical_request.as_bytes())),
	)
}

fn signing_key(secret: &str, date: &str, region: &str) -> Vec<u8> {
	let date_key = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
	let region_key = hmac(&date_key, region.as_bytes());
	let service_key = hmac(&region_key, SERVICE.as_bytes());
	hmac(&service_key, b"aws4_request")
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
	let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts keys of any size");
	mac.update(data);
	mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
	const HEX: &[u8; 16] = b"0123456789abcdef";
	let mut output = String::with_capacity(bytes.len() * 2);
	for byte in bytes {
		output.push(HEX[(byte >> 4) as usize] as char);
		output.push(HEX[(byte & 0x0f) as usize] as char);
	}
	output
}
