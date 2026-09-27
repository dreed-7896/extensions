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
} from "@suwatte/toolchain/types";
import { matureItem } from "../_shared/item";
import { mapPool } from "../_shared/pool";
import { fetchText } from "../_shared/http";
import {
  getGalleryIdsForQuery,
  getGalleryIdsFromNozomi,
  pageByteRange,
} from "./binary";
import {
  HITOMI_BASE,
  hitomiHeaders,
  LTN_URL,
  resolveImageUrl,
} from "./gg";

const PAGE_SIZE = 25;

type ImageFile = {
  hash: string;
  name: string;
  hasavif?: number;
};

type Named = { artist?: string; group?: string; character?: string; parody?: string; tag?: string; female?: string | number; male?: string | number };

type Gallery = {
  id?: string | number;
  galleryurl: string;
  title: string;
  japanese_title?: string;
  japaneseTitle?: string;
  date: string;
  type?: string;
  language?: string;
  tags?: Named[];
  artists?: Named[];
  groups?: Named[];
  characters?: Named[];
  parodys?: Named[];
  files: ImageFile[];
};

const toCamelCase = (value: string): string => {
  let capitalize = true;
  let result = "";
  for (const char of value) {
    result += capitalize ? char.toUpperCase() : char.toLowerCase();
    capitalize = char === " " || char === "\t";
  }
  return result;
};

const formatTag = (tag: Named): string => {
  const name = toCamelCase(tag.tag ?? "");
  if (String(tag.female) === "1") return `${name} ♀`;
  if (String(tag.male) === "1") return `${name} ♂`;
  return name;
};

const galleryIdFromUrl = (url: string): string => {
  const match = /(?:^|\/)(?:galleries\/)?(\d+)(?:\.js)?$/.exec(url);
  if (match) return match[1]!;
  const fromHtml = url
    .substring(url.lastIndexOf("-") + 1)
    .replace(/\.html?$/, "")
    .replace(/\/+$/, "");
  return fromHtml;
};

const parseGalleryScript = (body: string): Gallery => {
  const json = body.replace(/^[\s\S]*?var galleryinfo\s*=\s*/, "").trim();
  return JSON.parse(json) as Gallery;
};

const fetchGallery = async (id: string): Promise<Gallery> => {
  const body = await fetchText(`${LTN_URL}/galleries/${id}.js`, {
    headers: hitomiHeaders,
  });
  return parseGalleryScript(body);
};

const isGifFile = (file: ImageFile): boolean =>
  file.name.toLowerCase().endsWith(".gif") ||
  file.name.toLowerCase().endsWith(".webp");

const galleryToItem = async (gallery: Gallery): Promise<Item> => {
  const id = String(gallery.id ?? galleryIdFromUrl(gallery.galleryurl));
  const cover =
    gallery.files[0] != null
      ? await resolveImageUrl(gallery.files[0].hash, {
          thumbnail: true,
          isGif: isGifFile(gallery.files[0]),
        })
      : undefined;
  return matureItem({
    id,
    title: gallery.title,
    coverImage: cover,
    subtitle: gallery.type ?? gallery.language,
  });
};

const idsToItems = async (ids: number[]): Promise<Item[]> => {
  const settled = await Promise.all(
    ids.map(async (id) => {
      try {
        const gallery = await fetchGallery(String(id));
        return await galleryToItem(gallery);
      } catch {
        return null;
      }
    }),
  );
  return settled.filter((item): item is Item => item != null);
};

const listFromNozomi = async (
  area: string | null,
  tag: string,
  page: number,
): Promise<PagedItemList> => {
  const range = pageByteRange(page, PAGE_SIZE);
  const ids = await getGalleryIdsFromNozomi(area, tag, "all", range);
  const items = await idsToItems(ids);
  return {
    items,
    isLastPage: ids.length < PAGE_SIZE,
  };
};

const intersect = (left: Set<number>, right: number[]): Set<number> => {
  if (!left.size) return new Set(right);
  const next = new Set<number>();
  for (const id of right) {
    if (left.has(id)) next.add(id);
  }
  return next;
};

const hitomiSearch = async (query: string): Promise<number[]> => {
  const terms = query
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);

  if (!terms.length) {
    return getGalleryIdsFromNozomi(null, "index", "all");
  }

  const positive: string[] = [];
  const negative: string[] = [];
  for (const term of terms) {
    if (term.startsWith("-") && term.length > 1) {
      negative.push(term.slice(1));
    } else {
      positive.push(term);
    }
  }

  let results = new Set<number>();
  if (!positive.length) {
    results = new Set(await getGalleryIdsFromNozomi(null, "index", "all"));
  }

  for (const term of positive) {
    const ids = await getGalleryIdsForQuery(term, "all");
    results = intersect(results, ids);
  }
  for (const term of negative) {
    const ids = await getGalleryIdsForQuery(term, "all");
    for (const id of ids) results.delete(id);
  }

  return [...results];
};

// Cached search result pages (Tachiyomi keeps searchResponse in memory).
let cachedSearchQuery = "";
let cachedSearchIds: number[] = [];

export default class Target {
  static info: SourceInfo = {
    id: "all.hitomi",
    name: "Hitomi",
    version: 1.3,
    website: HITOMI_BASE,
    thumbnail: "hitomi.png",
    languages: ["all"],
    rating: ContentRating.MATURE,
  };

