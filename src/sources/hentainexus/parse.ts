import { absoluteUrl } from "../_shared/http";
import { allMatches, attr, firstMatch, stripTags } from "../_shared/html";
import { itemFrom } from "../_shared/item";
import {
  ContentRating,
  ContentStatus,
  ContentType,
  type Chapter,
  type Content,
  type Credit,
  type Item,
  type Tag,
} from "@suwatte/toolchain/types";
import { decryptReaderPayload } from "./decrypt";

export const BASE_URL = "https://hentainexus.com";
export const DEFAULT_IMAGE_FIELD = "image_fallback";

export const imageFieldFor = (format: string): string => {
  switch (format) {
    case "source":
      return "image_source";
    case "avif":
      return "image_avif";
    default:
      return DEFAULT_IMAGE_FIELD;
  }
};

/** Decrypt `initReader("...")` payload and return image URLs. */
export const pagesFromReaderHtml = (
  html: string,
  imageField = DEFAULT_IMAGE_FIELD,
): string[] => {
  const encoded = firstMatch(html, /initReader\("([^"]+)"/);
  if (!encoded) {
    throw new Error(
      "Could not find initReader script; the page structure may have changed",
    );
  }

  const decrypted = decryptReaderPayload(encoded);
  const parsed = JSON.parse(decrypted) as unknown;
  if (!Array.isArray(parsed)) {
    throw new Error("Unexpected reader payload shape");
  }

  const images = parsed.filter(
    (entry): entry is Record<string, unknown> =>
      !!entry &&
      typeof entry === "object" &&
      (entry as Record<string, unknown>).type === "image",
  );

  if (!images.length) {
    throw new Error("No pages found for this chapter");
  }

  if (typeof images[0]![imageField] !== "string" || !images[0]![imageField]) {
    throw new Error(
      "Selected quality is not available. Login or select another quality.",
    );
  }

  return images
    .map((image) => image[imageField])
    .filter((value): value is string => typeof value === "string" && !!value)
    .map((url) => absoluteUrl(BASE_URL, url));
};

const TAG_COUNT_REGEX = /\s*\([\d,]+\)$/;

export type ParsedList = {
  items: Item[];
  hasNextPage: boolean;
};

const ownTextFromCell = (cellHtml: string): string => {
  const withoutNested = cellHtml.replace(/<[^>]+>/g, " ");
  return stripTags(withoutNested).replace(TAG_COUNT_REGEX, "").trim();
};

const tableCell = (html: string, label: string): string | undefined => {
  const pattern = new RegExp(
    `<td\\s+class="viewcolumn">\\s*${label}\\s*</td>\\s*<td[^>]*>([\\s\\S]*?)</td>`,
    "i",
  );
  return firstMatch(html, pattern);
};

const linksFromCell = (cellHtml: string): string[] =>
  allMatches(cellHtml, /<a[^>]*>([\s\S]*?)<\/a>/gi)
    .map((entry) => stripTags(entry).replace(TAG_COUNT_REGEX, "").trim())
    .filter(Boolean);

export const parseMangaList = (html: string): ParsedList => {
  const items: Item[] = [];
  const seen = new Set<string>();

  const columnPattern =
    /<div\s+class="column[^"]*"[^>]*>([\s\S]*?)(?=<div\s+class="column[^"]*"|<\/div>\s*<\/div>\s*<nav|\s*<\/div>\s*<\/section|$)/gi;

  let match: RegExpExecArray | null;
  const global = new RegExp(columnPattern.source, "gi");
  while ((match = global.exec(html))) {
    const block = match[1] ?? "";
    const href =
      firstMatch(block, /href="(\/view\/\d+)"/i) ??
      firstMatch(block, /href="(https?:\/\/[^"]*\/view\/\d+)"/i);
    if (!href) continue;
    const idMatch = href.match(/\/view\/(\d+)/i);
    if (!idMatch?.[1]) continue;
    const id = idMatch[1];
    if (seen.has(id)) continue;

    const title =
      stripTags(
        firstMatch(block, /class="card-header-title"[^>]*>([\s\S]*?)<\/p>/i) ??
          firstMatch(block, /title="([^"]+)"/i) ??
          "",
      ) || id;
    const imgTag =
      firstMatch(block, /(<img\b[^>]*>)/i) ??
      firstMatch(block, /(<img\b[^>]*\/>)/i) ??
      "";
    const src =
      (imgTag &&
        (attr(imgTag, "data-src") ||
          attr(imgTag, "data-lazy-src") ||
          attr(imgTag, "data-cfsrc") ||
          attr(imgTag, "src"))) ||
      firstMatch(block, /(?:data-src|data-lazy-src|src)="([^"]+)"/i) ||
      "";

    seen.add(id);
    items.push(
      itemFrom(id, title, src ? absoluteUrl(BASE_URL, src) : undefined),
    );
  }

  // Fallback when column regex is too strict
  if (!items.length) {
    const linkPattern = /href="(\/view\/(\d+))"[^>]*>[\s\S]*?class="card-header-title"[^>]*>([\s\S]*?)<\/p>/gi;
    let linkMatch: RegExpExecArray | null;
    while ((linkMatch = linkPattern.exec(html))) {
      const id = linkMatch[2]!;
      if (seen.has(id)) continue;
      seen.add(id);
      items.push(itemFrom(id, stripTags(linkMatch[3] ?? id)));
    }
  }

  const hasNextPage = /<a[^>]*class="[^"]*pagination-next[^"]*"[^>]*href="/i.test(
    html,
  );
  return { items, hasNextPage };
};

