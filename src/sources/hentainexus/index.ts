"use httpclient";

import {
  ContentRating,
  PickerFilter,
  SearchFilter,
  TextFilter,
  type Chapter,
  type ChapterPage,
  type Content,
  type HomePage,
  type ItemListRequest,
  type PagedItemList,
  type SearchRequest,
  type SourceConfiguration,
  type SourceInfo,
} from "@suwatte/toolchain/types";
import { absoluteUrl, getText } from "../_shared/http";
import {
  BASE_URL,
  chapterForManga,
  imageFieldFor,
  pagesFromReaderHtml,
  parseMangaDetails,
  parseMangaList,
} from "./parse";

const PREFIX_ID_SEARCH = "id:";

const fetchHtml = (pathOrUrl: string): Promise<string> => {
  const url = pathOrUrl.startsWith("http")
    ? pathOrUrl
    : absoluteUrl(BASE_URL, pathOrUrl);
  return getText(url, {
    referer: `${BASE_URL}/`,
    headers: {
      Accept:
        "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8",
      "Accept-Language": "en-US,en;q=0.5",
    },
  });
};

const listUrl = (page: number, query?: string): string => {
  const base = page > 1 ? `${BASE_URL}/page/${page}` : `${BASE_URL}/`;
  if (!query?.trim()) return base;
  const sep = base.includes("?") ? "&" : "?";
  return `${base}${sep}q=${encodeURIComponent(query.trim())}`;
};

const extractId = (query: string): string | undefined => {
  const trimmed = query.trim();
  if (!trimmed) return undefined;
  if (trimmed.startsWith(PREFIX_ID_SEARCH)) {
    return trimmed.slice(PREFIX_ID_SEARCH.length).trim();
  }
  if (/^\d+$/.test(trimmed)) return trimmed;
  const match = trimmed.match(
    /^https?:\/\/(?:www\.)?hentainexus\.com\/view\/(\d+)/i,
  );
  return match?.[1];
};

export default class Target {
  static info: SourceInfo = {
    id: "en.hentainexus",
    name: "HentaiNexus",
    version: 1.2,
    website: BASE_URL,
    thumbnail: "hentainexus.png",
    languages: ["en"],
    rating: ContentRating.MATURE,
  };

  getConfiguration = (): SourceConfiguration => ({
    imageReferer: `${BASE_URL}/`,
  });

  getHomePage = async (): Promise<HomePage> => ({
    feeds: [
      {
        id: "popular",
        title: "Popular Now",
        content: { list: { key: "popular", disableSorting: true } },
      },
      {
        id: "latest",
        title: "Latest",
        content: { list: { key: "latest", disableSorting: true } },
      },
    ],
  });

  getSearchFilters = async () => [
    SearchFilter(
      "imageFormat",
      "Image Quality",
      PickerFilter([
        { id: "webp", title: "WebP" },
        { id: "avif", title: "AVIF" },
        { id: "source", title: "Original (login required)" },
      ]),
      "Original quality requires a user account.",
    ),
    SearchFilter(
      "tags",
      "Tags",
      TextFilter(),
      'Comma-separated. Prepend - to exclude. Quote multi-word values.',
    ),
    SearchFilter("artists", "Artists", TextFilter()),
    SearchFilter("authors", "Authors", TextFilter()),
    SearchFilter("circles", "Circles", TextFilter()),
    SearchFilter("parodies", "Parodies", TextFilter()),
  ];

  getSearchResults = async (
    request: SearchRequest,
    page: number,
  ): Promise<PagedItemList> => {
    const query = request.query?.trim() ?? "";
    const id = extractId(query);
    if (id) {
      if (page > 1) return { items: [], isLastPage: true, total: 0 };
      const html = await fetchHtml(`/view/${id}`);
      const { content } = parseMangaDetails(html, id);
      return {
        items: [
          {
            id,
            title: content.title,
            coverImage: content.coverImage,
            rating: ContentRating.MATURE,
          },
        ],
        isLastPage: true,
        total: 1,
      };
    }

    const filters = request.filters ?? {};
    const advParts: string[] = [];
    const appendAdv = (key: string, value: unknown) => {
      if (typeof value !== "string" || !value.trim()) return;
      // Split on commas outside quotes
      const tokens: string[] = [];
      let current = "";
      let inQuotes = false;
      for (const ch of value) {
        if (ch === '"') {
          inQuotes = !inQuotes;
          current += ch;
        } else if (ch === "," && !inQuotes) {
          const token = current.trim();
          if (token) tokens.push(token);
          current = "";
        } else {
          current += ch;
        }
      }
      const last = current.trim();
      if (last) tokens.push(last);

      for (const token of tokens) {
        const exclude = token.startsWith("-");
        const text = token.replace(/^-/, "");
        advParts.push(`${exclude ? "-" : ""}${key}:${text}`);
      }
    };

    appendAdv("tag", filters.tags);
    appendAdv("artist", filters.artists);
    appendAdv("author", filters.authors);
    appendAdv("circle", filters.circles);
    appendAdv("parody", filters.parodies);

    const q = [...advParts, query].filter(Boolean).join(" ").trim();
    const html = await fetchHtml(listUrl(page, q || undefined));
    const { items, hasNextPage } = parseMangaList(html);
    return { items, isLastPage: !hasNextPage || items.length === 0 };
  };

  getItemList = async (
    request: ItemListRequest,
    page: number,
  ): Promise<PagedItemList> => {
    // Keiyoushi: page 1 popular is /explore/hot; later pages fall back to sort:popular search.
    if (request.key === "popular") {
      if (page === 1) {
        const html = await fetchHtml("/explore/hot");
        const { items } = parseMangaList(html);
        return { items, isLastPage: false };
      }
      const html = await fetchHtml(listUrl(page - 1, "sort:popular"));
      const { items, hasNextPage } = parseMangaList(html);
      return { items, isLastPage: !hasNextPage || items.length === 0 };
    }

    const html = await fetchHtml(listUrl(page));
    const { items, hasNextPage } = parseMangaList(html);
    return { items, isLastPage: !hasNextPage || items.length === 0 };
  };

  getContent = async (contentId: string): Promise<Content> => {
    const html = await fetchHtml(`/view/${contentId}`);
    return parseMangaDetails(html, contentId).content;
  };

  getChapters = async (contentId: string): Promise<Chapter[]> => {
    const html = await fetchHtml(`/view/${contentId}`);
    const { published } = parseMangaDetails(html, contentId);
    return [chapterForManga(contentId, published)];
  };

  getChapterPages = async (
    contentId: string,
    chapterId: string,
    // image format can be passed via search filters only; default WebP
  ): Promise<ChapterPage[]> => {
    const id = chapterId || contentId;
    const html = await fetchHtml(`/read/${id}`);
    return pagesFromReaderHtml(html, imageFieldFor("webp")).map((url) => ({
      url,
    }));
  };
}
