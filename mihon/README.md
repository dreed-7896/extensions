# R2 Library — Mihon / TachiManga / Tachiyomi extension

Kotlin `HttpSource` (lib **1.4**, `1.4.15`). Display name **R2 Library**. Package stays `eu.kanade.tachiyomi.extension.all.r2merge` so existing installs update.

One R2 bucket. Chapters are **folder images**, **.cbz/.zip** (HTTP range), **or** `chapters.json` gallery/remote-archive/page-list entries. A non-empty JSON `chapters[]` replaces folder/cbz for that series.

## R2 layout

```text
<title-id>/
  details.json      # optional (or ComicInfo.xml)
  cover.webp        # optional
  chapters.json     # optional
  Chapter 001/      # image folder
  Chapter 002.cbz   # archive
```

`chapters.json` (replaces folder/cbz chapters for that series when the list is non-empty):

```json
{
  "chapters": [
    { "url": "https://hentairead.com/hentai/example/", "title": "Prologue" },
    { "url": "https://novelcrow.com/comic/slug/", "chapterRange": "2-", "titles": { "2": "Start here" } },
    { "title": "Ch 1", "number": 1, "url": "https://nhentai.net/g/289857/", "pageRange": "50" },
    { "title": "Ch 2", "number": 2, "url": "https://e-hentai.org/g/1503549/c16349ed0a/" },
    { "title": "Ch 3", "number": 3, "url": "https://cdn.example.com/ch3.cbz" },
    { "title": "Ch 4", "number": 4, "pages": ["https://cdn.example.com/4/001.jpg"] }
  ]
}
```

## Install

```
https://github.com/raahat-hossain/extensions/raw/cursor/r2-merge-extension-8f4a/repo/index.pb
```

Or sideload `repo/apk/tachiyomi-all.r2merge-v1.4.15.apk`. Settings: account id / access key / secret / bucket.

`details.json` `"cover"`: absolute URL, `"Chapter 1_1"` (chapter title + 1-based page — folder, `.cbz`, or `chapters.json` gallery), `"chapter 4_24.png"` (page filename), or a relative image key.

## Build

```sh
bash scripts/build-r2-merge-apk.sh
```
