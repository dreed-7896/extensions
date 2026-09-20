use aidoku::{
	alloc::{format, string::{String, ToString}, vec::Vec},
	imports::net::Request,
	prelude::*,
	Result,
};

use crate::{sigv4::R2Config, util::{is_absolute_url, is_hidden, is_image, natural_cmp}};

const TAIL_BYTES: usize = 128 * 1024;
const MAX_DIRECTORY_BYTES: u64 = 32 * 1024 * 1024;
const EOCD_SIGNATURE: u32 = 0x0605_4b50;
const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
const ZIP64_LOCATOR_SIGNATURE: u32 = 0x0706_4b50;
const ZIP64_EOCD_SIGNATURE: u32 = 0x0606_4b50;

pub fn archive_pages(config: &R2Config, target: &str) -> Result<(String, Vec<String>)> {
	let url = if is_absolute_url(target) { target.to_string() } else { config.content_url(target) };
	let entries = read_entries(&url)?;
	Ok((url, entries))
}

fn read_entries(url: &str) -> Result<Vec<String>> {
	let tail = fetch_tail(url)?;
	let Some(eocd_index) = find_eocd(&tail.bytes) else {
		bail!("Not a valid ZIP/CBZ archive.");
	};
	let mut total_entries = u16_at(&tail.bytes, eocd_index + 10).unwrap_or(0) as u64;
	let mut directory_size = u32_at(&tail.bytes, eocd_index + 12).unwrap_or(0) as u64;
	let mut directory_offset = u32_at(&tail.bytes, eocd_index + 16).unwrap_or(0) as u64;

	if total_entries == 0xffff || directory_size == 0xffff_ffff || directory_offset == 0xffff_ffff {
		let locator_index = eocd_index.checked_sub(20).ok_or_else(|| aidoku::AidokuError::message("Invalid ZIP64 archive"))?;
		if u32_at(&tail.bytes, locator_index) != Some(ZIP64_LOCATOR_SIGNATURE) {
			bail!("Invalid ZIP64 archive.");
		}
		let record_offset = u64_at(&tail.bytes, locator_index + 8)
			.ok_or_else(|| aidoku::AidokuError::message("Invalid ZIP64 locator"))?;
		let record = match tail.slice(record_offset, 56) {
			Some(record) => record,
			None => fetch_range(url, record_offset, record_offset + 55)?,
		};
		if u32_at(&record, 0) != Some(ZIP64_EOCD_SIGNATURE) {
			bail!("Invalid ZIP64 directory.");
		}
		total_entries = u64_at(&record, 32).unwrap_or(0);
		directory_size = u64_at(&record, 40).unwrap_or(0);
		directory_offset = u64_at(&record, 48).unwrap_or(0);
	}
	if directory_size == 0 || directory_size > MAX_DIRECTORY_BYTES {
		bail!("Unsupported ZIP directory size: {directory_size} bytes.");
	}
	let directory = match tail.slice(directory_offset, directory_size as usize) {
		Some(directory) => directory,
		None => fetch_range(url, directory_offset, directory_offset + directory_size - 1)?,
	};
	let mut entries = parse_directory(&directory, total_entries);
	entries.retain(|name| is_image(name) && !is_hidden(name));
	entries.sort_by(|left, right| natural_cmp(left, right));
	if entries.is_empty() {
		bail!("No readable images were found in this ZIP/CBZ archive.");
	}
	Ok(entries)
}

fn parse_directory(directory: &[u8], total_entries: u64) -> Vec<String> {
	let mut entries = Vec::with_capacity(total_entries.min(4096) as usize);
	let mut offset = 0usize;
	while offset + 46 <= directory.len() && u32_at(directory, offset) == Some(CENTRAL_SIGNATURE) {
		let name_length = u16_at(directory, offset + 28).unwrap_or(0) as usize;
		let extra_length = u16_at(directory, offset + 30).unwrap_or(0) as usize;
		let comment_length = u16_at(directory, offset + 32).unwrap_or(0) as usize;
		let name_start = offset + 46;
		let name_end = name_start.saturating_add(name_length);
		if name_end > directory.len() {
			break;
		}
		if let Ok(name) = String::from_utf8(directory[name_start..name_end].to_vec()) {
			if !name.ends_with('/') {
				entries.push(name);
			}
		}
		offset = name_end.saturating_add(extra_length).saturating_add(comment_length);
	}
	entries
}

struct Tail {
	bytes: Vec<u8>,
	start: u64,
}

impl Tail {
	fn slice(&self, offset: u64, length: usize) -> Option<Vec<u8>> {
		let relative = offset.checked_sub(self.start)? as usize;
		let end = relative.checked_add(length)?;
		(end <= self.bytes.len()).then(|| self.bytes[relative..end].to_vec())
	}
}

fn fetch_tail(url: &str) -> Result<Tail> {
	let range = format!("bytes=-{TAIL_BYTES}");
	let mut request = Request::get(url)?;
	request.set_header("Range".to_string(), range);
	let response = request.send()?;
	let status = response.status_code();
	if status != 200 && status != 206 {
		bail!("Could not read archive (HTTP {status}).");
	}
	let start = if status == 206 {
		response
			.get_header("Content-Range")
			.and_then(|value| value.strip_prefix("bytes ").map(ToString::to_string))
			.and_then(|value| value.split('-').next().and_then(|value| value.parse().ok()))
			.unwrap_or(0)
	} else {
		0
	};
	Ok(Tail { bytes: response.get_data()?, start })
}

fn fetch_range(url: &str, start: u64, end: u64) -> Result<Vec<u8>> {
	let mut request = Request::get(url)?;
	request.set_header("Range".to_string(), format!("bytes={start}-{end}"));
	let response = request.send()?;
	let status = response.status_code();
	if status != 200 && status != 206 {
		bail!("Could not read archive range (HTTP {status}).");
	}
	let bytes = response.get_data()?;
	if status == 206 {
		return Ok(bytes);
	}
	let from = (start as usize).min(bytes.len());
	let to = (end.saturating_add(1) as usize).min(bytes.len()).max(from);
	Ok(bytes[from..to].to_vec())
}

fn find_eocd(bytes: &[u8]) -> Option<usize> {
	if bytes.len() < 22 {
		return None;
	}
	let mut fallback = None;
	for index in (0..=bytes.len() - 22).rev() {
		if u32_at(bytes, index) != Some(EOCD_SIGNATURE) {
			continue;
		}
		fallback.get_or_insert(index);
		let comment = u16_at(bytes, index + 20).unwrap_or(0) as usize;
		if index + 22 + comment == bytes.len() {
			return Some(index);
		}
	}
	fallback
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
	Some(u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
	Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

fn u64_at(bytes: &[u8], offset: usize) -> Option<u64> {
	Some(u64::from_le_bytes(bytes.get(offset..offset + 8)?.try_into().ok()?))
}
