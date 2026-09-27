"use httpclient";

import {
  SearchFilter,
  SelectFilter,
  TextFilter,
  UIWebViewButton,
  type Chapter,
  type ChapterPage,
  type Content,
  type HomePage,
  type ItemListRequest,
  type NetworkRequest,
  type PagedItemList,
  type SearchRequest,
  type SortOptions,
  type SourceConfiguration,
  type SourceInfo,
  type UIForm,
} from "@suwatte/toolchain/types";
import { ContentRating } from "@suwatte/toolchain/types";
import { createProtectedClient } from "../_shared/client";
import {
  cloudflareFromHeaders,
  looksLikeCloudflare,
  throwCloudflare,
} from "../_shared/cloudflare";
import { withQuery } from "../_shared/http";
import {
  BASE,
  CF_RESOLVE,
  IMAGE_ACCEPT,
  SORTS,
  TEXT_FILTERS,
  TYPES,
} from "./constants";
import {
  contentUrl,
  hasNextPage,
  listingUrl,
  parseChapterPages,
  parseDetails,
  parsePageRange,
  parseListing,
  readerLanguageOrder,
  readerUrl,
  searchUrl,
} from "./parse";

type TermResult = { results?: Array<{ id: number; text: string }> };

const TAXONOMY: Record<string, string> = {
  artist: "manga_artist",
  manga_tag: "manga_tag",
};

const isImageRequest = (url: string): boolean =>
  /hencover\.|henread\.|\/wp-content\/uploads\//i.test(url) ||
  /\.(jpe?g|png|webp|avif|gif)(\?|$)/i.test(url);

const isCloudflareError = (error: unknown): boolean => {
  const name = String((error as { name?: string })?.name ?? "");
  const message = String((error as { message?: string })?.message ?? error);
  return (
    name.includes("Cloudflare") ||
    message.includes("Cloudflare") ||
    message.includes("cloudflare")
  );
};

const cloudflareFromThrown = (error: unknown): boolean => {
  if (isCloudflareError(error)) return true;
  const response = (error as { response?: { status?: number; headers?: unknown } })
    .response;
  if (!response) return false;
  return cloudflareFromHeaders(response.status ?? 0, response.headers as never);
};

/**
 * Documented CF path: https://suwatte.app/developers/networking/
 * `"use httpclient"` + owned HttpClient + cloudflareResolutionURL.
 * Native client throws CloudflareError; cookies attach to this client after
 * Resolve. Do not swallow non-2xx with a status validator — that skips native
 * CF throws.
 *
 * Keiyoushi has no CF code — Mihon's interceptor is app-side. Same split here.
 * Resolve on the same origin as the native listing request.
 */
export default class Target {
  client = (() => {
    const http = createProtectedClient(CF_RESOLVE, { Referer: `${BASE}/` });
    http.interceptors.request.use((request) => {
      if (isImageRequest(request.url)) {
        request.headers.set("Accept", IMAGE_ACCEPT);
        request.headers.set("Referer", `${BASE}/`);
      }
      return request;
    });
    return http;
  })();

  static info: SourceInfo = {
    id: "en.hentairead",
    name: "HentaiRead",
    version: 6,
    website: BASE,
    thumbnail: "hentairead.png",
    languages: ["en"],
    rating: ContentRating.MATURE,
  };

  getConfiguration = (): SourceConfiguration =>
    ({
      imageReferer: `${BASE}/`,
      cloudflareResolutionURL: CF_RESOLVE,
      useClientForImageRequests: true,
    }) as SourceConfiguration;

  getSettingsPage = async (): Promise<UIForm> => ({
    sections: [
      {
        header: "Cloudflare",
        footer:
          "Complete the website check, then refresh the source. If the check repeats, open the challenge page here.",
        views: [
          UIWebViewButton({
            title: "Open Challenge Page",
            url: { url: CF_RESOLVE },
          }),
        ],
      },
    ],
  });

