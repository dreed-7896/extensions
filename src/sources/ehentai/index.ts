"use httpclient";

import {
  ContentRating,
  ContentStatus,
  ContentType,
  type Chapter,
  type ChapterPage,
  type Content,
  type HomePage,
  type Item,
  type ItemListRequest,
  type PagedItemList,
  type SearchRequest,
  type SourceConfiguration,
  type SourceInfo,
  type Tag,
} from "@suwatte/toolchain/types";
import { allMatches, attr, decodeEntities, firstMatch, stripTags } from "../_shared/html";
import { absoluteUrl, fetchText, withQuery } from "../_shared/http";
import { matureItem } from "../_shared/item";
import { mapPool } from "../_shared/pool";

const BASE_URL = "https://e-hentai.org";
const COOKIE = "nw=1; uconfig=prn_n";
const HEADERS = {
  Cookie: COOKIE,
  "User-Agent":
    "Mozilla/5.0 (compatible; Suwatte/1.0; +https://suwatte.mantton.com)",
};

/** Cursor for EH `next=` pagination (last gallery id on previous page). */
let lastMangaId = "";

const normalizeGalleryPath = (href: string): string => {
  const absolute = absoluteUrl(BASE_URL, href);
  const match = /\/g\/(\d+)\/([0-9a-f]+)/i.exec(absolute);
  if (!match) throw new Error(`Unsupported E-Hentai url: ${href}`);
  return `/g/${match[1]}/${match[2]}/?nw=always`;
};

const galleryIdFromPath = (path: string): string => {
  const match = /\/g\/(\d+)\//.exec(path);
  return match?.[1] ?? path;
};

const contentIdFromPath = (path: string): string => {
  const match = /\/g\/(\d+)\/([0-9a-f]+)/i.exec(path);
  if (!match) return path.replace(/^\/+|\/+$/g, "");
  return `${match[1]}/${match[2]}`;
};

const pathFromContentId = (contentId: string): string => {
  if (contentId.includes("/")) {
    const [id, token] = contentId.split("/");
    return `/g/${id}/${token}/?nw=always`;
  }
  return `/g/${contentId}/?nw=always`;
};

const parseListing = (html: string): { items: Item[]; isLastPage: boolean } => {
  const items: Item[] = [];
  const rowPattern =
    /<td[^>]*class="[^"]*glname[^"]*"[^>]*>([\s\S]*?)<\/td>/gi;
  let match: RegExpExecArray | null;
  const rows: string[] = [];
  while ((match = rowPattern.exec(html))) {
    rows.push(match[1] ?? "");
  }

  // Pair each glname cell with its parent row for thumbnails.
  const tableChunks =
    html.match(/<tr[^>]*>[\s\S]*?class="[^"]*glname[^"]*"[\s\S]*?<\/tr>/gi) ??
    [];

  const sources = tableChunks.length ? tableChunks : rows;

  for (let i = 0; i < sources.length; i += 1) {
    const chunk = sources[i]!;
    const link = firstMatch(chunk, /<a[^>]+href="([^"]*\/g\/[^"]+)"/i);
    const title = stripTags(
      firstMatch(chunk, /class="glink"[^>]*>([\s\S]*?)<\/span>/i) ??
        firstMatch(chunk, /class="glink"[^>]*>([\s\S]*?)<\/a>/i) ??
        "",
    );
    if (!link || !title) continue;

    const path = normalizeGalleryPath(link);
    const id = contentIdFromPath(path);
    const cover =
      firstMatch(chunk, /data-src="(https?:\/\/[^"]+)"/i) ??
      firstMatch(chunk, /src="(https?:\/\/[^"]+)"/i);

    items.push(
      matureItem({
        id,
        title,
        coverImage: cover,
      }),
    );

    if (i === sources.length - 1) {
      lastMangaId = galleryIdFromPath(path);
    }
  }

  const hasNext = /id="unext"[^>]*href="/i.test(html);
  return { items, isLastPage: !hasNext || items.length === 0 };
};

const listingUrl = (base: string, page: number): string => {
  if (page <= 1) return base;
  return withQuery(base, { next: lastMangaId || undefined });
};

const fetchListing = async (url: string): Promise<PagedItemList> => {
  const html = await fetchText(url, { headers: HEADERS, referer: BASE_URL });
  const parsed = parseListing(html);
  return { items: parsed.items, isLastPage: parsed.isLastPage };
};

