"use httpclient";

import CryptoJS from "crypto-js";
import {
  ContentRating, ContentStatus, ContentType, ReadingMode,
  type Chapter, type ChapterPage, type Content, type HomePage, type Item,
  type ItemListRequest, type PagedItemList, type SearchRequest, type SourceInfo,
} from "@suwatte/toolchain/types";
import { absolute, assertOk, chaptersFrom, document, href, idFrom, image, pageResult, text } from "../../shared";

const BASE = "https://novelcrow.com";

// The site's protector stores an OpenSSL-salted AES string in an inline script.
// Some chapters have ordinary image elements instead, so try those first.
export function protectedImages(html: string): string[] {
  const payload = html.match(/\bchapter_data\s*[:=]\s*['"]([^'"]+)['"]/)?.[1];
  const password = html.match(/\b(?:wpmangaprotectornonce|nonce)\s*[:=]\s*['"]([^'"]+)['"]/)?.[1];
  if (!payload || !password || !payload.startsWith("U2FsdGVkX1")) return [];
  const plaintext = CryptoJS.AES.decrypt(payload, password).toString(CryptoJS.enc.Utf8);
  if (!plaintext) throw new Error("Could not decrypt NovelCrow chapter pages");
  const value: unknown = JSON.parse(plaintext);
  const entries: unknown[] = Array.isArray(value) ? value
    : value && typeof value === "object" && "images" in value && Array.isArray(value.images) ? value.images
    : value && typeof value === "object" && "pages" in value && Array.isArray(value.pages) ? value.pages : [];
  if (!entries.length && typeof value === "string") {
    return document(value).querySelectorAll("img").map((node) => node.getAttribute("data-src") || node.getAttribute("src") || "").filter(Boolean);
  }
  return entries.map((entry) => typeof entry === "string" ? entry
    : entry && typeof entry === "object" && "url" in entry ? String(entry.url)
    : entry && typeof entry === "object" && "src" in entry ? String(entry.src) : "")
    .filter(Boolean);
}

export default class NovelCrow {
  static info: SourceInfo = {
    id: "en.novelcrow", name: "NovelCrow", version: 1,
    website: BASE, languages: ["en"], rating: ContentRating.MATURE,
    minSupportedAppVersion: "7.0.0",
  };
  client = new HttpClient({ baseUrl: BASE, rateLimit: { permits: 2, period: 1 },
    headers: { "User-Agent": "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2 like Mac OS X) AppleWebKit/605.1.15 Mobile/15E148 Safari/604.1" },
  });
  getConfiguration() { return { imageReferer: `${BASE}/`, cloudflareResolutionURL: `${BASE}/` }; }
  private async html(path: string): Promise<string> {
    const response = await this.client.get(path);
    assertOk(response.status, absolute(path, BASE));
    return response.text();
  }
  private items(html: string, page: number): PagedItemList {
    const root = document(html);
    const nodes = root.querySelectorAll(".page-item-detail, .c-tabs-item__content, .manga__item, .page-listing-item");
    const unique = new Map<string, Item>();
    for (const node of nodes) {
      const link = node.querySelector("div.post-title a, h3 a, a.post-title, a[href*='/comic/']");
      const url = href(link, BASE);
      if (!url || !new URL(url).pathname.startsWith("/comic/")) continue;
      const id = idFrom(url, BASE);
      if (unique.has(id)) continue;
      unique.set(id, { id, title: text(link), coverImage: image(node.querySelector(".tab-thumb img, .item-thumb img, img"), BASE),
        webUrl: url, rating: ContentRating.MATURE });
    }
    return pageResult(Array.from(unique.values()), root, page);
  }
  async getSearchResults(request: SearchRequest, page: number): Promise<PagedItemList> {
    const query = encodeURIComponent(request.query?.trim() ?? "");
    const path = `${page > 1 ? `/page/${page}/` : "/"}?s=${query}&post_type=wp-manga`;
    return this.items(await this.html(path), page);
  }
  async getHomePage(): Promise<HomePage> {
    return { feeds: [
      { id: "recent", title: "Recently added", content: { list: { key: "recent" } } },
      { id: "popular", title: "Popular", content: { list: { key: "popular" } } },
    ] };
  }
  async getItemList(request: ItemListRequest, page: number): Promise<PagedItemList> {
    const sort = request.key === "popular" ? "views" : "latest";
    const path = `${page > 1 ? `/page/${page}/` : "/"}?s=&post_type=wp-manga&m_orderby=${sort}`;
    return this.items(await this.html(path), page);
  }
  async getContent(contentId: string): Promise<Content> {
    const url = absolute(`/${contentId.replace(/^\/+|\/+$/g, "")}/`, BASE);
    const root = document(await this.html(url));
    const title = text(root.querySelector("#manga-title h1, div.post-title h1, .post-title h3, h1"));
    const coverImage = image(root.querySelector(".summary_image img, .thumb img, meta[property='og:image']"), BASE)
      || absolute(root.querySelector("meta[property='og:image']")?.getAttribute("content") ?? undefined, BASE);
    const statusText = root.querySelectorAll(".post-content_item").find((n) => /status/i.test(text(n.querySelector("h5"))))?.text ?? "";
    return { title, coverImage, webUrl: url, rating: ContentRating.MATURE,
      contentType: ContentType.COMIC, readingMode: ReadingMode.PAGED_COMIC,
      status: /completed/i.test(statusText) ? ContentStatus.COMPLETED : ContentStatus.ONGOING,
      summary: text(root.querySelector(".description-summary .summary__content, .summary_content .post-content_item + div, .manga-excerpt")),
      ...(() => { const genres = root.querySelectorAll(".genres-content a").map((n) => ({ id: text(n), title: text(n) })); return genres.length ? { genres } : {}; })(),
    };
  }
  async getChapters(contentId: string): Promise<Chapter[]> {
    const root = document(await this.html(`/${contentId.replace(/^\/+|\/+$/g, "")}/`));
    return chaptersFrom(root.querySelectorAll("li.wp-manga-chapter a, .chapter-item a"), BASE);
  }
  async getChapterPages(_contentId: string, chapterId: string): Promise<ChapterPage[]> {
    const html = await this.html(chapterId);
    const root = document(html);
    const nodes = root.querySelectorAll(".reading-content .page-break img, .reading-content .blocks-gallery-item img, .reading-content .text-left img");
    const urls = nodes.map((node) => image(node, BASE)).filter(Boolean);
    if (urls.length) return [...new Set(urls)].map((url) => ({ url }));
    const protectedUrls = protectedImages(html).map((url) => absolute(url, BASE)).filter(Boolean);
    if (!protectedUrls.length) throw new Error("NovelCrow returned no chapter images; the reader data may have changed");
    return protectedUrls.map((url) => ({ url }));
  }
}
