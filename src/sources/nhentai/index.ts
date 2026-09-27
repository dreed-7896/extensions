"use httpclient";

import {
  ContentRating, ContentStatus, ContentType, ReadingMode,
  type Chapter, type ChapterPage, type Content, type HomePage,
  type Item, type ItemListRequest, type PagedItemList, type SearchRequest,
  type SortOptions, type SourceInfo,
} from "@suwatte/toolchain/types";
import { assertOk } from "../../shared";

const BASE = "https://nhentai.net";
const API = `${BASE}/api/v2`;
type Tag = { name: string; type: string };
type Listing = { id: number; english_title: string; japanese_title?: string; thumbnail: string };
type Page = { path: string };
type Gallery = {
  id: number; title: { english: string; japanese?: string; pretty?: string };
  cover: { path: string }; pages: Page[]; tags: Tag[];
  num_pages: number; num_favorites: number; upload_date: number;
};
type SearchResponse = { result: Listing[]; num_pages: number; total?: number };
const imageUrl = (path: string, thumb = false) =>
  /^https?:\/\//.test(path) ? path : `https://${thumb ? "t" : "i"}.nhentai.net/${path.replace(/^\//, "")}`;

export default class NHentai {
  static info: SourceInfo = {
    id: "en.nhentai", name: "NHentai", version: 3,
    website: BASE, languages: ["en"], rating: ContentRating.MATURE,
    minSupportedAppVersion: "7.0.0",
  };
  client = new HttpClient({ baseUrl: BASE, rateLimit: { permits: 2, period: 1 },
    cloudflareResolutionURL: `${BASE}/`,
    headers: { Referer: `${BASE}/` },
  });
  getConfiguration() { return { imageReferer: `${BASE}/`, cloudflareResolutionURL: `${BASE}/`, useClientForImageRequests: true }; }

  private async gallery(id: string): Promise<Gallery> {
    const path = `/api/v2/galleries/${encodeURIComponent(id)}`;
    const res = await this.client.get(path); assertOk(res.status, `${BASE}${path}`);
    return res.json<Gallery>();
  }
  private listItem(g: Listing): Item {
    return { id: String(g.id), title: g.english_title || g.japanese_title || `#${g.id}`,
      coverImage: imageUrl(g.thumbnail, true), webUrl: `${BASE}/g/${g.id}/`, rating: ContentRating.MATURE };
  }
  async getSearchResults(request: SearchRequest, page: number): Promise<PagedItemList> {
    const q = request.query?.trim() ?? "";
    if (/^\d+$/.test(q)) {
      if (page > 1) return { items: [], isLastPage: true };
      const g = await this.gallery(q);
      return { items: [{ id: q, title: g.title.english || g.title.pretty || `#${q}`,
        coverImage: imageUrl(g.cover.path, true), rating: ContentRating.MATURE }], isLastPage: true };
    }
    const sort = ["date", "popular-today", "popular-week", "popular"].includes(request.sort?.key ?? "")
      ? request.sort!.key : "date";
    const path = `/api/v2/search?query=${encodeURIComponent(q || " ")}&page=${Math.max(1, page)}&sort=${sort}`;
    const res = await this.client.get(path); assertOk(res.status, `${BASE}${path}`);
    const data = await res.json<SearchResponse>();
    return { items: data.result.map((g) => this.listItem(g)), isLastPage: page >= data.num_pages,
      total: data.total };
  }
  async getSortOptions(): Promise<SortOptions> {
    return { options: [
      { id: "date", title: "Latest" }, { id: "popular-today", title: "Popular today" },
      { id: "popular-week", title: "Popular this week" }, { id: "popular", title: "Most popular" },
    ], disableOrdering: true };
  }
  async getHomePage(): Promise<HomePage> {
    return { feeds: [
      { id: "latest", title: "Latest", content: { list: { key: "date" } } },
      { id: "popular-week", title: "Popular this week", content: { list: { key: "popular-week" } } },
      { id: "popular", title: "Most popular", content: { list: { key: "popular" } } },
    ] };
  }
  async getItemList(request: ItemListRequest, page: number): Promise<PagedItemList> {
    return this.getSearchResults({ sort: { key: request.key || "date" } }, page);
  }
  async getContent(contentId: string): Promise<Content> {
    const g = await this.gallery(contentId);
    const genres = g.tags.filter((t) => t.type === "tag").map((t) => ({ id: t.name, title: t.name }));
    const artists = g.tags.filter((t) => t.type === "artist").map((t) => ({ name: t.name, role: "Artist" }));
    return { title: g.title.english || g.title.pretty || g.title.japanese || `#${g.id}`,
      coverImage: imageUrl(g.cover.path, true), rating: ContentRating.MATURE,
      status: ContentStatus.COMPLETED, contentType: ContentType.MANGA,
      readingMode: ReadingMode.PAGED_MANGA, webUrl: `${BASE}/g/${g.id}/`,
      summary: `${g.num_pages} pages · ${g.num_favorites} favorites`,
      ...(genres.length ? { genres } : {}), ...(artists.length ? { credits: artists } : {}),
      ...(g.title.japanese ? { additionalTitles: [g.title.japanese] } : {}),
    };
  }
  async getChapters(contentId: string): Promise<Chapter[]> {
    const g = await this.gallery(contentId);
    return [{ id: String(g.id), index: 0, number: 1, title: "Complete gallery",
      date: new Date(g.upload_date * 1000), webUrl: `${BASE}/g/${g.id}/` }];
  }
  async getChapterPages(_contentId: string, chapterId: string): Promise<ChapterPage[]> {
    const g = await this.gallery(chapterId);
    return g.pages.map((p) => ({ url: imageUrl(p.path) }));
  }
}
