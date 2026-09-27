"use httpclient";

import {
  ContentRating, ContentStatus, ContentType, ReadingMode,
  type Chapter, type ChapterPage, type Content, type HomePage, type Item,
  type ItemListRequest, type PagedItemList, type SearchRequest, type SourceInfo,
} from "@suwatte/toolchain/types";
import { absolute, assertOk, chaptersFrom, document, href, idFrom, image, pageResult, pathFrom, text } from "../../shared";

const BASE = "https://hentairead.com";
const scriptImages = (html: string) => {
  const root = document(html);
  const scripts = root.querySelectorAll("#single-chapter-js-extra, #single-chapter-js-before");
  const addresses: string[] = [];
  for (const script of scripts) {
    const raw = script.text.replace(/\\\//g, "/");
    for (const match of raw.matchAll(/https?:\/\/[^\s"'<>\\]+\.(?:jpe?g|png|webp|gif)(?:\?[^\s"'<>\\]*)?/gi))
      addresses.push(match[0]);
  }
  return [...new Set(addresses)];
};

export default class HentaiRead {
  static info: SourceInfo = {
    id: "en.hentairead", name: "HentaiRead", version: 3,
    website: BASE, languages: ["en"], rating: ContentRating.MATURE,
    minSupportedAppVersion: "7.0.0",
  };
  client = new HttpClient({ baseUrl: BASE, rateLimit: { permits: 2, period: 1 },
    cloudflareResolutionURL: `${BASE}/`,
    headers: { Referer: `${BASE}/` },
  });
  getConfiguration() { return { imageReferer: `${BASE}/`, cloudflareResolutionURL: `${BASE}/`, useClientForImageRequests: true }; }
  private async html(path: string): Promise<string> {
    const response = await this.client.get(path); assertOk(response.status, absolute(path, BASE));
    return response.text();
  }
  private listing(html: string, page: number): PagedItemList {
    const root = document(html);
    const entries = new Map<string, Item>();
    for (const node of root.querySelectorAll(".manga-item")) {
      const link = node.querySelector("h3 a[href*='/hentai/'], a.manga-item__link, a[href*='/hentai/']");
      const url = href(link, BASE);
      if (!url || !pathFrom(url).startsWith("/hentai/")) continue;
      const id = idFrom(url, BASE);
      entries.set(id, { id, title: text(link),
        coverImage: image(node.querySelector("img.manga-item__img-inner, img"), BASE),
        webUrl: url, rating: ContentRating.MATURE });
    }
    return pageResult([...entries.values()], root, page);
  }
  async getSearchResults(request: SearchRequest, page: number): Promise<PagedItemList> {
    const query = request.query?.trim() ?? "";
    const path = `${page > 1 ? `/page/${page}/` : "/"}?s=${encodeURIComponent(query)}&title-type=contains&sortby=latest`;
    return this.listing(await this.html(path), page);
  }
  async getHomePage(): Promise<HomePage> {
    return { feeds: [
      { id: "recent", title: "Recently added", content: { list: { key: "recent" } } },
      { id: "popular", title: "Popular", content: { list: { key: "popular" } } },
    ] };
  }
  async getItemList(request: ItemListRequest, page: number): Promise<PagedItemList> {
    const sort = request.key === "popular" ? "views" : "latest";
    const path = `/hentai/${page > 1 ? `page/${page}/` : ""}?sortby=${sort}`;
    return this.listing(await this.html(path), page);
  }
  async getContent(contentId: string): Promise<Content> {
    const url = absolute(`/${contentId.replace(/^\/+|\/+$/g, "")}/`, BASE);
    const root = document(await this.html(url));
    return { title: text(root.querySelector(".manga-titles h1, h1")),
      coverImage: image(root.querySelector("img[fetchpriority='high'], .summary_image img"), BASE)
        || absolute(root.querySelector("meta[property='og:image'], meta[name='twitter:image']")?.getAttribute("content") ?? undefined, BASE),
      rating: ContentRating.MATURE, webUrl: url,
      contentType: ContentType.MANGA, readingMode: ReadingMode.PAGED_MANGA,
      status: ContentStatus.UNKNOWN,
      summary: text(root.querySelector(".description-summary, .summary__content, .manga-description")),
      ...(() => { const genres = root.querySelectorAll("a[href*='/genre/'], a[href*='/tag/']")
        .map((n) => ({ id: text(n), title: text(n) })).filter((tag) => tag.title);
        return genres.length ? { genres } : {}; })(),
    };
  }
  async getChapters(contentId: string): Promise<Chapter[]> {
    const root = document(await this.html(`/${contentId.replace(/^\/+|\/+$/g, "")}/`));
    const links = root.querySelectorAll("li.wp-manga-chapter a, .chapter-item a, .listing-chapters_wrap a[href*='/chapter-']");
    const chapters = chaptersFrom(links, BASE);
    // Some HentaiRead titles contain their pages directly, without a chapter list.
    return chapters.length ? chapters : [{ id: `/${contentId}/`, index: 0, number: 1, title: "Complete gallery", language: "en" }];
  }
  async getChapterPages(_contentId: string, chapterId: string): Promise<ChapterPage[]> {
    const html = await this.html(chapterId);
    const root = document(html);
    const images = root.querySelectorAll(".reading-content img, img.wp-manga-chapter-img");
    const urls = images.map((node) => image(node, BASE)).filter(Boolean);
    const pages = urls.length ? [...new Set(urls)] : scriptImages(html);
    if (!pages.length) throw new Error("HentaiRead returned no reader images");
    return pages.map((url) => ({ url }));
  }
}
