# R2 Library

Mihon / TachiManga / Tachiyomi extension. Reads a Cloudflare R2 bucket.

Package: `eu.kanade.tachiyomi.extension.all.r2merge` (in-place update of R2 Merge — credentials persist).

Lib **1.4**, `1.4.12`. NSFW.

## Bucket layout

Each folder under the root prefix is a title. Mix any of: image folders, `.cbz`/`.zip`, and chapter URLs in `details.json` (or a separate `chapters.json`).

```text
<title-id>/
  details.json          # metadata + optional chapters[]
  cover.webp            # optional
  Chapter 001/          # folder of images
  Chapter 002.cbz       # zip/cbz, ranged (not downloaded whole)
```

JSON generator:

**[https://raahat-hossain.github.io/extensions/](https://raahat-hossain.github.io/extensions/)**

Source: [`web/index.html`](web/index.html).

Suwatte source list (directory URL, not `runners.json`):

```
https://raahat-hossain.github.io/extensions/suwatte
```

R2 Library on Suwatte is **2.0**. Update the list in-app after adding. Library browse stays on R2; Cloudflare Resolve runs when you open a gallery chapter (HentaiRead, etc.).

```json
{
  "title": "Example",
  "author": "Hyji",
  "status": "completed",
  "cover": "Chapter 1_1",
  "chaptersOverlay": true,
  "chapters": [
    { "url": "https://hentairead.com/hentai/example/", "title": "Prologue" },
    {
      "url": "https://novelcrow.com/comic/slug/",
      "chapterRange": "2-",
      "titles": { "2": "Start here", "5": "Climax" }
    },
    { "url": "https://allporncomic.com/porncomic/slug/", "chapterRange": "1-10" },
    { "title": "Chapter 4", "number": 4, "url": "https://nhentai.net/g/289857/", "pageRange": "3-50" }
  ]
}
```

Bucket zip/folder chapters are picked up automatically. JSON `chapters[]` are **sources in order**: a gallery URL is one chapter; a NovelCrow / AllPornComic / MangaDex series URL expands to **every** chapter on that title (optional `chapterRange`, blank = all, including chapters that show up later). Display numbers are then 1, 2, 3… so two sources that both have a “chapter 1” both appear. `"titles"` on a series row renames by **that site’s chapter numbers**. With `"chaptersOverlay": true`, the composed list replaces matching folder/cbz numbers. Omit the flag to keep JSON chapters in addition to folders. Gallery URLs can live in `details.json` → `chapters` (preferred) or a separate `chapters.json`.

Gallery hosts: nhentai, HentaiRead, HentaiNexus, Hentai2Read, PandaChaika, E-Hentai / ExHentai, Hitomi, NovelCrow, AllPornComic, MangaDex. Series URLs: `https://novelcrow.com/comic/slug/`, `https://allporncomic.com/porncomic/slug/`, `https://mangadex.org/title/{uuid}/…` (MangaDex prefers English when the same number exists in multiple languages). Madara sites may need a one-time Cloudflare solve in WebView.

`"chapterRange"` filters a series (`"2-"` = from 2 through newest, `"2-10"` = those chapters, `"5"` = only 5). `"pageRange"` crops pages in the reader: `"50"` = first 50 pages, `"3-50"` = pages 3–50, `"3-"` = page 3 through the end. Overlay-only `{ "number": 4, "pageRange": "1-50" }` slices a folder/cbz already on that number. If `details.json` title/cover/author are blank, the first series URL’s title and cover are filled in (local fields still win).

`details.json` `"cover"`: `https://…`, `"Chapter 1_1"` (chapter + 1-based page), `"chapter 4_24.png"`, or a relative image.

`.cbr` / `.rar` / `.pdf` are not readable.

Layout notes: [`examples/r2-merge-layout/README.md`](examples/r2-merge-layout/README.md)

## Install

TachiManga wants **index.pb**:

```
https://github.com/raahat-hossain/extensions/raw/cursor/r2-merge-extension-8f4a/repo/index.pb
```

Sideload: [`repo/apk/tachiyomi-all.r2merge-v1.4.12.apk`](repo/apk/tachiyomi-all.r2merge-v1.4.12.apk)

1.4.11 was signed with a throwaway debug cert, so TachiManga hides the update. Uninstall R2 Library, remove the repo, re-add `index.pb`, install **1.4.12**. Later updates keep this key.

Settings: Account ID, Access Key, Secret, Bucket. Root Prefix empty if titles sit at bucket root. Optional public image URL (r2.dev / custom domain).

## Build

Needs Android SDK 37 + a Yūzōnō checkout (`.ref/yuzono` or `YUZONO_DIR`).

```sh
bash scripts/build-r2-merge-apk.sh
```
