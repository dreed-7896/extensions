extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
};
use serde_json::Value;

pub const DOMAINS: [&str; 2] = ["https://manhwaread.com", "https://manhwaread.org"];

/// Strip either mirror, keeping saved keys stable when the domain changes.
pub fn path(value: &str) -> Option<&str> {
    if value.starts_with('/') && !value.starts_with("//") {
        return Some(value.split(['?', '#']).next().unwrap_or(value));
    }
    for domain in DOMAINS {
        if let Some(path) = value
            .strip_prefix(domain)
            .filter(|path| path.starts_with('/'))
        {
            return Some(path.split(['?', '#']).next().unwrap_or(path));
        }
        if let Some(path) = value
            .strip_prefix(&domain.replace("https:", "http:"))
            .filter(|path| path.starts_with('/'))
        {
            return Some(path.split(['?', '#']).next().unwrap_or(path));
        }
    }
    None
}

pub fn manga_key(value: &str) -> Option<String> {
    let slug = path(value)?.strip_prefix("/manhwa/")?.split('/').next()?;
    (!slug.is_empty()).then(|| format!("/manhwa/{slug}/"))
}

pub fn absolute_url(base: &str, value: &str) -> String {
    let value = value.trim();
    if value.starts_with("//") {
        format!("https:{value}")
    } else if value.starts_with("https://") || value.starts_with("http://") {
        value.into()
    } else {
        format!(
            "{}/{}",
            base.trim_end_matches('/'),
            value.trim_start_matches('/')
        )
    }
}

pub fn chapter_number(title: &str) -> Option<f32> {
    let lower = title.to_ascii_lowercase();
    let candidate = lower
        .split_once("chapter")
        .or_else(|| lower.split_once("episode"))
        .map(|(_, tail)| tail)
        .unwrap_or(title);
    candidate
        .split(|ch: char| !ch.is_ascii_digit() && ch != '.')
        .find_map(|part| part.parse().ok())
}

/// Extract only the chapterData JSON object, including multiline/escaped strings.
/// Other script objects after the assignment must not be included.
pub fn chapter_data(script: &str) -> Option<&str> {
    let mut remaining = script;
    while let Some(index) = remaining.find("chapterData") {
        let tail = remaining.get(index + "chapterData".len()..)?;
        remaining = tail;
        let Some(tail) = tail.trim_start().strip_prefix('=') else {
            continue;
        };
        let tail = tail.trim_start();
        if !tail.starts_with('{') {
            continue;
        }
        let mut depth = 0;
        let mut in_string = false;
        let mut escaped = false;
        for (index, ch) in tail.char_indices() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
            } else {
                match ch {
                    '"' => in_string = true,
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(&tail[..=index]);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    None
}

pub fn image_urls(script: &str) -> core::result::Result<Vec<String>, &'static str> {
    let data: Value = serde_json::from_str(chapter_data(script).ok_or("Chapter data not found")?)
        .map_err(|_| "Invalid chapter data")?;
    let base = data
        .get("base")
        .and_then(Value::as_str)
        .filter(|base| !base.trim().is_empty())
        .ok_or("Missing chapter image base")?;
    let encoded = data
        .get("data")
        .and_then(Value::as_str)
        .ok_or("Missing chapter images")?;
    let encoded = encoded
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let decoded = STANDARD
        .decode(encoded.as_bytes())
        .or_else(|_| STANDARD_NO_PAD.decode(encoded.as_bytes()))
        .map_err(|_| "Invalid encoded chapter images")?;
    let images: Value = serde_json::from_slice(&decoded).map_err(|_| "Invalid chapter images")?;
    let images = images.as_array().ok_or("Invalid chapter image list")?;
    let mut result = Vec::with_capacity(images.len());
    for image in images {
        let src = image
            .get("src")
            .and_then(Value::as_str)
            .filter(|src| !src.trim().is_empty())
            .ok_or("Missing chapter image URL")?;
        result.push(absolute_url(base, src));
    }
    if result.is_empty() {
        return Err("No readable pages returned");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_keys_and_deep_links() {
        assert_eq!(
            manga_key("https://manhwaread.org/manhwa/title/chapter-2/?x=1").as_deref(),
            Some("/manhwa/title/")
        );
        assert_eq!(
            manga_key("http://manhwaread.com/manhwa/title/#x").as_deref(),
            Some("/manhwa/title/")
        );
        assert_eq!(manga_key("https://manhwaread.com.evil/manhwa/title/"), None);
        assert_eq!(manga_key("/manhwa/"), None);
        assert_eq!(manga_key("//evil/manhwa/title/"), None);
    }

    #[test]
    fn decodes_multiline_payload_in_page_order() {
        let encoded = STANDARD_NO_PAD.encode(
            br#"[{"src":"01.webp"},{"src":"/02.webp"},{"src":"https://cdn.test/03.webp"}]"#,
        );
        let script = format!(
            "var chapterData = {{\n\"base\":\"https://images.test/folder/\",\"data\":\"{encoded}\",\"extra\":{{\"value\":\"}}\\\"\"}}\n}}; var unrelated = {{}};"
        );
        assert_eq!(
            image_urls(&script).unwrap(),
            [
                "https://images.test/folder/01.webp",
                "https://images.test/folder/02.webp",
                "https://cdn.test/03.webp"
            ]
        );
    }

    #[test]
    fn invalid_payload_fails_instead_of_losing_pages() {
        assert!(
            image_urls("var chapterData = {\"base\":\"https://cdn.test\",\"data\":\"bad\"};")
                .is_err()
        );
        for payload in ["[]", "[{\"src\":\"a\"},{}]"] {
            let script = format!(
                "chapterData={{\"base\":\"https://cdn.test\",\"data\":\"{}\"}}",
                STANDARD.encode(payload)
            );
            assert!(image_urls(&script).is_err());
        }
    }

    #[test]
    fn decimal_chapters_and_protocol_relative_images() {
        assert_eq!(chapter_number("Season 2 - Chapter 12.5"), Some(12.5));
        assert_eq!(chapter_number("Prologue"), None);
        assert_eq!(
            absolute_url(DOMAINS[0], "//cdn.test/a.webp"),
            "https://cdn.test/a.webp"
        );
    }
}
