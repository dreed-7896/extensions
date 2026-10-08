use aidoku::{
	alloc::{String, string::ToString},
	imports::{error::AidokuError, net::{Request, Response}},
	prelude::*,
	Result,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{BASE_URL, USER_AGENT};

/// Accept imported URL/path keys without changing the physical key saved by the app.
pub fn gallery_id(key: &str, url: Option<&str>) -> Result<String> {
	for candidate in [Some(key), url].into_iter().flatten() {
		let candidate = candidate.trim().split(['?', '#']).next().unwrap_or_default();
		let path = candidate.strip_prefix(BASE_URL)
			.or_else(|| candidate.strip_prefix("https://www.nhentai.net"))
			.unwrap_or(candidate)
			.trim_matches('/');
		let id = path.strip_prefix("g/").unwrap_or(path).split('/').next().unwrap_or_default();
		if !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())
			&& let Ok(number) = id.parse::<i32>()
			&& number > 0
		{
			return Ok(number.to_string());
		}
	}
	bail!("This nhentai entry has no valid gallery ID. Open its original gallery link and add it again.");
}

pub fn request_json<T: DeserializeOwned>(url: &str) -> Result<T> {
	decode_response(Request::get(url)?
		.header("User-Agent", USER_AGENT)
		.header("Referer", BASE_URL)
		.header("Accept", "application/json")
		.send()?)
}

pub fn decode_response<T: DeserializeOwned>(response: Response) -> Result<T> {
	let status = response.status_code();
	decode_body(status, &response.get_data()?)
}

/// Error objects are not galleries. Never fill missing gallery fields with defaults.
fn decode_body<T: DeserializeOwned>(status: i32, body: &[u8]) -> Result<T> {
	match status {
		401 | 403 => bail!("nhentai denied API access. Open the source website, sign in if needed, and try again."),
		404 => bail!("This nhentai gallery is unavailable or its link is invalid."),
		429 => bail!("nhentai is limiting requests. Please try again later."),
		500..=599 => bail!("nhentai is temporarily unavailable. Please try again later."),
		_ => {}
	}
	let value: Value = serde_json::from_slice(body).map_err(|_| {
		AidokuError::message("nhentai returned a page instead of API data. Open the source website and try again.")
	})?;
	if let Some(error) = value.get("error")
		&& !error.is_null() && error != &Value::Bool(false)
	{
		let message = error.as_str()
			.or_else(|| error.get("message").and_then(Value::as_str))
			.or_else(|| value.get("message").and_then(Value::as_str));
		if let Some(message) = message.filter(|message| !message.is_empty()) {
			let message = message.chars().take(160).collect::<String>();
			bail!("nhentai: {message}");
		}
		bail!("nhentai rejected the request. Open the source website and check that the gallery is available.");
	}
	if !(200..300).contains(&status) {
		bail!("nhentai request failed (HTTP {status}). Please try again later.");
	}
	serde_json::from_value(value).map_err(|_| {
		AidokuError::message("nhentai returned incomplete gallery data. Update the extension and try again.")
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use aidoku_test::aidoku_test;

	#[aidoku_test]
	fn accepts_imported_gallery_keys() {
		for key in ["123456", "/g/123456/", "https://nhentai.net/g/123456/?page=2"] {
			assert_eq!(gallery_id(key, None).unwrap(), "123456");
		}
		assert_eq!(gallery_id("legacy-key", Some("https://nhentai.net/g/123456/")).unwrap(), "123456");
		assert!(gallery_id("", None).is_err());
		assert!(gallery_id("https://other.example/g/123456/", None).is_err());
		assert!(gallery_id("/g/not-a-number/", None).is_err());
	}

	#[aidoku_test]
	fn reports_api_errors_before_gallery_deserialization() {
		let result = decode_body::<crate::models::NHentaiGallery>(200, br#"{"error":"Not found"}"#);
		match result.unwrap_err() {
			AidokuError::Message(message) => assert_eq!(message, "nhentai: Not found"),
			other => panic!("Expected API message, got {other:?}"),
		}
		assert!(matches!(decode_body::<Value>(403, b"<html>blocked</html>"), Err(AidokuError::Message(_))));
		assert!(matches!(decode_body::<crate::models::NHentaiGallery>(200, b"{}"), Err(AidokuError::Message(_))));
	}

	#[aidoku_test]
	fn preserves_successful_response_data() {
		let value: Value = decode_body(200, br#"{"id":123456,"error":false}"#).unwrap();
		assert_eq!(value["id"], 123456);
		let value: crate::models::NHentaiSearchResponse = decode_body(200, br#"{"result":[],"num_pages":0,"per_page":25}"#).unwrap();
		assert!(value.result.is_empty());
		let gallery: crate::models::NHentaiGallery = decode_body(200, br#"{
			"id":123456,"media_id":"654321","title":{"english":"Fixture","japanese":null,"pretty":"Fixture"},
			"cover":{"path":"/galleries/654321/cover.jpg","width":1,"height":1},
			"thumbnail":{"path":"/galleries/654321/thumb.jpg","width":1,"height":1},
			"scanlator":"","upload_date":1,"tags":[],"num_pages":1,"num_favorites":0,
			"pages":[{"number":1,"path":"/galleries/654321/1.jpg","width":1,"height":1,
			"thumbnail":"/galleries/654321/1t.jpg","thumbnail_width":1,"thumbnail_height":1}]
		}"#).unwrap();
		assert_eq!(gallery.id, 123456);
		assert_eq!(gallery.pages[0].path, "/galleries/654321/1.jpg");
	}
}
