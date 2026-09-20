# Midoku Aidoku source list

Install URL:

`https://raw.githubusercontent.com/dreed-7896/extensions/aidoku-extensions/aidoku/public/index.min.json`

The packages are built with the current Aidoku Rust source API. Every source provides search,
title details, chapters, reader pages, popular/latest listings, image request headers, and deep
link handling where supported by the website.

## Development

Each directory in `sources/` is a standalone Aidoku source crate. The GitHub Actions workflow
packages all crates and commits the generated source list to `public/`.

The implementation is based on the corresponding Keiyoushi source behavior. See
`ATTRIBUTION.md` and `LICENSE-GPL-3.0`.

