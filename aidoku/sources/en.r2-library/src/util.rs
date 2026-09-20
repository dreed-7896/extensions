use aidoku::alloc::{borrow::ToOwned, string::String, vec::Vec};
use core::cmp::Ordering;

const IMAGE_EXTENSIONS: &[&str] = &[
	"jpg", "jpeg", "png", "webp", "gif", "avif", "bmp", "jxl", "heif", "heic",
];
const ARCHIVE_EXTENSIONS: &[&str] = &["cbz", "zip"];

pub fn file_name(value: &str) -> &str {
	value.trim_end_matches('/').rsplit('/').next().unwrap_or(value)
}

pub fn extension(value: &str) -> String {
	file_name(value)
		.rsplit_once('.')
		.map(|(_, extension)| extension.to_ascii_lowercase())
		.unwrap_or_default()
}

pub fn is_hidden(value: &str) -> bool {
	let name = file_name(value);
	name.starts_with('.') || name.eq_ignore_ascii_case("Thumbs.db") || value.contains("__MACOSX/")
}

pub fn is_image(value: &str) -> bool {
	!is_hidden(value) && IMAGE_EXTENSIONS.iter().any(|item| *item == extension(value))
}

pub fn is_archive(value: &str) -> bool {
	!is_hidden(value) && ARCHIVE_EXTENSIONS.iter().any(|item| *item == extension(value))
}

pub fn is_cover(value: &str) -> bool {
	file_name(value)
		.rsplit_once('.')
		.map(|(name, _)| name.eq_ignore_ascii_case("cover"))
		.unwrap_or(false)
}

pub fn is_absolute_url(value: &str) -> bool {
	value.starts_with("https://") || value.starts_with("http://")
}

pub fn natural_cmp(a: &str, b: &str) -> Ordering {
	let left = a.as_bytes();
	let right = b.as_bytes();
	let (mut i, mut j) = (0, 0);
	while i < left.len() && j < right.len() {
		if left[i].is_ascii_digit() && right[j].is_ascii_digit() {
			let (mut li, mut rj) = (i, j);
			while li + 1 < left.len() && left[li] == b'0' && left[li + 1].is_ascii_digit() { li += 1; }
			while rj + 1 < right.len() && right[rj] == b'0' && right[rj + 1].is_ascii_digit() { rj += 1; }
			let (mut le, mut re) = (li, rj);
			while le < left.len() && left[le].is_ascii_digit() { le += 1; }
			while re < right.len() && right[re].is_ascii_digit() { re += 1; }
			let length = (le - li).cmp(&(re - rj));
			if length != Ordering::Equal { return length; }
			let digits = left[li..le].cmp(&right[rj..re]);
			if digits != Ordering::Equal { return digits; }
			i = le;
			j = re;
		} else {
			let order = left[i].to_ascii_lowercase().cmp(&right[j].to_ascii_lowercase());
			if order != Ordering::Equal { return order; }
			i += 1;
			j += 1;
		}
	}
	(left.len() - i).cmp(&(right.len() - j))
}

pub fn chapter_number(value: &str) -> Option<f32> {
	let mut number = String::new();
	let mut started = false;
	for character in value.chars() {
		if character.is_ascii_digit() || (started && character == '.') {
			number.push(character);
			started = true;
		} else if started {
			break;
		}
	}
	number.parse().ok()
}

pub fn percent_encode(value: &str, encode_slash: bool) -> String {
	const HEX: &[u8; 16] = b"0123456789ABCDEF";
	let mut output = String::new();
	for byte in value.as_bytes() {
		let unreserved = byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'.' | b'_' | b'~');
		if unreserved || (*byte == b'/' && !encode_slash) {
			output.push(*byte as char);
		} else {
			output.push('%');
			output.push(HEX[(byte >> 4) as usize] as char);
			output.push(HEX[(byte & 0x0f) as usize] as char);
		}
	}
	output
}

pub fn percent_decode(value: &str) -> String {
	let bytes = value.as_bytes();
	let mut output = Vec::with_capacity(bytes.len());
	let mut index = 0;
	while index < bytes.len() {
		if bytes[index] == b'%' && index + 2 < bytes.len() {
			if let (Some(high), Some(low)) = (hex_value(bytes[index + 1]), hex_value(bytes[index + 2])) {
				output.push((high << 4) | low);
				index += 3;
				continue;
			}
		}
		output.push(bytes[index]);
		index += 1;
	}
	String::from_utf8(output).unwrap_or_else(|_| value.to_owned())
}

fn hex_value(value: u8) -> Option<u8> {
	match value {
		b'0'..=b'9' => Some(value - b'0'),
		b'a'..=b'f' => Some(value - b'a' + 10),
		b'A'..=b'F' => Some(value - b'A' + 10),
		_ => None,
	}
}

pub fn unix_date_parts(timestamp: i64) -> (i32, u32, u32, u32, u32, u32) {
	let days = timestamp.div_euclid(86_400);
	let seconds = timestamp.rem_euclid(86_400) as u32;
	let z = days + 719_468;
	let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
	let doe = z - era * 146_097;
	let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
	let mut year = yoe + era * 400;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let day = doy - (153 * mp + 2) / 5 + 1;
	let month = mp + if mp < 10 { 3 } else { -9 };
	year += if month <= 2 { 1 } else { 0 };
	(
		year as i32,
		month as u32,
		day as u32,
		seconds / 3600,
		(seconds % 3600) / 60,
		seconds % 60,
	)
}

pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
	let adjusted_year = year - if month <= 2 { 1 } else { 0 };
	let era = if adjusted_year >= 0 { adjusted_year } else { adjusted_year - 399 } / 400;
	let yoe = adjusted_year - era * 400;
	let shifted_month = month + if month > 2 { -3 } else { 9 };
	let doy = (153 * shifted_month + 2) / 5 + day - 1;
	let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
	era * 146_097 + doe - 719_468
}
