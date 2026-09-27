"use httpclient";

import {
  ContentRating, ContentStatus, ContentType, ReadingMode,
  type Chapter, type ChapterPage, type Content, type HomePage, type Item,
  type ItemListRequest, type PagedItemList, type SearchRequest, type SourceInfo,
} from "@suwatte/toolchain/types";
import { absolute, assertOk, chaptersFrom, checkedHtml, document, href, idFrom, image, pageResult, text } from "../../shared";

const BASE = "https://allporncomic.com";
const pathFor = (id: string) => `/${id.replace(/^\/+|\/+$/g, "")}/`;

export default class AllPornComic {
  static info: SourceInfo = {
    id: "en.allporncomic", name: "AllPornComic", version: 1, thumbnail: "allporncomic.png",
    website: BASE, languages: ["en"], rating: ContentRating.MATURE,
    minSupportedAppVersion: "7.0.0",
  };
  client = new HttpClient({ baseUrl: BASE, cloudflareResolutionURL: `${BASE}/`,
    headers: { Referer: `${BASE}/` }, rateLimit: { permits: 2, period: 1 } });
  getConfiguration() { return { imageReferer: `${BASE}/`, cloudflareResolutionURL: `${BASE}/`, useClientForImageRequests: true }; }
  private async html(path: string) {
    const response = await this.client.get(path);
    assertOk(response.status, absolute(path, BASE));
    return checkedHtml(await response.text(), `${BASE}/`);
  }
  private async list(path: string, page: number): Promise<PagedItemList> {
    const root = document(await this.html(path));
    const entries = new Map<string, Item>();
    for (const card of root.querySelectorAll(".page-item-detail, .manga__item, .c-tabs-item__content, .manga-item")) {
      const link = card.querySelector(".post-title a, a.manga-item__link, h3 a, h2 a");
      const url = href(link, BASE);
      if (!url || !url.includes("/porncomic/")) continue;
      const id = idFrom(url, BASE);
      const cover = card.querySelector(".tab-thumb img, .item-thumb img, img");
      const title = text(link) || link?.getAttribute("title")?.trim()
        || cover?.getAttribute("alt")?.trim() || id.split("/").pop()!.replace(/-/g, " ");
      entries.set(id, { id, title, coverImage: image(cover, BASE), webUrl: url, rating: ContentRating.MATURE });
    }
    return pageResult([...entries.values()], root, page);
  }
  getHomePage = async (): Promise<HomePage> => ({ feeds: [
    { id: "popular", title: "Popular", content: { list: { key: "popular" } } },
    { id: "latest", title: "Latest", content: { list: { key: "latest" } } },
  ] });
  getItemList(request: ItemListRequest, page: number) {
    const sort = request.key === "popular" ? "views" : "latest";
    return this.list(`/porncomic/${page > 1 ? `page/${page}/` : ""}?m_orderby=${sort}`, page);
  }
  getSearchResults(request: SearchRequest, page: number) {
    const query = request.query?.trim() ?? "";
    if (!query) return this.getItemList({ key: "latest" }, page);
    return this.list(`${page > 1 ? `/page/${page}/` : "/"}?s=${encodeURIComponent(query)}&post_type=wp-manga`, page);
  }
  async getContent(contentId: string): Promise<Content> {
    const url = absolute(pathFor(contentId), BASE);
    const root = document(await this.html(url));
    const title = text(root.querySelector(".post-title h1, .post-title h3, h1"))
      || root.querySelector("meta[property='og:title']")?.getAttribute("content")?.trim() || contentId;
    const coverImage = image(root.querySelector(".summary_image img, .thumb img"), BASE)
      || absolute(root.querySelector("meta[property='og:image']")?.getAttribute("content") ?? undefined, BASE);
    const status = root.querySelectorAll(".post-content_item").find((n) => /status/i.test(text(n.querySelector("h5"))))?.text ?? "";
    const genres = root.querySelectorAll(".genres-content a").map((n) => ({ id: text(n), title: text(n) })).filter((g) => g.title);
    return { title, coverImage, webUrl: url, rating: ContentRating.MATURE,
      contentType: ContentType.MANGA, readingMode: ReadingMode.PAGED_MANGA,
      status: /complete/i.test(status) ? ContentStatus.COMPLETED : ContentStatus.UNKNOWN,
      summary: text(root.querySelector(".description-summary, .summary__content")),
      ...(genres.length ? { genres } : {}),
    };
  }
  async getChapters(contentId: string): Promise<Chapter[]> {
    const root = document(await this.html(pathFor(contentId)));
    return chaptersFrom(root.querySelectorAll("li.wp-manga-chapter a, .chapter-item a"), BASE);
  }
  async getChapterPages(_contentId: string, chapterId: string): Promise<ChapterPage[]> {
    const root = document(await this.html(chapterId));
    const pages = root.querySelectorAll(".reading-content img, .page-break img, img.wp-manga-chapter-img")
      .map((node) => image(node, BASE)).filter(Boolean);
    if (!pages.length) throw new Error("AllPornComic returned no chapter images");
    return [...new Set(pages)].map((url) => ({ url }));
  }
}
