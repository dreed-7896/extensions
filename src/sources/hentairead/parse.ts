import type { ChapterPage, Content } from "@suwatte/toolchain/types";
import { ContentRating, ContentStatus } from "@suwatte/toolchain/types";
import {
  attr,
  base64Decode,
  collectBlocks,
  firstMatch,
  imageFromTag,
  metaContent,
  stripTags,
} from "../_shared/html";
import { absoluteUrl } from "../_shared/http";
import { matureItem } from "../_shared/item";
import { BASE, MANGA } from "./constants";

type PagesDto = {
  data?: { chapter?: { images?: Array<{ src?: string }> } };
};

/** Card root class token — not `manga-item__wrapper` / `group/manga-item`. */
const CARD_OPEN =
  /<div\b[^>]*class=["'](?:[^"']*\s)?manga-item(?:\s[^"']*)?["'][^>]*>/gi;

const TITLE_LINK =
  /<a\b([^>]*\bmanga-item__link\b[^>]*)>([\s\S]*?)<\/a>/i;

const COVER_IMG = /(<img\b[^>]*\bmanga-item__img-inner\b[^>]*>)/i;

const BASE64_JSON = /eyJ[A-Za-z0-9+/=]{16,}/g;

export const parseMangaId = (href: string): string => {
  const cleaned = href.replace(/[?#].*$/, "").replace(/\/+$/, "");
  const match = cleaned.match(/\/hentai\/([^/]+)/i);
  if (match?.[1]) return decodeURIComponent(match[1]);
  const last = cleaned.split("/").filter(Boolean).pop();
  return last ? decodeURIComponent(last) : cleaned;
};

export const listingUrl = (page: number, sortby: string): string => {
  const path =
    page <= 1 ? `${BASE}/${MANGA}/` : `${BASE}/${MANGA}/page/${page}/`;
  return `${path}?sortby=${encodeURIComponent(sortby)}`;
};

/** Keiyoushi always uses `/page/$page/` even for page 1. */
export const searchUrl = (page: number, pairs: [string, string][]): string => {
  const path = `${BASE}/page/${Math.max(1, page)}/`;
  if (!pairs.length) return path;
  const query = pairs
    .map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`)
    .join("&");
  return `${path}?${query}`;
};

export const contentUrl = (contentId: string): string =>
  `${BASE}/${MANGA}/${encodeURI(contentId).replace(/%2F/gi, "/")}/`;

export const readerUrl = (contentId: string, language = "english"): string => {
  const base = contentUrl(contentId);
  if (!language) return `${base}p/1/`;
  return `${base}${language}/p/1/`;
};

export const hasNextPage = (html: string): boolean =>
  /<a\b[^>]*rel=["']next["']/i.test(html) ||
  /class=["'][^"']*\bnextpostslink\b/i.test(html) ||
  /class=["'][^"']*\bnext\b[^"']*\bpage-numbers\b/i.test(html);

const isUsableImage = (src: string): boolean =>
  !!src &&
  !src.startsWith("data:") &&
  !/placeholder|blank\.|spacer|1x1|logo|spinner|icon|avatar/i.test(src);

/** Listing covers: original `src` on hencover.xyz, not the 350w srcset entry. */
const listingCover = (imgTag: string): string => {
  const src = (attr(imgTag, "src") ?? "").trim();
  if (isUsableImage(src)) return src;
  return imageFromTag(imgTag);
};

const itemFromCard = (card: string) => {
  const link = TITLE_LINK.exec(card);
  if (!link) return undefined;
  const tag = `<a ${link[1]}>`;
  const href = attr(tag, "href") ?? "";
  const title = stripTags(link[2] ?? "") || attr(tag, "title") || "";
  if (!href || !title) return undefined;
  const id = parseMangaId(href);
  if (!id || /\/page\/\d+/i.test(href)) return undefined;

  const imgTag =
    firstMatch(card, COVER_IMG) ?? firstMatch(card, /(<img\b[^>]*>)/i);
  const cover = imgTag ? listingCover(imgTag) : "";

  return matureItem({
    id,
    title,
    coverImage: cover ? absoluteUrl(BASE, cover) : undefined,
    webUrl: absoluteUrl(BASE, href),
  });
};

/**
 * Cards are `div.manga-item.loop-item`. Title is `a.manga-item__link` — not
 * the first `<a>` (genre badges come first). Covers: `img.manga-item__img-inner`.
 */
export const parseListing = (html: string) => {
  const items: ReturnType<typeof matureItem>[] = [];
  const seen = new Set<string>();

  const cards = collectBlocks(html, CARD_OPEN, "div");
  const chunks = cards.length
    ? cards
    : html.split(/(?=<div\b[^>]*class=["'](?:[^"']*\s)?manga-item(?:\s))/i);

  for (const chunk of chunks) {
    const item = itemFromCard(chunk);
    if (!item || seen.has(item.id)) continue;
    seen.add(item.id);
    items.push(item);
  }

  if (items.length) return items;

  // Last-ditch: every title link, cover = nearest img-inner before it.
  const linkRe = /<a\b([^>]*\bmanga-item__link\b[^>]*)>([\s\S]*?)<\/a>/gi;
  let match: RegExpExecArray | null;
  while ((match = linkRe.exec(html))) {
    const tag = `<a ${match[1]}>`;
    const href = attr(tag, "href") ?? "";
    const title = stripTags(match[2] ?? "") || attr(tag, "title") || "";
    if (!href || !title) continue;
    const id = parseMangaId(href);
    if (!id || seen.has(id) || /\/page\/\d+/i.test(href)) continue;
    seen.add(id);
    const before = html.slice(Math.max(0, match.index - 24_000), match.index);
    const imgs = before.match(/<img\b[^>]*\bmanga-item__img-inner\b[^>]*>/gi);
    const imgTag = imgs?.[imgs.length - 1];
    const cover = imgTag ? listingCover(imgTag) : "";
    items.push(
      matureItem({
        id,
        title,
        coverImage: cover ? absoluteUrl(BASE, cover) : undefined,
        webUrl: absoluteUrl(BASE, href),
      }),
    );
  }

  if (items.length) return items;

  // Markup drift: h3 > a[href*="/hentai/"] even without manga-item__link.
  const h3Re =
    /<h3\b[^>]*>\s*<a\b([^>]*href=["'][^"']*\/hentai\/[^"']+["'][^>]*)>([\s\S]*?)<\/a>/gi;
  while ((match = h3Re.exec(html))) {
    const tag = `<a ${match[1]}>`;
    const href = attr(tag, "href") ?? "";
    const title = stripTags(match[2] ?? "") || attr(tag, "title") || "";
    if (!href || !title) continue;
    const id = parseMangaId(href);
    if (!id || seen.has(id) || /\/page\/\d+/i.test(href)) continue;
    seen.add(id);
    const before = html.slice(Math.max(0, match.index - 24_000), match.index);
    const imgs = before.match(/<img\b[^>]*(?:manga-item__img-inner|src=)[^>]*>/gi);
    const imgTag = imgs?.[imgs.length - 1];
    const cover = imgTag ? listingCover(imgTag) : "";
    items.push(
      matureItem({
        id,
        title,
        coverImage: cover ? absoluteUrl(BASE, cover) : undefined,
        webUrl: absoluteUrl(BASE, href),
      }),
    );
  }

  return items;
};

const capitalizeEach = (value: string): string =>
  value
    .split(" ")
    .map((word) =>
      word ? word.charAt(0).toUpperCase() + word.slice(1) : word,
    )
    .join(" ");

const isIndexHref = (href: string): boolean =>
  /-index\/?$/i.test(href.replace(/\/+$/, ""));

/** First span text of taxonomy chips; skip *-index directory links. */
export const taxonomyNames = (html: string, path: string): string[] => {
  const pattern = new RegExp(
    `<a\\b([^>]*href=["'][^"']*${path}[^"']*["'][^>]*)>([\\s\\S]*?)<\\/a>`,
    "gi",
  );
  const names: string[] = [];
  const seen = new Set<string>();
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(html))) {
    const href = attr(`<a ${match[1]}>`, "href") ?? "";
    if (isIndexHref(href) || !href.includes(path)) continue;
    const span = firstMatch(match[2] ?? "", /<span\b[^>]*>([\s\S]*?)<\/span>/i);
    const name = stripTags(span ?? match[2] ?? "");
    if (!name || seen.has(name.toLowerCase())) continue;
    seen.add(name.toLowerCase());
    names.push(name);
  }
  return names;
};

export const parseLanguages = (html: string): string[] => {
  const slugs: string[] = [];
  const re = /href=["'][^"']*\/language\/([^/"']+)\/?[^"']*["']/gi;
  let match: RegExpExecArray | null;
  while ((match = re.exec(html))) {
    const slug = match[1]?.toLowerCase();
    if (slug && slug !== "language-index" && !slugs.includes(slug)) {
      slugs.push(slug);
    }
  }
  return slugs;
};

export const parseGalleryId = (html: string): string | undefined => {
  const clip = firstMatch(html, /data-clipboard-text=["']#(\d+)["']/i);
  if (clip) return clip;
  return firstMatch(html, /#\s*(\d{3,})/);
};

export const parseUploadedDate = (html: string): Date | undefined => {
  const raw = stripTags(
    firstMatch(
      html,
      /Uploaded:<\/div>\s*<div\b[^>]*>([\s\S]*?)<\/div>/i,
    ) ?? "",
  );
  if (!raw) return undefined;
  const date = new Date(raw);
  return Number.isNaN(date.getTime()) ? undefined : date;
};

const labeledValue = (html: string, label: string): string =>
  stripTags(
    firstMatch(
      html,
      new RegExp(
        `${label}:<\\/div>[\\s\\S]{0,400}?<span[^>]*text-gray-100[^>]*>([\\s\\S]*?)<\\/span>`,
        "i",
      ),
    ) ??
      firstMatch(
        html,
        new RegExp(`${label}:<\\/div>[\\s\\S]{0,400}?>(\\d[\\d.,KMB]*)`, "i"),
      ) ??
      "",
  );

export const parseDetails = (html: string, contentId: string): Content => {
  const url = contentUrl(contentId);
  const title =
    stripTags(
      firstMatch(
        html,
        /<div[^>]*class=["'][^"']*manga-titles[^"']*["'][^>]*>[\s\S]*?<h1\b[^>]*>([\s\S]*?)<\/h1>/i,
      ) ??
        firstMatch(html, /<h1\b[^>]*>([\s\S]*?)<\/h1>/i) ??
        "",
    ) || contentId;

  const artists = taxonomyNames(html, "/artist/");
  const circles = taxonomyNames(html, "/circle/");
  const genres = taxonomyNames(html, "/tag/");
  const categories = taxonomyNames(html, "/genre/");
  const characters = [
    ...taxonomyNames(html, "/characters/"),
    ...taxonomyNames(html, "/character/"),
  ].filter((name, index, all) => all.indexOf(name) === index);
  const parodies = taxonomyNames(html, "/parody/");
  const conventions = taxonomyNames(html, "/convention/");
  const scanlators = taxonomyNames(html, "/scanlator/");
  const languages = parseLanguages(html);
  const galleryId = parseGalleryId(html);
  const pagesCount = labeledValue(html, "Pages");
  const views = labeledValue(html, "Views");
  const year = taxonomyNames(html, "/release/")[0];
  const uploaded = parseUploadedDate(html);

  const altRaw =
    stripTags(
      firstMatch(
        html,
        /<div[^>]*class=["'][^"']*manga-titles[^"']*["'][^>]*>[\s\S]*?<h2\b[^>]*>([\s\S]*?)<\/h2>/i,
      ) ?? "",
    ) || "";
  const additionalTitles = altRaw
    ? altRaw
        .split("|")
        .map((t) => t.trim())
        .filter(Boolean)
    : [];

  const parts: string[] = [];
  if (characters.length) {
    parts.push(`Characters: ${capitalizeEach(characters.join(", "))}`);
  }
  if (parodies.length) {
    parts.push(`Parodies: ${capitalizeEach(parodies.join(", "))}`);
  }
  if (circles.length) {
    parts.push(`Circles: ${capitalizeEach(circles.join(", "))}`);
  }
  if (conventions.length) {
    parts.push(`Convention: ${capitalizeEach(conventions.join(", "))}`);
  }
  if (scanlators.length) {
    parts.push(`Scanlators: ${capitalizeEach(scanlators.join(", "))}`);
  }
  if (languages.length) {
    parts.push(`Language: ${languages.map(capitalizeEach).join(", ")}`);
  }
  if (additionalTitles.length) {
    parts.push(
      `Alternative Titles:\n${additionalTitles.map((t) => `- ${t}`).join("\n")}`,
    );
  }
  if (pagesCount) parts.push(`Pages: ${pagesCount}`);

  let cover =
    metaContent(html, "og:image") ||
    metaContent(html, "twitter:image") ||
    "";
  if (!cover) {
    const img =
      firstMatch(html, /(<img\b[^>]*fetchpriority=["']high["'][^>]*>)/i) ??
      firstMatch(html, /(<img\b[^>]*class=["'][^"']*manga-item__img-inner[^>]*>)/i);
    if (img) cover = listingCover(img);
  }

  const author = circles.join(", ") || artists.join(", ") || undefined;
  const artist = artists.join(", ") || circles.join(", ") || undefined;
  const additionalDetails: Record<string, string> = {
    ...(galleryId ? { Gallery: galleryId } : {}),
    ...(categories.length ? { Category: categories.join(", ") } : {}),
    ...(year ? { Year: year } : {}),
    ...(pagesCount ? { Pages: pagesCount } : {}),
    ...(views ? { Views: views } : {}),
  };
  const credits = [
    ...(author ? [{ name: author, role: "Author" }] : []),
    ...(artist && artist !== author
      ? [{ name: artist, role: "Artist" }]
      : []),
  ];

  return {
    title,
    coverImage: cover ? absoluteUrl(BASE, cover) : "",
    webUrl: url,
    rating: ContentRating.MATURE,
    status: ContentStatus.COMPLETED,
    summary: parts.join("\n\n") || undefined,
    additionalTitles: additionalTitles.length ? additionalTitles : undefined,
    additionalDetails: Object.keys(additionalDetails).length
      ? additionalDetails
      : undefined,
    genres: genres.length
      ? genres.map((g) => ({
          id: g.toLowerCase().replace(/\s+/g, "-"),
          title: g,
        }))
      : undefined,
    ...(characters.length
      ? { characters: characters.map((name) => ({ name })) }
      : {}),
    ...(credits.length ? { credits } : {}),
    context: {
      ...(scanlators.length ? { scanlator: scanlators.join(", ") } : {}),
      ...(languages.length ? { languages } : {}),
      ...(galleryId ? { galleryId } : {}),
      ...(pagesCount ? { pageCount: pagesCount } : {}),
      ...(uploaded ? { uploaded: uploaded.toISOString() } : {}),
    },
  };
};

export const parsePageRange = (
  query: string,
  minPages = 1,
  maxPages = 9999,
): [number, number] => {
  const num = Number.parseInt(query.replace(/\D/g, ""), 10);
  const limited = (n = num) => Math.min(maxPages, Math.max(minPages, n));
  if (Number.isNaN(num) || num < 0) return [minPages, maxPages];
  switch (query[0]) {
    case "<":
      return query[1] === "="
        ? [minPages, limited()]
        : [minPages, limited(Math.max(minPages, num - 1))];
    case ">":
      return [limited(query[1] === "=" ? num : num + 1), maxPages];
    case "=":
      if (query[1] === ">") return [limited(), maxPages];
      if (query[1] === "<") return [minPages, limited()];
      return [limited(), limited()];
    default:
      return [limited(), limited()];
  }
};

const parsePageBaseUrl = (html: string): string => {
  const extra =
    firstMatch(
      html,
      /<script\b[^>]*id=["']single-chapter-js-extra["'][^>]*>([\s\S]*?)<\/script>/i,
    ) ?? html;
  return (
    firstMatch(extra, /"baseUrl"\s*:\s*"([^"]+)"/) ??
    firstMatch(extra, /'baseUrl'\s*:\s*'([^']+)'/) ??
    ""
  );
};

const imagesFromDto = (dto: PagesDto, pageBaseUrl: string): string[] => {
  const images = dto.data?.chapter?.images ?? [];
  return images
    .map((image) => (image.src ?? "").trim())
    .filter(Boolean)
    .map((src) => {
      if (/^https?:\/\//i.test(src) || src.startsWith("//")) {
        return absoluteUrl(BASE, src);
      }
      if (!pageBaseUrl) return "";
      return `${pageBaseUrl.replace(/\/+$/, "")}/${src.replace(/^\/+/, "")}`;
    })
    .filter(Boolean);
};

const dtoFromBase64 = (blob: string): PagesDto | undefined => {
  try {
    const parsed = JSON.parse(base64Decode(blob)) as PagesDto;
    if (parsed?.data?.chapter?.images?.length) return parsed;
  } catch {
    return undefined;
  }
  return undefined;
};

/** Reader scripts: `#single-chapter-js-extra` + base64 JSON in `#single-chapter-js-before`. */
export const parseChapterPages = (html: string): ChapterPage[] => {
  const pageBaseUrl = parsePageBaseUrl(html);

  const before =
    firstMatch(
      html,
      /<script\b[^>]*id=["']single-chapter-js-before["'][^>]*>([\s\S]*?)<\/script>/i,
    ) ?? "";
  const blobs = [
    ...new Set([
      ...(before.match(BASE64_JSON) ?? []),
      ...(html.match(BASE64_JSON) ?? []),
    ]),
  ];
  for (const blob of blobs) {
    const dto = dtoFromBase64(blob);
    if (!dto) continue;
    const urls = imagesFromDto(dto, pageBaseUrl);
    if (urls.length) return urls.map((url) => ({ url }));
  }

  const urls: string[] = [];
  const imgRe =
    /<img\b[^>]*(?:wp-manga-chapter-img|reading-content)[^>]*>|<img\b[^>]*>/gi;
  let img: RegExpExecArray | null;
  const reading =
    firstMatch(
      html,
      /<div\b[^>]*class=["'][^"']*reading-content[^"']*["'][^>]*>([\s\S]*?)<\/div>/i,
    ) ?? html;
  while ((img = imgRe.exec(reading))) {
    const src = imageFromTag(img[0] ?? "");
    if (isUsableImage(src) && !/hencover\.xyz/i.test(src)) {
      urls.push(absoluteUrl(BASE, src));
    }
  }
  return [...new Set(urls)].map((url) => ({ url }));
};

export const readerLanguageOrder = (html: string): string[] => {
  const listed = parseLanguages(html);
  const order = [
    ...listed.filter((lang) => lang === "english"),
    "english",
    ...listed.filter((lang) => lang !== "english"),
    "",
  ];
  return order.filter((lang, index) => order.indexOf(lang) === index);
};