  willRequestImage = async (
    request: NetworkRequest,
  ): Promise<NetworkRequest> => ({
    ...request,
    headers: {
      ...(request.headers ?? {}),
      Accept: IMAGE_ACCEPT,
      Referer: `${BASE}/`,
    },
  });

  private getHtml = async (
    url: string,
    referer = `${BASE}/`,
    extraHeaders: Record<string, string> = {},
  ): Promise<string> => {
    try {
      const response = await this.client.get(url, {
        headers: { Referer: referer, ...extraHeaders },
      });
      if (cloudflareFromHeaders(response.status, response.headers)) {
        throwCloudflare(CF_RESOLVE);
      }
      const body = await response.text();
      if (looksLikeCloudflare(body)) throwCloudflare(CF_RESOLVE);
      if (!response.ok) {
        throw new Error(`GET ${url} failed (${response.status})`);
      }
      return body;
    } catch (error) {
      if (cloudflareFromThrown(error)) throwCloudflare(CF_RESOLVE);
      const response = (
        error as {
          response?: {
            status?: number;
            headers?: unknown;
            text?: () => Promise<string>;
          };
        }
      ).response;
      if (response) {
        const body = (await response.text?.().catch(() => "")) ?? "";
        if (
          looksLikeCloudflare(body) ||
          cloudflareFromHeaders(response.status ?? 0, response.headers as never)
        ) {
          throwCloudflare(CF_RESOLVE);
        }
      }
      throw error;
    }
  };

  private failEmptyListing = (html: string): never => {
    if (looksLikeCloudflare(html)) throwCloudflare(CF_RESOLVE);
    const title =
      html.match(/<title>([\s\S]*?)<\/title>/i)?.[1]?.replace(/\s+/g, " ").trim() ??
      "";
    throw new Error(
      `HentaiRead listing parsed 0 titles (${html.length} bytes${title ? `, ${title.slice(0, 80)}` : ""}). Clear Network Cache, Open Challenge Page, retry.`,
    );
  };

  getHomePage = async (): Promise<HomePage> => {
    return {
      feeds: [
        {
          id: "latest",
          title: "Latest",
          content: { list: { key: "latest", disableSorting: true } },
        },
        {
          id: "popular",
          title: "Popular",
          content: { list: { key: "popular", disableSorting: true } },
        },
      ],
    };
  };

  getSortOptions = async (): Promise<SortOptions> => ({
    options: [...SORTS],
    disableOrdering: false,
  });

  getSearchFilters = async () => [
    SearchFilter(
      "types",
      "Types",
      SelectFilter(
        TYPES.map((type) => ({ id: type.id, title: type.title })),
        false,
      ),
    ),
    ...TEXT_FILTERS.map((filter) =>
      SearchFilter(filter.id, filter.name, TextFilter(), filter.hint),
    ),
    SearchFilter(
      "uploaded",
      "Uploaded",
      TextFilter(),
      "Year filter, e.g. >2024",
    ),
    SearchFilter("pages", "Pages", TextFilter(), "Page count filter, e.g. >20"),
  ];

  getItemList = async (
    request: ItemListRequest,
    page: number,
  ): Promise<PagedItemList> => {
    const sortby = request.key === "popular" ? "views" : "new";
    const html = await this.getHtml(listingUrl(page, sortby));
    const items = parseListing(html);
    if (!items.length) this.failEmptyListing(html);
    return { items, isLastPage: !hasNextPage(html) };
  };

