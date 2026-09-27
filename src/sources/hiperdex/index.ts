"use httpclient";

import {
  PickerFilter,
  SearchFilter,
  SelectFilter,
  UIPicker,
  UITextField,
  UIToggle,
  UIWebViewButton,
  type Chapter,
  type ChapterPage,
  type Content,
  type HomePage,
  type ItemListRequest,
  type NetworkRequest,
  type PagedItemList,
  type PopulatedForm,
  type SearchRequest,
  type SortOptions,
  type SourceConfiguration,
  type SourceInfo,
  type UIForm,
} from "@suwatte/toolchain/types";
import { ContentRating } from "@suwatte/toolchain/types";
import { browserHeaders } from "../_shared/client";
import {
  cloudflareFromHeaders,
  looksLikeCloudflare,
  throwCloudflare,
} from "../_shared/cloudflare";
import { absoluteUrl } from "../_shared/http";
import {
  AUTH_HEADER,
  BASE,
  CF_RESOLVE,
  DEFAULT_AUTH,
  DEFAULT_MAX_RATING,
  GENRES,
  IMAGE_ACCEPT,
  PAGE_LIMIT,
  RATINGS,
  SETTINGS,
  SORTS,
  STATUSES,
  TYPES,
} from "./constants";
import {
  chapterFromDto,
  chaptersInput,
  cleanTitle,
  contentFromManga,
  detailsInput,
  isApiKeyError,
  itemFromManga,
  pagesFromDto,
  pagesInput,
  parseAuthFromBundle,
  parseChapterId,
  parseSlug,
  scriptSrcFromHome,
  searchInput,
  sessionCookieFromHeaders,
  trpcUrl,
  unwrapBatch,
  type ChapterDto,
  type MangaDto,
  type PageDto,
} from "./parse";

type TitlePrefs = {
  stripVersion: boolean;
  customRegex: string;
  noCleanBrowse: boolean;
};

const isCloudflareError = (error: unknown): boolean => {
  const name = String((error as { name?: string })?.name ?? "");
  const message = String((error as { message?: string })?.message ?? error);
  return (
    name.includes("Cloudflare") ||
    message.toLowerCase().includes("cloudflare")
  );
};

const cloudflareFromThrown = (error: unknown): boolean => {
  if (isCloudflareError(error)) return true;
  const response = (
    error as { response?: { status?: number; headers?: unknown } }
  ).response;
  if (!response) return false;
  return cloudflareFromHeaders(response.status ?? 0, response.headers as never);
};

const isImageRequest = (url: string): boolean =>
  /r2d2storage\.|cloud-\d+\.r2d2/i.test(url) ||
  /\.(jpe?g|png|webp|avif|gif)(\?|$)/i.test(url);

/**
 * Hiper (Keiyoushi) tRPC client.
 * Auth is `x-cfg-auth` plus session cookie `__st` minted by GET `/`.
 * 401 without the cookie is "Invalid or missing API Key" — same as Mihon.
 */
export default class Target {
  #auth = DEFAULT_AUTH;
  #cookie = "";
  #warmed = false;

