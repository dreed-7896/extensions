"use httpclient";

import {
  ContentRating, ContentStatus, ContentType, ReadingMode,
  type Chapter, type ChapterPage, type Content, type HomePage, type Item,
  type ItemListRequest, type PagedItemList, type SearchRequest, type SortOptions, type SourceInfo,
} from "@suwatte/toolchain/types";
import { absolute, assertOk, checkedHtml, document, href, idFrom, image, text } from "../../shared";

const BASE = "https://imhentai.xxx";
const contentPath = (id: string) => `/${id.replace(/^\/+|\/+$/g, "")}/`;
const fullImage = (url: string) => url.replace(/t\.(jpe?g|png|webp|gif)(\?.*)?$/i, ".$1$2");
const thumbnail = (card: ReturnType<ReturnType<typeof document>["querySelector"]>) => {
  const img = card?.querySelector(".inner_thumb img:not(.thumb_flag), img:not(.thumb_flag)");
  const url = image(img, BASE);
  return /(?:\/flags?\/|thumb_flag)/i.test(url) ? "" : url;
};

export default class IMHentai {
  static info: SourceInfo = {
    id: "all.imhentai", name: "IMHentai", version: 1, thumbnail: "imhentai.png",
    website: BASE, languages: ["all", "en", "ja", "es", "fr"], rating: ContentRating.MATURE,
    minSupportedAppVersion: "7.0.0",
  };
  client = new HttpClient({ baseUrl: BASE, cloudflareResolutionURL: `${BASE}/`,
    headers: { Referer: `${BASE}/`, "Accept-Encoding": "identity" }, rateLimit: { permits: 2, period: 1 } });
  getConfiguration() { return { imageReferer: `${BASE}/`, cloudflareResolutionURL: `${BASE}/`, useClientForImageRequests: true }; }
  private async html(path: string) {
    const response = await this.client.get(path);
    assertOk(response.status, absolute(path, BASE));
    return checkedHtml(await response.text(), `${BASE}/`);
  }
  private async list(path: string): Promise<PagedItemList> {
    const root = document(await this.html(path));
    const items: Item[] = [];
    const seen = new Set<string>();
    for (const card of root.querySelectorAll("div.thumb")) {
      const link = card.querySelector(".inner_thumb a");
      const url = href(link, BASE);
      if (!url) continue;
      const id = idFrom(url, BASE);
      const title = text(card.querySelector(".caption")) || link?.getAttribute("title")?.trim() || "";
      if (!title || seen.has(id)) continue;
      seen.add(id);
      items.push({ id, title, coverImage: thumbnail(card), webUrl: url, rating: ContentRating.MATURE });
    }
    return { items, isLastPage: !root.querySelector(".pagination li.active + li:not(.disabled), a[rel=next]") };
  }
  getHomePage = async (): Promise<HomePage> => ({ feeds: [
    { id: "popular", title: "Popular", content: { list: { key: "popular" } } },
    { id: "latest", title: "Latest", content: { list: { key: "latest" } } },
  ] });
  getSortOptions = async (): Promise<SortOptions> => ({ options: [
    { id: "popular", title: "Popular" }, { id: "latest", title: "Latest" },
    { id: "downloads", title: "Downloads" }, { id: "trending", title: "Trending" },
  ], disableOrdering: true });
  getItemList(request: ItemListRequest, page: number) {
    return this.list(`/${request.key === "popular" ? "popular/" : ""}?page=${Math.max(1, page)}`);
  }
  async getSearchResults(request: SearchRequest, page: number): Promise<PagedItemList> {
    const sort = request.sort?.key ?? "popular";
    const flags = { pp: "0", lt: "0", dl: "0", tr: "0" };
    flags[sort === "latest" ? "lt" : sort === "downloads" ? "dl" : sort === "trending" ? "tr" : "pp"] = "1";
    const params = new Map(Object.entries(flags));
    for (const key of ["m", "d", "w", "i", "a", "g", "en", "jp", "es", "fr", "kr", "de", "ru"])
      params.set(key, "1");
    const filters = request.filters ?? {};
    const terms: string[] = [];
    for (const key of ["tags", "parodies", "artists", "characters", "groups"]) {
      const value = filters[key];
      if (typeof value !== "string") continue;
      for (const term of value.split(",").map((entry) => entry.trim()).filter(Boolean)) {
        const excluded = term.startsWith("-");
        const normalized = term.replace(/^-/, "").trim();
        terms.push(`${excluded ? "-" : "+"}${key.replace(/s$/, "")}:"${normalized}"`);
      }
    }
    const q = request.query?.trim() ?? "";
    params.set("key", terms.length ? terms.join(" ") : q);
    params.set("page", String(Math.max(1, page)));
    const makePath = () => `/search/?${[...params].map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`).join("&")}`;
    let result = await this.list(makePath());
    if (!result.items.length && !terms.length && q.split(/\s+/).length > 1) {
      params.set("key", q.split(/\s+/).join(","));
      result = await this.list(makePath());
    }
    return result;
  }
  async getContent(contentId: string): Promise<Content> {
    const url = absolute(contentPath(contentId), BASE);
    const root = document(await this.html(url));
    const info = root.querySelector(".gallery_first");
    if (!info) throw new Error("IMHentai gallery details were not found");
    const title = text(info.querySelector("h1")) || contentId;
    const genres = info.querySelectorAll("li").filter((node) => /^Tags:/i.test(text(node)))
      .flatMap((node) => node.querySelectorAll("a.tag")).map((node) => text(node)).filter(Boolean);
    return { title, coverImage: image(info.querySelector(".left_cover img, .cover img"), BASE),
      webUrl: url, rating: ContentRating.MATURE, status: ContentStatus.COMPLETED,
      contentType: ContentType.MANGA, readingMode: ReadingMode.PAGED_MANGA,
      summary: info.querySelectorAll("li").filter((node) => /^(Parodies|Characters|Languages|Categories):/i.test(text(node)))
        .map((node) => text(node)).join("\n"),
      ...(genres.length ? { genres: genres.map((tag) => ({ id: tag, title: tag })) } : {}),
    };
  }
  async getChapters(contentId: string): Promise<Chapter[]> {
    return [{ id: contentId, index: 0, number: 1, title: "Complete gallery", language: "all",
      webUrl: absolute(contentPath(contentId), BASE) }];
  }
  async getChapterPages(_contentId: string, chapterId: string): Promise<ChapterPage[]> {
    const root = document(await this.html(contentPath(chapterId)));
    for (const script of root.querySelectorAll("script")) {
      const json = script.text.match(/\$\.parseJSON\('([\s\S]*?)'\);/)?.[1];
      if (!json) continue;
      try {
        const data = JSON.parse(json) as Record<string, string>;
        const field = (id: string) => root.querySelector(`input#${id}`)?.getAttribute("value") ?? "";
        const directory = field("load_dir");
        const loadId = field("load_id");
        const server = field("load_server") ? `m${field("load_server")}.imhentai.xxx`
          : image(root.querySelector(".left_cover img, .cover img"), BASE).match(/^https?:\/\/([^/]+)/)?.[1];
        if (!directory || !loadId || !server || !field("gallery_id")) continue;
        const pages = Object.entries(data).filter(([index]) => /^\d+$/.test(index))
          .sort(([a], [b]) => Number(a) - Number(b)).map(([index, kind]) => {
            const ext = ({ p: "png", b: "bmp", g: "gif", w: "webp" } as Record<string, string>)[kind.split(",")[0]] ?? "jpg";
            return { url: `https://${server}/${directory}/${loadId}/${index}.${ext}` };
          });
        if (pages.length) return pages;
      } catch { /* Fall back to the visible thumbnail gallery. */ }
    }
    const pages = root.querySelectorAll(".gthumb img, .gallery_thumb img")
      .map((img) => fullImage(image(img, BASE))).filter(Boolean);
    if (!pages.length) throw new Error("IMHentai returned no reader images");
    return pages.map((url) => ({ url }));
  }
}
