use aidoku::{
	alloc::{format, string::String, vec, vec::Vec},
	prelude::*,
	Result,
};

use crate::{
	sigv4::R2Config,
	util::{days_from_civil, percent_decode, percent_encode},
};

#[derive(Clone, Debug)]
pub struct S3Object {
	pub key: String,
	pub size: u64,
	pub last_modified: i64,
}

#[derive(Default)]
pub struct S3Listing {
	pub prefixes: Vec<String>,
	pub objects: Vec<S3Object>,
	pub next_token: Option<String>,
}

pub fn list_all(
	config: &R2Config,
	prefix: &str,
	delimiter: Option<&str>,
	max_pages: usize,
) -> Result<S3Listing> {
	let mut output = S3Listing::default();
	let mut token = None;
	for _ in 0..max_pages.max(1) {
		let page = list_page(config, prefix, delimiter, token.as_deref())?;
		output.prefixes.extend(page.prefixes);
		output.objects.extend(page.objects);
		token = page.next_token;
		if token.is_none() {
			break;
		}
	}
	output.next_token = token;
	Ok(output)
}

fn list_page(
	config: &R2Config,
	prefix: &str,
	delimiter: Option<&str>,
	continuation: Option<&str>,
) -> Result<S3Listing> {
	let mut params = vec![
		("list-type".into(), "2".into()),
		("max-keys".into(), "1000".into()),
		("encoding-type".into(), "url".into()),
	];
	if !prefix.is_empty() {
		params.push(("prefix".into(), prefix.into()));
	}
	if let Some(delimiter) = delimiter {
		params.push(("delimiter".into(), delimiter.into()));
	}
	if let Some(continuation) = continuation {
		params.push(("continuation-token".into(), continuation.into()));
	}
	let path = format!("/{}", percent_encode(&config.bucket, true));
	let response = config.signed_get(&path, params, None)?.send()?;
	let status = response.status_code();
	let body = response.get_string()?;
	if !(200..300).contains(&status) {
		let code = xml_value(&body, "Code").unwrap_or_default();
		let message = xml_value(&body, "Message").unwrap_or_default();
		let hint = match status {
			400 => "Check the bucket name and account ID / endpoint.",
			401 | 403 => "Check the access key, secret key, and read permission for this bucket.",
			404 => "Bucket not found. Check the bucket name.",
			_ => "",
		};
		bail!("R2 request failed (HTTP {status}) — {code}: {message} {hint}");
	}
	Ok(parse_listing(&body))
}

fn parse_listing(xml: &str) -> S3Listing {
	let objects = xml_blocks(xml, "Contents")
		.into_iter()
		.filter_map(|block| {
			let key = xml_value(block, "Key").map(|value| percent_decode(&value))?;
			Some(S3Object {
				key,
				size: xml_value(block, "Size").and_then(|value| value.parse().ok()).unwrap_or(0),
				last_modified: xml_value(block, "LastModified")
					.and_then(|value| parse_iso8601(&value))
					.unwrap_or(0),
			})
		})
		.collect();
	let prefixes = xml_blocks(xml, "CommonPrefixes")
		.into_iter()
		.filter_map(|block| xml_value(block, "Prefix"))
		.map(|value| percent_decode(&value))
		.collect();
	let truncated = xml_value(xml, "IsTruncated")
		.map(|value| value.eq_ignore_ascii_case("true"))
		.unwrap_or(false);
	let next_token = if truncated {
		xml_value(xml, "NextContinuationToken").filter(|value| !value.is_empty())
	} else {
		None
	};
	S3Listing { prefixes, objects, next_token }
}

fn xml_blocks<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
	let open = format!("<{tag}>");
	let close = format!("</{tag}>");
	let mut output = Vec::new();
	let mut remainder = xml;
	while let Some(start) = remainder.find(&open) {
		let content = &remainder[start + open.len()..];
		let Some(end) = content.find(&close) else { break };
		output.push(&content[..end]);
		remainder = &content[end + close.len()..];
	}
	output
}

pub fn xml_value(xml: &str, tag: &str) -> Option<String> {
	let open = format!("<{tag}>");
	let close = format!("</{tag}>");
	let start = xml.find(&open)? + open.len();
	let end = xml[start..].find(&close)? + start;
	Some(xml_unescape(xml[start..end].trim()))
}

pub fn xml_unescape(value: &str) -> String {
	value
		.replace("&lt;", "<")
		.replace("&gt;", ">")
		.replace("&quot;", "\"")
		.replace("&apos;", "'")
		.replace("&amp;", "&")
}

pub fn parse_iso8601(value: &str) -> Option<i64> {
	if value.len() < 19 {
		return None;
	}
	let year = value.get(0..4)?.parse::<i64>().ok()?;
	let month = value.get(5..7)?.parse::<i64>().ok()?;
	let day = value.get(8..10)?.parse::<i64>().ok()?;
	let hour = value.get(11..13)?.parse::<i64>().ok()?;
	let minute = value.get(14..16)?.parse::<i64>().ok()?;
	let second = value.get(17..19)?.parse::<i64>().ok()?;
	Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second)
}

pub fn object_text(config: &R2Config, object: &S3Object, max_bytes: u64) -> Result<String> {
	if object.size > max_bytes {
		bail!("{} is too large to read.", object.key);
	}
	let response = config.signed_object_get(&object.key, None)?.send()?;
	let status = response.status_code();
	if !(200..300).contains(&status) {
		bail!("Could not read {} (HTTP {status}).", object.key);
	}
	Ok(response.get_string()?)
}
