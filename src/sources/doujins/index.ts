"use httpclient";

import {
  ContentRating, ContentStatus, ContentType, ReadingMode,
  type Chapter, type ChapterPage, type Content, type HomePage, type Item,
  type ItemListRequest, type PagedItemList, type SearchRequest, type SourceInfo,
} from "@suwatte/toolchain/types";
import { absolute, assertOk, checkedHtml, document, href, idFrom, image, text } from "../../shared";

const BASE = "https://doujins.com";
type Folder = { link: string; name: string; artistList?: string; tags?: Array<{ tag: string }>; thumbnail2?: string };
const contentPath = (id: string) => `/${id.replace(/^\/+|\/+$/g, "")}/`;

export default class Doujins {
  static info: SourceInfo = {
    id: "en.doujins", name: "Doujins", version: 1, thumbnail: "doujins.png",
    website: BASE, languages: ["en"], rating: ContentRating.MATURE,
    minSupportedAppVersion: "7.0.0",
  };
  client = new HttpClient({ baseUrl: BASE, cloudflareResolutionURL: `${BASE}/`,
    headers: { Referer: `${BASE}/` }, rateLimit: { permits: 2, period: 1 } });
  getConfiguration() { return { imageReferer: `${BASE}/`, cloudflareResolutionURL: `${BASE}/`, useClientForImageRequests: true }; }
  private async get(path: string) {
    const response = await this.client.get(path);
    assertOk(response.status, absolute(path, BASE));
    return checkedHtml(await response.text(), `${BASE}/`);
  }
  private async listing(path: string): Promise<PagedItemList> {
    const root = document(await this.get(path));
    const items: Item[] = [];
    const seen = new Set<string>();
    for (const link of root.querySelectorAll(".thumbnail-doujin a.gallery-visited-from-favorites")) {
      if (link.parentNode?.classNames?.includes("premium-folder")) continue;
      const url = href(link, BASE);
      if (!url) continue;
      const id = idFrom(url, BASE);
      const title = text(link.querySelector("div.title .text")) || link.getAttribute("title")?.trim()
        || link.querySelector("img")?.getAttribute("alt")?.trim() || "";
      if (!title || seen.has(id)) continue;
      seen.add(id);
      items.push({ id, title, coverImage: image(link.querySelector("img"), BASE), webUrl: url, rating: ContentRating.MATURE });
    }
    return { items, isLastPage: !root.querySelector(".pagination li.page-item:last-child:not(.disabled), .pagination a[rel=next]") };
  }
  getHomePage = async (): Promise<HomePage> => ({ feeds: [
    { id: "popular", title: "Popular", content: { list: { key: "popular" } } },
    { id: "latest", title: "Latest", content: { list: { key: "latest" } } },
  ] });
  async getItemList(request: ItemListRequest, page: number): Promise<PagedItemList> {
    if (request.key !== "latest") return this.listing(`/top/month${page > 1 ? `?page=${page}` : ""}`);
    const end = Math.floor(Date.now() / 1000) + 86400 - 3 * 86400 * (Math.max(1, page) - 1);
    const start = end - 3 * 86400;
    const data = JSON.parse(await this.get(`/folders?start=${start}&end=${end}`)) as { folders?: Folder[] };
    const items = (data.folders ?? []).map((folder) => {
      const url = absolute(folder.link, BASE);
      return { id: idFrom(url, BASE), title: folder.name, coverImage: absolute(folder.thumbnail2, BASE),
        webUrl: url, rating: ContentRating.MATURE } satisfies Item;
    });
    return { items, isLastPage: items.length === 0 };
  }
  getSearchResults(request: SearchRequest, page: number) {
    const query = request.query?.trim();
    return query ? this.listing(`/searches?words=${encodeURIComponent(query)}&page=${page}&sort=`)
      : this.getItemList({ key: "popular" }, page);
  }
  async getContent(contentId: string): Promise<Content> {
    const url = absolute(contentPath(contentId), BASE);
    const root = document(await this.get(url));
    const headings = root.querySelectorAll(".folder-title a");
    const title = text(headings[headings.length - 1]) || text(root.querySelector("h1")) || contentId;
    const names = root.querySelectorAll(".gallery-artist a").map((n) => text(n)).filter(Boolean);
    const tags = root.querySelectorAll(".tag-area a").map((n) => text(n)).filter(Boolean);
    return { title, coverImage: image(root.querySelector(".thumbnail-doujin img, .folder-cover img, img.cover"), BASE)
      || absolute(root.querySelector("meta[property='og:image']")?.getAttribute("content") ?? undefined, BASE)
      || absolute(root.querySelector(".doujin[data-file]")?.getAttribute("data-file") ?? undefined, BASE),
      summary: tags.length ? `Tags: ${tags.join(", ")}` : "", webUrl: url,
      contentType: ContentType.MANGA, readingMode: ReadingMode.PAGED_MANGA,
      status: ContentStatus.COMPLETED, rating: ContentRating.MATURE,
      ...(names.length ? { credits: names.map((name) => ({ name, role: "Artist" })) } : {}),
      ...(tags.length ? { genres: tags.map((tag) => ({ id: tag, title: tag })) } : {}),
    };
  }
  async getChapters(contentId: string): Promise<Chapter[]> {
    return [{ id: contentId, index: 0, number: 1, title: "Complete gallery", language: "en",
      webUrl: absolute(contentPath(contentId), BASE) }];
  }
  async getChapterPages(_contentId: string, chapterId: string): Promise<ChapterPage[]> {
    const root = document(await this.get(contentPath(chapterId)));
    const urls = root.querySelectorAll(".doujin[data-file]").map((node) =>
      absolute(node.getAttribute("data-file")?.replace(/&amp;|amp;/g, "&") ?? undefined, BASE)).filter(Boolean);
    if (!urls.length) throw new Error("Doujins returned no reader images");
    return urls.map((url) => ({ url }));
  }
}