  client = (() => {
    const http = new HttpClient({
      timeout: 45_000,
      retries: { count: 2, delay: 400 },
      cloudflareResolutionURL: CF_RESOLVE,
      headers: browserHeaders({
        Referer: `${BASE}/`,
        Origin: BASE,
        [AUTH_HEADER]: DEFAULT_AUTH,
      }),
    });
    http.interceptors.request.use((request) => {
      request.headers.set(AUTH_HEADER, this.#auth);
      if (this.#cookie) request.headers.set("Cookie", this.#cookie);
      if (isImageRequest(request.url)) {
        request.headers.set("Accept", IMAGE_ACCEPT);
        request.headers.set("Referer", `${BASE}/`);
      }
      return request;
    });
    return http;
  })();

  static info: SourceInfo = {
    id: "en.hiperdex",
    name: "Hiperdex",
    version: 1.0,
    website: BASE,
    thumbnail: "hiperdex.png",
    languages: ["en"],
    rating: ContentRating.MATURE,
  };

  getConfiguration = (): SourceConfiguration =>
    ({
      imageReferer: `${BASE}/`,
      cloudflareResolutionURL: CF_RESOLVE,
      useClientForImageRequests: true,
    }) as SourceConfiguration;

  getSettingsPage = async (): Promise<UIForm> => {
    const maxRating =
      (await ObjectStore.string(SETTINGS.maxRating)) ?? DEFAULT_MAX_RATING;
    const cleanTitleOn =
      (await ObjectStore.boolean(SETTINGS.cleanTitle)) ?? false;
    const cleanRegex = (await ObjectStore.string(SETTINGS.cleanRegex)) ?? "";
    const noCleanBrowse =
      (await ObjectStore.boolean(SETTINGS.noCleanBrowse)) ?? false;
    return {
      sections: [
        {
          header: "Cloudflare",
          footer:
            "If Browse fails with CF: complete Resolve, then retry. Availability aborts on CF by design. Settings → Advanced → Clear Network Cache if it loops.",
          views: [
            UIWebViewButton({
              title: "Open Challenge Page",
              url: { url: CF_RESOLVE },
            }),
          ],
        },
        {
          header: "Content",
          footer: "Restricts listings to this rating or below.",
          views: [
            UIPicker({
              id: SETTINGS.maxRating,
              title: "Default max rating",
              options: [...RATINGS],
              currentValue: maxRating,
              defaultValue: DEFAULT_MAX_RATING,
            }),
          ],
        },
        {
          header: "Titles",
          views: [
            UIToggle({
              id: SETTINGS.cleanTitle,
              title: "Strip version tags from titles",
              currentValue: cleanTitleOn,
              defaultValue: false,
            }),
            UITextField({
              id: SETTINGS.cleanRegex,
              title: "Custom title regex",
              currentValue: cleanRegex,
              placeholder: "optional",
            }),
            UIToggle({
              id: SETTINGS.noCleanBrowse,
              title: "Don't clean titles while browsing",
              currentValue: noCleanBrowse,
              defaultValue: false,
            }),
          ],
        },
      ],
    };
  };

  onFormSubmitted = async (_id: string, data: PopulatedForm): Promise<void> => {
    if (typeof data[SETTINGS.maxRating] === "string") {
      await ObjectStore.set(SETTINGS.maxRating, data[SETTINGS.maxRating]);
    }
    if (typeof data[SETTINGS.cleanTitle] === "boolean") {
      await ObjectStore.set(SETTINGS.cleanTitle, data[SETTINGS.cleanTitle]);
    }
    if (typeof data[SETTINGS.cleanRegex] === "string") {
      await ObjectStore.set(SETTINGS.cleanRegex, data[SETTINGS.cleanRegex]);
    }
    if (typeof data[SETTINGS.noCleanBrowse] === "boolean") {
      await ObjectStore.set(SETTINGS.noCleanBrowse, data[SETTINGS.noCleanBrowse]);
    }
  };

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

  getHomePage = async (): Promise<HomePage> => ({
    feeds: [
      {
        id: "popular",
        title: "Popular",
        content: { list: { key: "popular", disableSorting: true } },
      },
      {
        id: "latest",
        title: "Latest",
        content: { list: { key: "latest", disableSorting: true } },
      },
    ],
  });

  getSortOptions = async (): Promise<SortOptions> => ({
    options: [...SORTS],
    disableOrdering: true,
  });

  getSearchFilters = async () => [
    SearchFilter(
      "type",
      "Type",
      PickerFilter([{ id: "", title: "All" }, ...TYPES]),
    ),
    SearchFilter(
      "status",
      "Status",
      PickerFilter([{ id: "", title: "All" }, ...STATUSES]),
    ),
    SearchFilter(
      "rating",
      "Rating",
      PickerFilter([{ id: "", title: "All" }, ...RATINGS]),
    ),
    SearchFilter(
      "genres",
      "Genres",
      SelectFilter(
        GENRES.map((g) => ({ id: g, title: g })),
        false,
      ),
    ),
  ];

  getItemList = async (
    request: ItemListRequest,
    page: number,
  ): Promise<PagedItemList> => {
    const sort = request.key === "latest" ? "recent" : "popular";
    return this.searchPage({ query: "", sort, page });
  };

  getSearchResults = async (
    request: SearchRequest,
    page: number,
  ): Promise<PagedItemList> => {
    const filters = request.filters ?? {};
    const genres = (filters.genres as { include?: string[] } | undefined)
      ?.include ?? [];
    return this.searchPage({
      query: request.query?.trim() ?? "",
      sort: request.sort?.key || "relevance",
      page,
      type: String(filters.type ?? ""),
      status: String(filters.status ?? ""),
      contentRating: String(filters.rating ?? ""),
      genres,
    });
  };

  getContent = async (contentId: string): Promise<Content> => {
    const slug = parseSlug(contentId);
    const data = await this.getJson(
      trpcUrl("auth.me,series.bySlugWithGenres", detailsInput(slug)),
    );
    const dto = unwrapBatch<MangaDto>(data);
    if (!dto?.slug) throw new Error(`Title not found: ${slug}`);
    const prefs = await this.titlePrefs();
    const title = cleanTitle(dto.title ?? slug, prefs);
    const content = contentFromManga(dto, title);
    if (title !== (dto.title ?? "").trim() && dto.title) {
      content.summary = [dto.title, content.summary]
        .filter(Boolean)
        .join("\n\n");
    }
    return content;
  };

  getChapters = async (contentId: string): Promise<Chapter[]> => {
    const slug = parseSlug(contentId);
    const details = await this.getJson(
      trpcUrl("auth.me,series.bySlugWithGenres", detailsInput(slug)),
    );
    const manga = unwrapBatch<MangaDto>(details);
    const seriesId = manga?.id;
    if (seriesId == null) throw new Error(`Missing series id for ${slug}`);
    const data = await this.getJson(
      trpcUrl("auth.me,series.chapters", chaptersInput(seriesId)),
    );
    const list = unwrapBatch<ChapterDto[]>(data) ?? [];
    return list
      .map((dto, index) => {
        const chapter = chapterFromDto(dto, slug);
        if (!chapter) return undefined;
        chapter.index = index;
        return chapter;
      })
      .filter((chapter): chapter is Chapter => !!chapter);
  };

  getChapterPages = async (
    contentId: string,
    chapterId: string,
  ): Promise<ChapterPage[]> => {
    const slug = parseSlug(contentId);
    const ref = parseChapterId(chapterId);
    const data = await this.getJson(
      trpcUrl(
        "auth.me,series.bySlug,reader.chapterPages",
        pagesInput(slug, ref.number, ref.id),
      ),
    );
    if (Array.isArray(data) && data.some((el) => el && typeof el === "object" && "error" in el)) {
      throw new Error("Reader API rejected this chapter (try Resolve CF, then retry).");
    }
    const pages = unwrapBatch<PageDto[]>(data) ?? [];
    const mapped = pagesFromDto(pages);
    if (!mapped.length) throw new Error("No pages found for this chapter");
    return mapped;
  };

  private searchPage = async (options: {
    query: string;
    sort: string;
    page: number;
    type?: string;
    status?: string;
    contentRating?: string;
    genres?: string[];
  }): Promise<PagedItemList> => {
    const prefs = await this.titlePrefs();
    const maxRating =
      (await ObjectStore.string(SETTINGS.maxRating)) ?? DEFAULT_MAX_RATING;
    const data = await this.getJson(
      trpcUrl(
        "search.query",
        searchInput({
          query: options.query,
          sort: options.sort,
          genres: options.genres ?? [],
          type: options.type,
          status: options.status,
          contentRating: options.contentRating,
          maxRating,
          page: options.page,
          limit: PAGE_LIMIT,
        }),
      ),
    );
    const hits =
      unwrapBatch<{ hits?: MangaDto[] }>(data, 0)?.hits ??
      unwrapBatch<{ hits?: MangaDto[] }>(data)?.hits ??
      [];
    const browseClean = prefs.stripVersion || !!prefs.customRegex;
    const items = hits
      .filter((hit) => hit.slug && hit.title)
      .map((hit) => {
        const title =
          prefs.noCleanBrowse || !browseClean
            ? hit.title!
            : cleanTitle(hit.title!, prefs);
        const item = itemFromManga(hit, title);
        item.context = hit.id != null ? { mangaId: hit.id } : undefined;
        return item;
      });
    return { items, isLastPage: hits.length < PAGE_LIMIT };
  };

  private titlePrefs = async (): Promise<TitlePrefs> => ({
    stripVersion: (await ObjectStore.boolean(SETTINGS.cleanTitle)) ?? false,
    customRegex: (await ObjectStore.string(SETTINGS.cleanRegex)) ?? "",
    noCleanBrowse: (await ObjectStore.boolean(SETTINGS.noCleanBrowse)) ?? false,
  });

  private warmSession = async (): Promise<void> => {
    try {
      const response = await this.client.get(`${BASE}/`, {
        headers: { Accept: "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8" },
      });
      if (cloudflareFromHeaders(response.status, response.headers)) {
        throwCloudflare(CF_RESOLVE);
      }
      const body = await response.text();
      if (looksLikeCloudflare(body)) throwCloudflare(CF_RESOLVE);
      const cookie = sessionCookieFromHeaders(response.headers);
      if (cookie) this.#cookie = cookie;
      this.#warmed = true;
    } catch (error) {
      if (cloudflareFromThrown(error)) throwCloudflare(CF_RESOLVE);
      throw error;
    }
  };

  private refreshAuth = async (): Promise<void> => {
    try {
      const home = await this.client.get(`${BASE}/`, {
        headers: { Accept: "text/html,application/xhtml+xml" },
      });
      const html = await home.text();
      const src = scriptSrcFromHome(html);
      if (!src) return;
      const js = await this.client.get(absoluteUrl(BASE, src), {
        headers: { Accept: "*/*", Referer: `${BASE}/` },
      });
      const next = parseAuthFromBundle(await js.text());
      if (next) this.#auth = next;
    } catch (error) {
      if (cloudflareFromThrown(error)) throwCloudflare(CF_RESOLVE);
    }
  };

  private getJson = async (url: string): Promise<unknown> => {
    const once = async (): Promise<{ status: number; body: string }> => {
      try {
        const response = await this.client.get(url, {
          headers: {
            Accept: "application/json",
            Origin: BASE,
            Referer: `${BASE}/`,
          },
        });
        if (cloudflareFromHeaders(response.status, response.headers)) {
          throwCloudflare(CF_RESOLVE);
        }
        const body = await response.text();
        if (looksLikeCloudflare(body)) throwCloudflare(CF_RESOLVE);
        return { status: response.status, body };
      } catch (error) {
        if (cloudflareFromThrown(error)) throwCloudflare(CF_RESOLVE);
        const response = (
          error as { response?: { status: number; text: () => Promise<string> } }
        ).response;
        if (response?.status) {
          const body = await response.text().catch(() => "");
          if (looksLikeCloudflare(body)) throwCloudflare(CF_RESOLVE);
          return { status: response.status, body };
        }
        throw error;
      }
    };

    try {
      if (!this.#warmed) await this.warmSession();
      let { status, body } = await once();
      if (isApiKeyError(status, body)) {
        await this.warmSession();
        ({ status, body } = await once());
      }
      if (isApiKeyError(status, body)) {
        await this.refreshAuth();
        ({ status, body } = await once());
      }
      if (isApiKeyError(status, body)) {
        throw new Error("Hiperdex API key/session rejected. Open the source, Resolve CF, then retry.");
      }
      if (status < 200 || status >= 300) {
        throw new Error(`GET ${url} failed (${status}): ${body.slice(0, 180)}`);
      }
      return JSON.parse(body) as unknown;
    } catch (error) {
      if (cloudflareFromThrown(error)) throwCloudflare(CF_RESOLVE);
      throw error;
    }
  };
}