const extractBackgroundUrl = (style: string): string | undefined => {
  const match = /url\((['"]?)(.*?)\1\)/i.exec(style);
  return match?.[2];
};

const parseTags = (html: string): Tag[] => {
  const tags: Tag[] = [];
  const rowPattern = /<tr>([\s\S]*?)<\/tr>/gi;
  const taglist =
    firstMatch(html, /id="taglist"[\s\S]*?<table[^>]*>([\s\S]*?)<\/table>/i) ??
    "";
  let match: RegExpExecArray | null;
  while ((match = rowPattern.exec(taglist))) {
    const row = match[1] ?? "";
    const namespace = stripTags(
      firstMatch(row, /class="tc"[^>]*>([\s\S]*?)<\/td>/i) ?? "",
    ).replace(/:$/, "");
    const names = allMatches(row, /<div[^>]*>([\s\S]*?)<\/div>/gi).map(stripTags);
    for (const name of names) {
      if (!name) continue;
      const title = namespace ? `${namespace}:${name}` : name;
      tags.push({
        id: title.toLowerCase().replace(/\s+/g, "-"),
        title,
      });
    }
  }
  return tags;
};

const collectPageLinks = async (galleryPath: string): Promise<string[]> => {
  const urls: string[] = [];
  let next: string | null = absoluteUrl(BASE_URL, galleryPath);

  while (next) {
    const html = await fetchText(next, { headers: HEADERS, referer: BASE_URL });
    const gdt =
      firstMatch(html, /id="gdt"[^>]*>([\s\S]*?)(?:<table|<div id="c)/i) ??
      html;
    const pageHrefs = allMatches(
      gdt,
      /<a[^>]+href="([^"]+\/s\/[^"]+)"/gi,
    ).map((href) => absoluteUrl(BASE_URL, decodeEntities(href)));

    for (const href of pageHrefs) {
      if (!urls.includes(href)) urls.push(href);
    }

    const navLinks =
      html.match(
        /<a[^>]*onclick="return false"[^>]*href="([^"]*)"[^>]*>([^<]*)<\/a>/gi,
      ) ?? [];
    let following: string | null = null;
    for (const tag of navLinks) {
      const href = attr(tag, "href");
      const label = stripTags(tag);
      if (label === ">" && href) {
        following = absoluteUrl(BASE_URL, href);
        break;
      }
    }
    next = following;
  }

  return urls;
};

const resolveImageUrl = async (pageUrl: string): Promise<string> => {
  const html = await fetchText(pageUrl, { headers: HEADERS, referer: BASE_URL });
  const img =
    firstMatch(html, /id="img"[^>]*src="([^"]+)"/i) ??
    firstMatch(html, /id="img"[^>]*data-src="([^"]+)"/i);
  if (!img) throw new Error(`No image on page ${pageUrl}`);
  return decodeEntities(img);
};

export default class Target {
  static info: SourceInfo = {
    id: "all.ehentai",
    name: "E-Hentai",
    version: 1.2,
    website: BASE_URL,
    thumbnail: "ehentai.png",
    languages: ["all"],
    rating: ContentRating.MATURE,
  };

  getConfiguration = (): SourceConfiguration => ({
    imageReferer: `${BASE_URL}/`,
  });

  getHomePage = async (): Promise<HomePage> => ({
    feeds: [
      {
        id: "popular",
        title: "Popular",
        content: {
          list: { key: "popular", disableSorting: true },
        },
      },
      {
        id: "latest",
        title: "Latest",
        content: {
          list: { key: "latest", disableSorting: true },
        },
      },
    ],
  });

  getSearchResults = async (
    request: SearchRequest,
    page: number,
  ): Promise<PagedItemList> => {
    const query = request.query?.trim() ?? "";

    // id:123456/abcdef or full URL
    const idMatch =
      /^(?:id:|https?:\/\/(?:e-|ex)hentai\.org\/g\/)?(\d+)\/([0-9a-f]+)\/?/i.exec(
        query,
      );
    if (idMatch) {
      if (page > 1) return { items: [], isLastPage: true };
      const contentId = `${idMatch[1]}/${idMatch[2]}`;
      const content = await this.getContent(contentId);
      return {
        items: [
          matureItem({
            id: contentId,
            title: content.title,
            coverImage: content.coverImage,
          }),
        ],
        isLastPage: true,
      };
    }

    if (page === 1) lastMangaId = "";

    const searchBase = withQuery(`${BASE_URL}/`, {
      f_apply: "Apply Filter",
      f_search: query || undefined,
    });
    return fetchListing(listingUrl(searchBase, page));
  };

  getItemList = async (
    request: ItemListRequest,
    page: number,
  ): Promise<PagedItemList> => {
    if (page === 1) lastMangaId = "";

    if (request.key === "popular") {
      const url = withQuery(`${BASE_URL}/`, {
        f_srdd: "5",
        f_sr: "on",
      });
      return fetchListing(listingUrl(url, page));
    }

    return fetchListing(listingUrl(BASE_URL, page));
  };

  getContent = async (contentId: string): Promise<Content> => {
    const path = pathFromContentId(contentId);
    const url = absoluteUrl(BASE_URL, path);
    const html = await fetchText(url, { headers: HEADERS, referer: BASE_URL });

    const title =
      stripTags(firstMatch(html, /id="gn"[^>]*>([\s\S]*?)<\/h1>/i) ?? "") ||
      contentId;
    const altTitle = stripTags(
      firstMatch(html, /id="gj"[^>]*>([\s\S]*?)<\/h1>/i) ?? "",
    );
    const coverStyle =
      firstMatch(
        html,
        /id="gd1"[^>]*>[\s\S]*?style="([^"]+)"/i,
      ) ?? "";
    const coverImage =
      extractBackgroundUrl(coverStyle) ??
      "https://via.placeholder.com/300x450?text=E-Hentai";

    const category = stripTags(
      firstMatch(html, /id="gdc"[\s\S]*?<div[^>]*>([\s\S]*?)<\/div>/i) ?? "",
    );
    const uploader = stripTags(
      firstMatch(html, /id="gdn"[^>]*>([\s\S]*?)<\/a>/i) ??
        firstMatch(html, /id="gdn"[^>]*>([\s\S]*?)<\/div>/i) ??
        "",
    );

    const details: Record<string, string> = {};
    const detailRows =
      firstMatch(html, /id="gdd"[\s\S]*?<table[^>]*>([\s\S]*?)<\/table>/i) ??
      "";
    const rowPattern = /<tr>([\s\S]*?)<\/tr>/gi;
    let rowMatch: RegExpExecArray | null;
    while ((rowMatch = rowPattern.exec(detailRows))) {
      const row = rowMatch[1] ?? "";
      const left = stripTags(
        firstMatch(row, /class="gdt1"[^>]*>([\s\S]*?)<\/td>/i) ?? "",
      )
        .replace(/:$/, "")
        .toLowerCase();
      const right = stripTags(
        firstMatch(row, /class="gdt2"[^>]*>([\s\S]*?)<\/td>/i) ?? "",
      );
      if (left && right) details[left] = right;
    }

    const averageRating = stripTags(
      firstMatch(html, /id="rating_label"[^>]*>([\s\S]*?)<\/td>/i) ?? "",
    )
      .replace(/^Average:\s*/i, "")
      .trim();
    const ratingCount = stripTags(
      firstMatch(html, /id="rating_count"[^>]*>([\s\S]*?)<\/span>/i) ?? "",
    );

    const genres = parseTags(html);
    const summary = [
      altTitle ? `Alternate Title: ${altTitle}` : "",
      uploader ? `Uploader: ${uploader}` : "",
      details.posted ? `Posted: ${details.posted}` : "",
      category ? `Category: ${category}` : "",
      details.language ? `Language: ${details.language}` : "",
      details["file size"] ? `File Size: ${details["file size"]}` : "",
      details.length ? `Length: ${details.length}` : "",
      averageRating
        ? `Rating: ${averageRating}${ratingCount ? ` (${ratingCount})` : ""}`
        : "",
    ]
      .filter(Boolean)
      .join("\n");

    return {
      title,
      coverImage,
      webUrl: url,
      rating: ContentRating.MATURE,
      status: /\[ongoing\]|\(ongoing\)|\{ongoing\}/i.test(title)
        ? ContentStatus.ONGOING
        : ContentStatus.COMPLETED,
      contentType: ContentType.COMIC,
      summary,
      additionalTitles: altTitle ? [altTitle] : undefined,
      genres: genres.length ? genres : undefined,
      statistics: averageRating
        ? {
            rating: Number.parseFloat(averageRating) || undefined,
          }
        : undefined,
      additionalDetails: {
        ...(category ? { Category: category } : {}),
        ...(uploader ? { Uploader: uploader } : {}),
      },
    };
  };

  getChapters = async (contentId: string): Promise<Chapter[]> => [
    {
      id: "chapter",
      index: 0,
      number: 1,
      title: "Chapter",
      language: "all",
      date: new Date(),
      webUrl: absoluteUrl(BASE_URL, pathFromContentId(contentId)),
    },
  ];

  getChapterPages = async (
    contentId: string,
    _chapterId: string,
  ): Promise<ChapterPage[]> => {
    const path = pathFromContentId(contentId);
    const pageUrls = await collectPageLinks(path);
    // Parallelize image-url resolution (was sequential N+1).
    const pages = (
      await mapPool(pageUrls, 6, async (pageUrl) => {
        try {
          return { url: await resolveImageUrl(pageUrl) };
        } catch {
          return null;
        }
      })
    ).filter((page): page is { url: string } => !!page);

    if (!pages.length) {
      throw new Error(`No readable pages for ${contentId}`);
    }
    return pages;
  };
}