  getConfiguration = (): SourceConfiguration => ({
    imageReferer: `${HITOMI_BASE}/`,
  });

  getHomePage = async (): Promise<HomePage> => ({
    feeds: [
      {
        id: "recent",
        title: "Recent",
        content: {
          list: { key: "recent", disableSorting: true },
        },
      },
      {
        id: "popular",
        title: "Popular (Year)",
        content: {
          list: { key: "popular", disableSorting: true },
        },
      },
    ],
  });

  getSearchResults = async (
    request: SearchRequest,
    page: number,
  ): Promise<PagedItemList> => {
    const query = request.query?.trim() ?? "";

    if (!query) {
      return listFromNozomi(null, "index", page);
    }

    // Direct gallery id
    if (/^\d+$/.test(query)) {
      if (page > 1) return { items: [], isLastPage: true };
      try {
        const gallery = await fetchGallery(query);
        return { items: [await galleryToItem(gallery)], isLastPage: true };
      } catch {
        return { items: [], isLastPage: true };
      }
    }

    if (page === 1 || cachedSearchQuery !== query) {
      cachedSearchQuery = query;
      cachedSearchIds = await hitomiSearch(query);
    }

    const start = (page - 1) * PAGE_SIZE;
    const slice = cachedSearchIds.slice(start, start + PAGE_SIZE);
    const items = await idsToItems(slice);
    return {
      items,
      isLastPage: start + PAGE_SIZE >= cachedSearchIds.length,
    };
  };

  getItemList = async (
    request: ItemListRequest,
    page: number,
  ): Promise<PagedItemList> => {
    switch (request.key) {
      case "popular":
        return listFromNozomi("popular", "year", page);
      case "recent":
      default:
        return listFromNozomi(null, "index", page);
    }
  };

  getContent = async (contentId: string): Promise<Content> => {
    const gallery = await fetchGallery(contentId);
    const cover =
      gallery.files[0] != null
        ? await resolveImageUrl(gallery.files[0].hash, {
            thumbnail: true,
            isGif: isGifFile(gallery.files[0]),
          })
        : "https://via.placeholder.com/300x450?text=Hitomi";

    const artists = (gallery.artists ?? [])
      .map((entry) => toCamelCase(entry.artist ?? ""))
      .filter(Boolean);
    const groups = (gallery.groups ?? [])
      .map((entry) => toCamelCase(entry.group ?? ""))
      .filter(Boolean);
    const tags = (gallery.tags ?? []).map(formatTag).filter(Boolean);
    const japanese = gallery.japanese_title ?? gallery.japaneseTitle;
    const parodies = (gallery.parodys ?? [])
      .map((entry) => toCamelCase(entry.parody ?? ""))
      .filter(Boolean);
    const characters = (gallery.characters ?? [])
      .map((entry) => toCamelCase(entry.character ?? ""))
      .filter(Boolean);

    const summary = [
      japanese ? `Japanese title: ${japanese}` : "",
      parodies.length ? `Series: ${parodies.join(", ")}` : "",
      characters.length ? `Characters: ${characters.join(", ")}` : "",
      gallery.type ? `Type: ${gallery.type}` : "",
      `Pages: ${gallery.files.length}`,
      gallery.language ? `Language: ${gallery.language}` : "",
      artists.length ? `Artists: ${artists.join(", ")}` : "",
      groups.length ? `Groups: ${groups.join(", ")}` : "",
    ]
      .filter(Boolean)
      .join("\n");

    return {
      title: gallery.title,
      coverImage: cover,
      webUrl: `${HITOMI_BASE}${gallery.galleryurl}`,
      rating: ContentRating.MATURE,
      status: ContentStatus.COMPLETED,
      contentType: ContentType.COMIC,
      summary,
      additionalTitles: japanese ? [japanese] : undefined,
      genres: tags.length
        ? tags.map((title) => ({
            id: title.toLowerCase().replace(/\s+/g, "-"),
            title,
          }))
        : undefined,
      additionalDetails:
        artists.length || groups.length
          ? {
              ...(artists.length ? { Artists: artists.join(", ") } : {}),
              ...(groups.length ? { Groups: groups.join(", ") } : {}),
            }
          : undefined,
    };
  };

  getChapters = async (contentId: string): Promise<Chapter[]> => {
    const gallery = await fetchGallery(contentId);
    const datePart = gallery.date.substring(0, 19).replace(" ", "T");
    const parsed = new Date(datePart);
    return [
      {
        id: "chapter",
        index: 0,
        number: 1,
        title: "Chapter",
        language: gallery.language ?? "all",
        date: Number.isNaN(parsed.getTime()) ? new Date() : parsed,
        webUrl: `${HITOMI_BASE}${gallery.galleryurl}`,
      },
    ];
  };

  getChapterPages = async (
    contentId: string,
    _chapterId: string,
  ): Promise<ChapterPage[]> => {
    const gallery = await fetchGallery(contentId);
    if (!gallery.files.length) return [];
    // Warm gg.js once, then resolve hashes concurrently.
    await resolveImageUrl(gallery.files[0]!.hash, {
      isGif: isGifFile(gallery.files[0]!),
    });
    return mapPool(gallery.files, 12, async (file) => ({
      url: await resolveImageUrl(file.hash, { isGif: isGifFile(file) }),
    }));
  };
}
