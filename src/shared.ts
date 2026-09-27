import { parse, type HTMLElement } from "node-html-parser";
import type { Chapter, Item, PagedItemList } from "@suwatte/toolchain/types";
import { ContentRating } from "@suwatte/toolchain/types";

export const adult = ContentRating.MATURE;
export const document = (html: string) => parse(html);
export const pathFrom = (url: string): string =>
  (url.replace(/^https?:\/\/[^/?#]+/i, "").split(/[?#]/)[0] || "/");
export const absolute = (value: string | undefined, base: string): string => {
  if (!value) return "";
  const raw = value.trim();
  if (/^https?:\/\//i.test(raw)) return raw;
  if (raw.startsWith("//")) return `${base.match(/^https?:/i)?.[0] ?? "https:"}${raw}`;
  if (!raw || /^[a-z][a-z\d+.-]*:/i.test(raw) || raw.startsWith("#")) return "";
  const origin = base.match(/^https?:\/\/[^/?#]+/i)?.[0];
  if (!origin) return "";
  const basePath = pathFrom(base);
  const relative = raw.startsWith("/") ? raw
    : raw.startsWith("?") ? `${basePath}${raw}`
    : `${basePath.replace(/\/[^/]*$/, "/")}${raw}`;
  const boundary = relative.search(/[?#]/);
  const pathname = boundary < 0 ? relative : relative.slice(0, boundary);
  const suffix = boundary < 0 ? "" : relative.slice(boundary);
  const segments: string[] = [];
  for (const segment of pathname.split("/")) {
    if (segment === "..") segments.pop();
    else if (segment && segment !== ".") segments.push(segment);
  }
  return `${origin}/${segments.join("/")}${pathname.endsWith("/") && segments.length ? "/" : ""}${suffix}`;
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
  return pathFrom(absolute(url, base)).replace(/^\/+|\/+$/g, "");
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
