import { parse, type HTMLElement } from "node-html-parser";
import type { Chapter, Item, PagedItemList } from "@suwatte/toolchain/types";
import { ContentRating } from "@suwatte/toolchain/types";

export const adult = ContentRating.MATURE;
export const document = (html: string) => parse(html);
export const absolute = (value: string | undefined, base: string): string => {
  if (!value || value.startsWith("data:") || value.startsWith("javascript:")) return "";
  try { return new URL(value, base).href; } catch { return ""; }
};
export const href = (node: HTMLElement | null | undefined, base: string) =>
  absolute(node?.getAttribute("href") ?? undefined, base);
export const image = (node: HTMLElement | null | undefined, base: string) => {
  const img = node?.tagName === "IMG" ? node : node?.querySelector("img");
  const src = ["data-src", "data-lazy-src", "data-cfsrc", "data-original", "srcset", "src"]
    .map((key) => img?.getAttribute(key))
    .find((value) => value && !/^(data:|about:|#)/i.test(value));
  return absolute(src?.split(",")[0].trim().split(/\s+/)[0], base);
};
export const text = (node: HTMLElement | null | undefined) => node?.text.trim().replace(/\s+/g, " ") ?? "";
export const idFrom = (url: string, base: string): string => {
  const path = new URL(url, base).pathname;
  return path.replace(/^\/+|\/+$/g, "");
};
export const pageResult = (items: Item[], root: HTMLElement, page: number): PagedItemList => ({
  items,
  isLastPage: !root.querySelector("a[rel=next], a.nextpostslink, a.next.page-numbers, .nav-previous a, .pagination .next") || items.length === 0,
});
export function chaptersFrom(nodes: HTMLElement[], base: string): Chapter[] {
  const unique = new Map<string, { title: string; url: string; date?: Date }>();
  for (const node of nodes) {
    const url = href(node, base);
    if (!url || unique.has(url)) continue;
    const dateText = node.parentNode?.querySelector(".chapter-release-date, .chapter-release-dated, .date")?.text.trim();
    const date = dateText && !Number.isNaN(Date.parse(dateText)) ? new Date(dateText) : undefined;
    unique.set(url, { title: text(node), url, date });
  }
  const entries = Array.from(unique.values());
  // WordPress lists most recent first; chapter numbers remain stable as more are published.
  return entries.map((entry, index) => ({
    id: entry.url,
    index,
    number: Number(entry.title.match(/(?:chapter|ch\.?|episode|ep\.?)\s*([\d.]+)/i)?.[1]) || entries.length - index,
    title: entry.title,
    webUrl: entry.url,
    date: entry.date,
    language: "en",
  }));
}
export const assertOk = (status: number, url: string) => {
  if (status < 200 || status >= 300) throw new Error(`HTTP ${status} loading ${url}`);
};
