use aidoku::{
	alloc::{borrow::ToOwned, string::{String, ToString}, vec::Vec},
	MangaStatus,
};
use serde_json::Value;

use crate::s3::{parse_iso8601, xml_value};

#[derive(Default)]
pub struct Metadata {
	pub title: Option<String>,
	pub author: Option<String>,
	pub artist: Option<String>,
	pub description: Option<String>,
	pub genres: Option<Vec<String>>,
	pub status: MangaStatus,
}

#[derive(Clone)]
pub struct RemoteChapter {
	pub name: String,
	pub archive_url: Option<String>,
	pub pages: Vec<String>,
	pub number: Option<f32>,
	pub date_uploaded: Option<i64>,
	pub scanlator: Option<String>,
}

pub fn details_json(raw: &str) -> Option<Metadata> {
	let root: Value = serde_json::from_str(raw).ok()?;
	Some(Metadata {
		title: string(&root, "title"),
		author: string_or_first(&root, "author"),
		artist: string_or_first(&root, "artist"),
		description: string(&root, "description"),
		genres: string_list(&root, "genre"),
		status: status(string(&root, "status").as_deref()),
	})
}

pub fn comic_info(raw: &str) -> Option<Metadata> {
	let writer = xml_value(raw, "Writer")
		.or_else(|| xml_value(raw, "Penciller"))
		.or_else(|| xml_value(raw, "Inker"));
	let artist = xml_value(raw, "CoverArtist")
		.or_else(|| xml_value(raw, "Penciller"))
		.or_else(|| xml_value(raw, "Letterer"));
	let genres = [xml_value(raw, "Genre"), xml_value(raw, "Tags")]
		.into_iter()
		.flatten()
		.flat_map(|value| {
			value.split(',').map(str::trim).filter(|item| !item.is_empty()).map(ToOwned::to_owned).collect::<Vec<_>>()
		})
		.collect::<Vec<_>>();
	let metadata = Metadata {
		title: xml_value(raw, "Series").or_else(|| xml_value(raw, "Title")),
		author: writer,
		artist,
		description: xml_value(raw, "Summary"),
		genres: (!genres.is_empty()).then_some(genres),
		status: status(
			xml_value(raw, "PublishingStatusTachiyomi")
				.or_else(|| xml_value(raw, "Status"))
				.as_deref(),
		),
	};
	(metadata.title.is_some()
		|| metadata.author.is_some()
		|| metadata.artist.is_some()
		|| metadata.description.is_some())
		.then_some(metadata)
}

pub fn chapter_list(raw: &str) -> Vec<RemoteChapter> {
	let Ok(root) = serde_json::from_str::<Value>(raw) else { return Vec::new() };
	let items = root
		.as_array()
		.or_else(|| root.get("chapters").and_then(Value::as_array));
	let Some(items) = items else { return Vec::new() };
	items.iter().filter_map(remote_chapter).collect()
}

fn remote_chapter(value: &Value) -> Option<RemoteChapter> {
	let archive_url = string(value, "url").or_else(|| string(value, "archive"));
	let pages = value
		.get("pages")
		.and_then(Value::as_array)
		.map(|values| {
			values.iter().filter_map(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned).collect()
		})
		.unwrap_or_default();
	if archive_url.is_none() && pages.is_empty() {
		return None;
	}
	let name = string(value, "name")
		.or_else(|| string(value, "title"))
		.or_else(|| {
			archive_url.as_ref().and_then(|url| {
				url.split('?').next()?.rsplit('/').next()?.rsplit_once('.').map(|(name, _)| name.to_owned())
			})
		})?;
	let number = value.get("number").and_then(|value| {
		value.as_f64().map(|number| number as f32)
			.or_else(|| value.as_str().and_then(|number| number.parse().ok()))
	});
	let date_uploaded = string(value, "date")
		.or_else(|| string(value, "date_upload"))
		.and_then(|value| parse_date(&value));
	Some(RemoteChapter {
		name,
		archive_url,
		pages,
		number,
		date_uploaded,
		scanlator: string(value, "scanlator"),
	})
}

fn string(value: &Value, key: &str) -> Option<String> {
	value.get(key)?.as_str().map(str::trim).filter(|value| !value.is_empty() && *value != "null").map(ToOwned::to_owned)
}

fn string_or_first(value: &Value, key: &str) -> Option<String> {
	string(value, key).or_else(|| {
		value.get(key)?.as_array()?.iter().find_map(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned)
	})
}

fn string_list(value: &Value, key: &str) -> Option<Vec<String>> {
	let value = value.get(key)?;
	let values = if let Some(items) = value.as_array() {
		items.iter().filter_map(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned).collect::<Vec<_>>()
	} else {
		value.as_str()?.split(',').map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned).collect::<Vec<_>>()
	};
	(!values.is_empty()).then_some(values)
}

fn parse_date(value: &str) -> Option<i64> {
	if let Ok(number) = value.parse::<i64>() {
		return Some(if number > 100_000_000_000 { number / 1000 } else { number });
	}
	if value.len() == 10 {
		return parse_iso8601(&format_date(value));
	}
	parse_iso8601(value)
}

fn format_date(value: &str) -> String {
	let mut output = value.to_string();
	output.push_str("T00:00:00Z");
	output
}

fn status(value: Option<&str>) -> MangaStatus {
	match value.unwrap_or("").trim().to_ascii_lowercase().replace('_', " ").as_str() {
		"1" | "ongoing" | "publishing" | "continuing" => MangaStatus::Ongoing,
		"2" | "completed" | "complete" | "finished" | "ended" | "publishing finished" => MangaStatus::Completed,
		"5" | "cancelled" | "canceled" | "abandoned" => MangaStatus::Cancelled,
		"6" | "on hiatus" | "hiatus" => MangaStatus::Hiatus,
		_ => MangaStatus::Unknown,
	}
}