  getSearchResults = async (
    request: SearchRequest,
    page: number,
  ): Promise<PagedItemList> => {
    const filters = request.filters ?? {};
    const pairs: [string, string][] = [
      ["s", request.query?.trim() ?? ""],
      ["title-type", "contains"],
      ["sortby", request.sort?.key ?? "new"],
      ["order", request.sort?.ascending ? "asc" : "desc"],
    ];

    const types = filters.types as { include?: string[] } | undefined;
    for (const value of types?.include ?? []) {
      pairs.push(["categories[]", value]);
    }

    for (const filter of TEXT_FILTERS) {
      const raw = (filters[filter.id] as string | undefined)?.trim();
      if (!raw) continue;
      for (const part of raw.split(",").map((s) => s.trim()).filter(Boolean)) {
        const exclude = part.startsWith("-");
        const name = exclude ? part.slice(1).trim() : part;
        const id = await this.getTagId(name, filter.type);
        if (id == null) {
          throw new Error(
            `${filter.name.replace(/s$/, "")} not found: ${name}`,
          );
        }
        if (filter.type === "manga_tag") {
          pairs.push([exclude ? "excluding[]" : "including[]", String(id)]);
        } else {
          pairs.push([`${filter.type}s[]`, String(id)]);
        }
      }
    }

    const uploaded = (filters.uploaded as string | undefined)?.trim();
    if (uploaded) {
      const kind =
        uploaded[0] === ">" ? "after" : uploaded[0] === "<" ? "before" : "in";
      pairs.push(["release-type", kind]);
      pairs.push(["release", uploaded.replace(/\D/g, "")]);
    }

    const pages = (filters.pages as string | undefined)?.trim();
    if (pages) {
      const [min, max] = parsePageRange(pages);
      pairs.push(["pages", `${min}-${max}`]);
    }

    const html = await this.getHtml(searchUrl(page, pairs));
    const items = parseListing(html);
    return { items, isLastPage: !hasNextPage(html) || items.length === 0 };
  };

  getContent = async (contentId: string): Promise<Content> => {
    const html = await this.getHtml(contentUrl(contentId));
    return parseDetails(html, contentId);
  };

  getChapters = async (contentId: string): Promise<Chapter[]> => {
    const content = await this.getContent(contentId);
    const scanlator = content.context?.scanlator as string | undefined;
    const uploaded = content.context?.uploaded as string | undefined;
    const date = uploaded ? new Date(uploaded) : new Date();
    return [
      {
        id: contentId,
        index: 0,
        number: 1,
        title: scanlator || "Chapter",
        language: "en",
        date: Number.isNaN(date.getTime()) ? new Date() : date,
        webUrl: readerUrl(contentId),
      },
    ];
  };

  getChapterPages = async (
    contentId: string,
    _chapterId: string,
  ): Promise<ChapterPage[]> => {
    const detailsHtml = await this.getHtml(contentUrl(contentId));
    const order = readerLanguageOrder(detailsHtml);

    let lastError: unknown;
    for (const language of order) {
      try {
        const html = await this.getHtml(
          readerUrl(contentId, language),
          contentUrl(contentId),
        );
        const pages = parseChapterPages(html);
        if (pages.length) return pages;
      } catch (error) {
        if (cloudflareFromThrown(error)) throwCloudflare(CF_RESOLVE);
        lastError = error;
      }
    }

    throw (
      lastError ??
      new Error("Failed to find page list. Reader scripts were missing.")
    );
  };

  private getTagId = async (
    tag: string,
    type: string,
  ): Promise<number | undefined> => {
    const taxonomy = TAXONOMY[type] ?? type;
    const url = withQuery(`${BASE}/wp-admin/admin-ajax.php`, {
      action: "search_manga_terms",
      search: tag,
      taxonomy,
    });
    const html = await this.getHtml(url, `${BASE}/`, {
      "X-Requested-With": "XMLHttpRequest",
      Accept: "application/json, text/javascript, */*;q=0.1",
    });
    let data: TermResult;
    try {
      data = JSON.parse(html) as TermResult;
    } catch {
      return undefined;
    }
    const hit = data.results?.find(
      (item) => item.text.toLowerCase() === tag.toLowerCase(),
    );
    return hit?.id;
  };
}