export const parseMangaDetails = (
  html: string,
  contentId: string,
): { content: Content; published?: string } => {
  const title =
    stripTags(firstMatch(html, /<h1[^>]*class="[^"]*title[^"]*"[^>]*>([\s\S]*?)<\/h1>/i) ?? "") ||
    contentId;

  const artists = linksFromCell(tableCell(html, "Artist") ?? "");
  const authors = linksFromCell(tableCell(html, "Author") ?? "");
  const credits: Credit[] = [...new Set([...authors, ...artists])].map(
    (name) => ({ name, role: artists.includes(name) ? "Artist" : "Author" }),
  );

  const detailKeys = [
    "Circle",
    "Event",
    "Magazine",
    "Parody",
    "Publisher",
    "Pages",
    "Favorites",
  ] as const;
  const additionalDetails: Record<string, string> = {};
  const summaryLines: string[] = [];
  for (const key of detailKeys) {
    const cell = tableCell(html, key);
    if (!cell) continue;
    const value =
      ownTextFromCell(cell) || linksFromCell(cell)[0] || "";
    if (!value) continue;
    additionalDetails[key] = value;
    summaryLines.push(`${key}: ${value}`);
  }

  const descriptionCell = tableCell(html, "Description");
  const descriptionBody = descriptionCell
    ? stripTags(descriptionCell)
    : "";
  const summary = [summaryLines.join("\n"), descriptionBody]
    .filter(Boolean)
    .join("\n\n")
    .trim();

  const tagsCell = tableCell(html, "Tags") ?? "";
  const genres: Tag[] = linksFromCell(tagsCell).map((name) => ({
    id: name.toLowerCase().replace(/\s+/g, "-"),
    title: name,
    rating: ContentRating.MATURE,
  }));

  const imgTag =
    firstMatch(
      html,
      /<figure[^>]*class="[^"]*image[^"]*"[^>]*>\s*(<img\b[^>]*>)/i,
    ) ?? "";
  const coverSrc =
    (imgTag &&
      (attr(imgTag, "data-src") ||
        attr(imgTag, "data-lazy-src") ||
        attr(imgTag, "src"))) ||
    firstMatch(html, /property="og:image"\s+content="([^"]+)"/i) ||
    "";

  const published = tableCell(html, "Published")
    ? ownTextFromCell(tableCell(html, "Published")!)
    : undefined;

  const favorites = Number(
    (additionalDetails.Favorites || "").replace(/,/g, ""),
  );

  return {
    content: {
      title,
      coverImage: coverSrc
        ? absoluteUrl(BASE_URL, coverSrc)
        : "https://via.placeholder.com/300x450?text=No+Cover",
      webUrl: `${BASE_URL}/view/${contentId}`,
      rating: ContentRating.MATURE,
      status: ContentStatus.COMPLETED,
      contentType: ContentType.COMIC,
      summary: summary || undefined,
      additionalDetails:
        Object.keys(additionalDetails).length > 0
          ? additionalDetails
          : undefined,
      statistics: Number.isFinite(favorites)
        ? { favorites }
        : undefined,
      credits: credits.length ? credits : undefined,
      genres: genres.length ? genres : undefined,
    },
    published,
  };
};

export const chapterForManga = (
  contentId: string,
  published?: string,
): Chapter => {
  let date: Date | undefined;
  if (published) {
    const parsed = Date.parse(published);
    if (!Number.isNaN(parsed)) date = new Date(parsed);
  }
  return {
    id: contentId,
    index: 0,
    number: 1,
    title: "Chapter",
    date,
    webUrl: `${BASE_URL}/read/${contentId}`,
    language: "en",
  };
};
