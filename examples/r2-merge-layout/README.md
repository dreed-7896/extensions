# R2 bucket layout for R2 Library (Mihon / TachiManga)

One folder = one library title. Mix any of: image folders, `.cbz`/`.zip`, and chapter URLs in `details.json` (`chapters` array) or a separate `chapters.json`.

```text
Manga/                          # folder name = content id
  details.json                  # optional
  cover.webp                    # optional
  chapters.json                 # optional
  Chapter 001/                  # folder of images
    001.jpg
    002.jpg
  Chapter 002.cbz               # ranged zip — not downloaded whole
```

## chapters.json

Array, or `{ "chapters": [ ... ] }`. Can also live inside `details.json` as `"chapters": [ ... ]` (preferred — one file). Replacing that file is the update. Folder/cbz chapters in the same prefix still show alongside JSON if you leave them there.

JSON `chapters[]` are sources **in list order**. Any host can be first.

- A gallery URL is one chapter. A NovelCrow / AllPornComic / MangaDex series URL expands to every chapter on that title.
- Blank `chapterRange` = all chapters, including ones that appear later (`"2-"` = from 2 through newest).
- `"titles"` on a series row renames by **that source’s chapter numbers** (`"2"`, `"5"`, …).
- After expansion, display numbers are 1, 2, 3…

```json
{
  "chapters": [
    { "url": "https://hentairead.com/hentai/example/", "title": "Prologue" },
    {
      "url": "https://novelcrow.com/comic/slug/",
      "chapterRange": "2-",
      "titles": { "2": "Start here", "5": "Climax" }
    },
    {
      "url": "https://allporncomic.com/porncomic/slug/"
    }
  ]
}
```

```json
{
  "chapters": [
    "https://nhentai.net/g/289857/",
    {
      "title": "Chapter 2",
      "number": 2,
      "url": "https://hitomi.la/galleries/123456.html"
    },
    {
      "title": "Chapter 3",
      "number": 3,
      "url": "https://cdn.example.com/ch3.cbz"
    },
    {
      "title": "Chapter 4",
      "number": 4,
      "pages": ["https://cdn.example.com/4/001.jpg", "https://cdn.example.com/4/002.jpg"]
    }
  ]
}
```

| Field | Notes |
| --- | --- |
| `url` / `href` / `link` / `archive` / `file` | Gallery page, remote `.cbz`/`.zip`, or a path relative to the series folder |
| `source` / `site` / `host` | Optional: `nhentai`, `hentairead`, `hentainexus`, `hentai2read`, `pandachaika`, `ehentai`, `hitomi`, `novelcrow`, `allporncomic`, `mangadex` (aliases: `nh`, `hr`, `hn`, `h2r`, `chaika`, `eh`, `exhentai`, `nc`, `apc`, `md`). `zip`/`cbz` forces archive handling |
| `id` | Gallery/slug id if you skip the URL (`id` + `source`) |
| `title` / `number` / `date` / `scanlator` | Optional display fields. Series rows ignore `title`/`number` (use `titles` / `chapterRange`) |
| `chapterRange` | Series only. Blank = all (including new chapters later). `"2-"` = from 2 through newest, `"2-10"` = those chapters, `"5"` = only 5. On a series row, `range` is treated as `chapterRange` |
| `titles` | Series only. Map of **source** chapter number → name, e.g. `{ "2": "Start here" }` |
| `pages` | Raw image URLs — skips site/archive parsing |
| `pageRange` | Crop the reader. `"50"` = first 50, `"3-50"` = pages 3–50, `"3-"` = 3 through the end. Also `pageStart`/`pageEnd` |

### Gallery hosts

- `https://nhentai.net/g/<id>/`
- `https://hentairead.com/hentai/<slug>/`
- `https://hentainexus.com/view/<id>` or `/read/<id>`
- `https://hentai2read.com/<slug>/<chapter>/`
- `https://panda.chaika.moe/archive/<id>`
- `https://e-hentai.org/g/<id>/<token>/` (also `exhentai.org`)
- `https://hitomi.la/galleries/<id>.html`
- `https://novelcrow.com/comic/<slug>/` (series — expands to all chapters; optional `chapterRange` / `titles`)
- `https://novelcrow.com/comic/<slug>/<chapter>/` (single chapter)
- `https://allporncomic.com/porncomic/<slug>/` (series — same Madara ajax as NovelCrow; also `allporncomics.com`)
- `https://mangadex.org/title/<uuid>/` (series — expands; English preferred per chapter number)
- `https://mangadex.org/chapter/<uuid>` (single chapter)

### Archives and folders

- Chapter folders of jpg/png/webp/… (nested `Volume 1/Chapter 003` is fine)
- `.cbz` / `.zip` in the series folder (range requests)
- Remote `url` ending in `.cbz`/`.zip` (host must support `Range`)
- Images dumped in the series folder with no chapter dirs → one chapter
- `.cbr` / `.rar` / `.pdf` are not readable — convert to `.cbz`

`details.json` uses Tachiyomi local-source fields (`title`, `author`, `artist`, `description`, `genre`, `status`, `cover`). `ComicInfo.xml` in the series folder works as a fallback. Default rating is **mature** when omitted.

### Cover

Resolve order:

1. `details.json` → `"cover"`
2. `cover.(png|jpg|jpeg|webp|gif|avif)` in the title folder
3. First page of the first chapter

`cover` values:

```json
"cover": "https://cdn.example.com/front.jpg"
```

```json
"cover": "Chapter 1_1"
```

That is `chapterName_pageIndex` (1-based). `"Chapter 1_1"` uses the first page of the chapter titled `Chapter 1` — folder, `.cbz`, **or** a `chapters.json` gallery URL. Also matches `Chapter 001` / `001 - Chapter 1.cbz`.

Legacy filename form still works: `"chapter 4_24.png"` → page `24.png` inside that chapter.

Relative image keys work too: `"front.webp"` or `"art/cover.jpg"`.
