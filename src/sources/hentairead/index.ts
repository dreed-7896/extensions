"use httpclient";

import {
  ContentRating, ContentStatus, ContentType, ReadingMode,
  type Chapter, type ChapterPage, type Content, type HomePage, type Item,
  type ItemListRequest, type PagedItemList, type SearchRequest, type SourceInfo,
} from "@suwatte/toolchain/types";
import { absolute, assertOk, chaptersFrom, checkedHtml, document, href, idFrom, image, pageResult, pathFrom, text } from "../../shared";

const BASE = "https://hentairead.com";
const titleOf = (node: ReturnType<ReturnType<typeof document>["querySelector"]>) =>
  text(node) || node?.getAttribute("title")?.trim() || node?.getAttribute("aria-label")?.trim()
  || node?.querySelector("img")?.getAttribute("alt")?.trim() || "";

// HentaiRead serves some images from a separate reader CDN. Its cover CDN's
// /preview/ image is only a thumbnail, even if the URL points to a full page.
const readerImage = (url: string) => url.replace(/^https?:\/\/hencover\.xyz\//i, "https://henread.xyz/")
  .replace(/^(https?:\/\/henread\.xyz\/)(?:preview\/)/i, "$1");

const embeddedPages = (raw: string): string[] => {
  const urls: string[] = [];
  for (const assignment of raw.matchAll(/\b(?:pagesData|pages_data|chapter_data)\s*[:=]\s*/gi)) {
    const start = assignment.index + assignment[0].length;
    if (raw[start] !== "{") continue;
    let depth = 0;
    let quoted = false;
    let escaped = false;
    let end = start;
    for (; end < raw.length; end++) {
      const character = raw[end];
      if (escaped) { escaped = false; continue; }
      if (quoted && character === "\\") { escaped = true; continue; }
      if (character === '"') { quoted = !quoted; continue; }
      if (!quoted && character === "{") depth++;
      if (!quoted && character === "}" && --depth === 0) { end++; break; }
    }
    try {
      const value: unknown = JSON.parse(raw.slice(start, end));
      const collect = (item: unknown): void => {
        if (!item || typeof item !== "object") return;
        if (Array.isArray(item)) { item.forEach(collect); return; }
        const record = item as Record<string, unknown>;
        if (typeof record.src === "string") urls.push(record.src);
        for (const key of ["data", "chapter", "images", "pages"]) collect(record[key]);
      };
      collect(value);
    } catch { /* Other site scripts can assign JavaScript objects rather than JSON. */ }
  }
  return urls;
};

const scriptImages = (html: string) => {
  const root = document(html);
  const scripts = root.querySelectorAll("#single-chapter-js-extra, #single-chapter-js-before, script");
  const addresses: string[] = [];
  for (const script of scripts) {
    const raw = script.text.replace(/\\\//g, "/").replace(/&amp;/g, "&");
    for (const url of embeddedPages(raw)) addresses.push(absolute(url, BASE));
    for (const match of raw.matchAll(/https?:\/\/[^\s"'<>\\]+\.(?:jpe?g|png|webp|gif)(?:\?[^\s"'<>\\]*)?/gi))
      if (!/\b(?:logo|avatar|spinner|placeholder|icon|tracking|pixel)\b/i.test(match[0])) addresses.push(match[0]);
  }
  return [...new Set(addresses.filter(Boolean))];
};

export default class HentaiRead {
  static info: SourceInfo = {
    id: "en.hentairead", name: "HentaiRead", version: 4,
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
    return checkedHtml(await response.text(), `${BASE}/`);
  }
  private listing(html: string, page: number): PagedItemList {
    const root = document(html);
    const entries = new Map<string, Item>();
    for (const node of root.querySelectorAll(".manga-grid .manga-item, .manga-item")) {
      const link = node.querySelector("h3 a[href*='/hentai/'], a.manga-item__link, a[href*='/hentai/']");
      const url = href(link, BASE);
      if (!url || !pathFrom(url).startsWith("/hentai/")) continue;
      const id = idFrom(url, BASE);
      const title = titleOf(link) || titleOf(node.querySelector("h3")) || node.querySelector("img")?.getAttribute("alt")?.trim() || "";
      if (!title) continue;
      entries.set(id, { id, title,
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
    const heading = root.querySelector(".manga-titles h1, .manga-title h1, h1");
    const title = titleOf(heading) || root.querySelector("meta[property='og:title'], meta[name='twitter:title']")?.getAttribute("content")?.trim()
      || contentId.split("/").filter(Boolean).pop()?.replace(/-/g, " ") || "";
    return { title,
      coverImage: image(root.querySelector("img[fetchpriority='high'], .summary_image img"), BASE)
        || absolute(root.querySelector("meta[property='og:image'], meta[name='twitter:image']")?.getAttribute("content") ?? undefined, BASE)
        || image(root.querySelector(".manga-cover img, .manga-image img, .manga-item img"), BASE),
      rating: ContentRating.MATURE, webUrl: url,
      contentType: ContentType.MANGA, readingMode: ReadingMode.PAGED_MANGA,
      status: ContentStatus.UNKNOWN,
      summary: text(root.querySelector(".description-summary, .summary__content, .manga-description, #mangaDescription, .description"))
        || root.querySelector("meta[name='description'], meta[property='og:description']")?.getAttribute("content")?.trim() || "",
      ...(() => { const genres = root.querySelectorAll("a[href*='/genre/'], a[href*='/tag/']")
        .map((n) => ({ id: text(n), title: text(n) })).filter((tag) => tag.title);
        return genres.length ? { genres } : {}; })(),
    };
  }
  async getChapters(contentId: string): Promise<Chapter[]> {
    const root = document(await this.html(`/${contentId.replace(/^\/+|\/+$/g, "")}/`));
    const links = root.querySelectorAll("li.wp-manga-chapter a, .chapter-item a, .listing-chapters_wrap a[href*='/chapter-'], a[href*='/p/1/']");
    const chapters = chaptersFrom(links, BASE);
    // Galleries with no chapter list have a dedicated /p/1/ reading route.
    return chapters.length ? chapters : [{ id: `/${contentId.replace(/^\/+|\/+$/g, "")}/p/1/`, index: 0, number: 1, title: "Complete gallery", language: "en" }];
  }
  async getChapterPages(_contentId: string, chapterId: string): Promise<ChapterPage[]> {
    const html = await this.html(chapterId);
    const root = document(html);
    const images = root.querySelectorAll(".chapter-image-item img, .reading-content img, img.wp-manga-chapter-img");
    const urls = images.map((node) => image(node, BASE)).filter(Boolean);
    const pages = urls.length ? [...new Set(urls)] : scriptImages(html);
    if (!pages.length) throw new Error("HentaiRead returned no reader images");
    return pages.map((url) => ({ url: readerImage(url) }));
  }
}
