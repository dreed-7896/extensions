"use httpclient";

import {
  ContentRating, ContentStatus, ContentType, ReadingMode,
  type Chapter, type ChapterPage, type Content, type HomePage, type Item,
  type ItemListRequest, type PagedItemList, type SearchRequest, type SourceInfo,
} from "@suwatte/toolchain/types";
import { absolute, assertOk, checkedHtml, document } from "../../shared";

const BASE = "https://omegascans.org";
const API = "https://api.omegascans.org";
type Series = { id: number; series_slug: string; title: string; thumbnail: string;
  description?: string; author?: string; studio?: string; status?: string; tags?: Array<{ name: string }> };
type ChapterDTO = { chapter_name: string; chapter_title?: string; chapter_slug: string; price?: number };
const slugOf = (contentId: string) => contentId.split("#")[0].replace(/^\/+|\/+$/g, "");
const asset = (src: string) => absolute(src, `${API}/`);
const queryString = (params: Record<string, string>) => Object.entries(params)
  .map(([key, value]) => `${encodeURIComponent(key)}=${encodeURIComponent(value)}`).join("&");

export default class OmegaScans {
  static info: SourceInfo = {
    id: "en.omegascans", name: "OmegaScans", version: 1, thumbnail: "omegascans.png",
    website: BASE, languages: ["en"], rating: ContentRating.MATURE,
    minSupportedAppVersion: "7.0.0",
  };
  client = new HttpClient({ baseUrl: API, cloudflareResolutionURL: `${API}/`,
    headers: { Referer: `${BASE}/` }, rateLimit: { permits: 3, period: 1 } });
  getConfiguration() { return { imageReferer: `${BASE}/`, cloudflareResolutionURL: `${API}/`, useClientForImageRequests: true }; }
  private async json<T>(path: string): Promise<T> {
    const response = await this.client.get(path);
    assertOk(response.status, absolute(path, API));
    return JSON.parse(checkedHtml(await response.text(), `${API}/`)) as T;
  }
  private status(raw?: string): ContentStatus {
    switch (raw?.toLowerCase()) {
      case "ongoing": return ContentStatus.ONGOING;
      case "completed": case "finished": return ContentStatus.COMPLETED;
      case "hiatus": return ContentStatus.HIATUS;
      case "dropped": case "cancelled": case "canceled": return ContentStatus.CANCELLED;
      default: return ContentStatus.UNKNOWN;
    }
  }
  private item(series: Series): Item {
    return { id: `${series.series_slug}#${series.id}`, title: series.title,
      coverImage: asset(series.thumbnail), webUrl: `${BASE}/series/${series.series_slug}`,
      rating: ContentRating.MATURE };
  }
  private async list(query: string, orderBy: string, page: number): Promise<PagedItemList> {
    const params = queryString({ query_string: query, status: "All", order: "desc", orderBy,
      series_type: "Comic", page: String(page), perPage: "12", tags_ids: "[]", adult: "true" });
    const data = await this.json<{ data?: Series[]; meta?: { current_page: number; last_page: number } }>(`/query?${params}`);
    return { items: (data.data ?? []).map((entry) => this.item(entry)),
      isLastPage: data.meta ? data.meta.current_page >= data.meta.last_page : !(data.data?.length) };
  }
  getHomePage = async (): Promise<HomePage> => ({ feeds: [
    { id: "popular", title: "Popular", content: { list: { key: "popular" } } },
    { id: "latest", title: "Latest", content: { list: { key: "latest" } } },
  ] });
  getItemList(request: ItemListRequest, page: number) {
    return this.list("", request.key === "latest" ? "latest" : "total_views", page);
  }
  getSearchResults(request: SearchRequest, page: number) {
    return this.list(request.query?.trim() ?? "", "total_views", page);
  }
  private details(slug: string) { return this.json<Series>(`/series/${encodeURIComponent(slug)}`); }
  async getContent(contentId: string): Promise<Content> {
    const slug = slugOf(contentId);
    const series = await this.details(slug);
    const genres = (series.tags ?? []).map((tag) => ({ id: tag.name, title: tag.name }));
    const credits = [series.author && { name: series.author, role: "Author" },
      series.studio && { name: series.studio, role: "Artist" }].filter((entry): entry is { name: string; role: string } => !!entry);
    return { title: series.title, coverImage: asset(series.thumbnail),
      webUrl: `${BASE}/series/${slug}`, rating: ContentRating.MATURE,
      contentType: ContentType.COMIC, readingMode: ReadingMode.VERTICAL,
      status: this.status(series.status), summary: document(series.description ?? "").text.trim(),
      ...(genres.length ? { genres } : {}), ...(credits.length ? { credits } : {}),
    };
  }
  async getChapters(contentId: string): Promise<Chapter[]> {
    const slug = slugOf(contentId);
    const id = Number(contentId.split("#")[1]) || (await this.details(slug)).id;
    const params = queryString({ page: "1", perPage: "1000", series_id: String(id) });
    const response = await this.json<{ data?: ChapterDTO[] }>(`/chapter/query?${params}`);
    return (response.data ?? []).filter((chapter) => !chapter.price).map((chapter, index) => ({
      id: `/chapter/${slug}/${chapter.chapter_slug}`, index,
      number: Number(chapter.chapter_name.match(/[\d.]+/)?.[0]) || index + 1,
      title: `${chapter.chapter_name}${chapter.chapter_title ? ` - ${chapter.chapter_title}` : ""}`,
      webUrl: `${BASE}/series/${slug}/${chapter.chapter_slug}`, language: "en",
    }));
  }
  async getChapterPages(_contentId: string, chapterId: string): Promise<ChapterPage[]> {
    const data = await this.json<{ paywall?: boolean; chapter?: { chapter_data?: { images?: string[] } } }>(chapterId);
    if (data.paywall && !data.chapter?.chapter_data) throw new Error("Paid chapter unavailable");
    const pages = data.chapter?.chapter_data?.images ?? [];
    if (!pages.length) throw new Error("OmegaScans returned no readable pages");
    return pages.map((src) => ({ url: asset(src) }));
  }
}
