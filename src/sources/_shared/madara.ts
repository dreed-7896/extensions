import {
  ContentRating,
  ContentStatus,
  type Chapter,
  type ChapterPage,
  type Content,
  type Item,
  type PagedItemList,
} from "@suwatte/toolchain/types";
import {
  absoluteUrl,
  fetchText,
  joinUrl,
  postEmpty,
  postForm,
  withQuery,
  type SourceHttpClient,
} from "./http";
import {
  allMatches,
  attr,
  collectBlocks,
  firstMatch,
  imageFromTag,
  metaContent,
  stripTags,
} from "./html";
import { matureItem } from "./item";

export type MadaraConfig = {
  baseUrl: string;
  mangaSubString?: string;
  useNewChapterEndpoint?: boolean;
  chapterUrlSuffix?: string;
  /** Popular / latest listing uses search-style `?s=&post_type=wp-manga&m_orderby=` */
  listViaSearch?: boolean;
  popularOrderBy?: string;
  latestOrderBy?: string;
  itemSelector?: string;
  searchItemSelector?: string;
  titleLinkSelector?: string;
  chapterListSelector?: string;
  pageImageSelector?: string;
  /** Source-owned HttpClient so Cloudflare cookies stick. */
  client?: SourceHttpClient;
};

const DEFAULTS = {
  mangaSubString: "manga",
  useNewChapterEndpoint: false,
  chapterUrlSuffix: "?style=list",
  listViaSearch: false,
  popularOrderBy: "views",
  latestOrderBy: "latest",
  itemSelector: "div.page-item-detail|div.manga__item|article.manga",
  searchItemSelector:
    "div.c-tabs-item__content|div.page-item-detail|div.manga__item",
  titleLinkSelector: "div.post-title a",
  chapterListSelector: "li.wp-manga-chapter",
  pageImageSelector:
    "div.page-break|li.blocks-gallery-item|div.reading-content img",
};

export type ResolvedMadara = Required<Omit<MadaraConfig, "client">> & {
  client?: SourceHttpClient;
};

export const resolveMadara = (config: MadaraConfig): ResolvedMadara => ({
  baseUrl: config.baseUrl.replace(/\/+$/, ""),
  mangaSubString: config.mangaSubString ?? DEFAULTS.mangaSubString,
  useNewChapterEndpoint:
    config.useNewChapterEndpoint ?? DEFAULTS.useNewChapterEndpoint,
  chapterUrlSuffix: config.chapterUrlSuffix ?? DEFAULTS.chapterUrlSuffix,
  listViaSearch: config.listViaSearch ?? DEFAULTS.listViaSearch,
  popularOrderBy: config.popularOrderBy ?? DEFAULTS.popularOrderBy,
  latestOrderBy: config.latestOrderBy ?? DEFAULTS.latestOrderBy,
  itemSelector: config.itemSelector ?? DEFAULTS.itemSelector,
  searchItemSelector: config.searchItemSelector ?? DEFAULTS.searchItemSelector,
  titleLinkSelector: config.titleLinkSelector ?? DEFAULTS.titleLinkSelector,
  chapterListSelector:
    config.chapterListSelector ?? DEFAULTS.chapterListSelector,
  pageImageSelector: config.pageImageSelector ?? DEFAULTS.pageImageSelector,
  client: config.client,
});

const fetchOpts = (cfg: ResolvedMadara, referer?: string) => ({
  client: cfg.client,
  referer: referer ?? `${cfg.baseUrl}/`,
  cloudflareResolutionURL: `${cfg.baseUrl}/`,
});
const searchPage = (page: number): string =>
  page <= 1 ? "" : `page/${page}/`;

export const mangaUrl = (cfg: ResolvedMadara, id: string): string => {
  const decoded = safeDecode(id);
  if (decoded.startsWith("/")) {
    return `${cfg.baseUrl}${decoded}`.replace(/([^:]\/)\/+/g, "$1");
  }
  return joinUrl(cfg.baseUrl, cfg.mangaSubString, decoded) + "/";
};

export const parseMangaId = (cfg: ResolvedMadara, href: string): string => {
  const cleaned = href.replace(/[?#].*$/, "").replace(/\/+$/, "");
  const marker = `/${cfg.mangaSubString}/`;
  const idx = cleaned.indexOf(marker);
  if (idx !== -1) {
    return cleaned.slice(idx + marker.length).split("/")[0] ?? cleaned;
  }
  const path = cleaned.replace(/^https?:\/\/[^/]+/i, "");
  return path.startsWith("/") ? path : `/${path}`;
};

export const parseChapterId = (
  cfg: ResolvedMadara,
  href: string,
  mangaId: string,
): string => {
  const cleaned = href.replace(/[?#].*$/, "").replace(/\/+$/, "");
  const rawManga = safeDecode(mangaId);
  const marker = `/${cfg.mangaSubString}/${rawManga}/`;
  const idx = cleaned.indexOf(marker);
  if (idx !== -1) return cleaned.slice(idx + marker.length);
  const path = cleaned.replace(/^https?:\/\/[^/]+/i, "");
  return path.startsWith("/") ? path : cleaned.split("/").pop() ?? cleaned;
};

const safeDecode = (value: string): string => {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
};

const selectorToOpenRegex = (selector: string): RegExp => {
  // Supports simple "tag.class" or "tag#id" alternatives separated by `|`.
  const parts = selector.split("|").map((part) => {
    const trimmed = part.trim();
    const tagMatch = trimmed.match(/^([a-z0-9]+)/i);
    const tag = tagMatch?.[1] ?? "div";
    const classes = [...trimmed.matchAll(/\.([a-z0-9_-]+)/gi)].map((m) => m[1]);
    const id = trimmed.match(/#([a-z0-9_-]+)/i)?.[1];
    let pattern = `<${tag}\\b[^>]*`;
    if (id) pattern += `id=["'][^"']*${id}[^"']*["'][^>]*`;
    for (const cls of classes) {
      pattern += `(?=class=["'][^"']*\\b${cls}\\b)`;
    }
    pattern += `[^>]*>`;
    return pattern;
  });
  return new RegExp(parts.join("|"), "gi");
};

const firstImgIn = (block: string): string => {
  const img = firstMatch(block, /(<img\b[^>]*>)/i);
  return img ? imageFromTag(img) : "";
};

const titleFromBlock = (block: string, titleSelector: string): {
  title: string;
  href: string;
} => {
  // Prefer anchors under post-title, then any title-ish link.
  const patterns =
    titleSelector.includes("post-title")
      ? [
          /<div[^>]*class=["'][^"']*post-title[^"']*["'][^>]*>[\s\S]*?<a\b([^>]*)>([\s\S]*?)<\/a>/i,
          /<a\b([^>]*class=["'][^"']*manga-item__link[^"']*["'][^>]*)>([\s\S]*?)<\/a>/i,
          /<a\b([^>]*)>([\s\S]*?)<\/a>/i,
        ]
      : [
          /<a\b([^>]*class=["'][^"']*manga-item__link[^"']*["'][^>]*)>([\s\S]*?)<\/a>/i,
          /<div[^>]*class=["'][^"']*post-title[^"']*["'][^>]*>[\s\S]*?<a\b([^>]*)>([\s\S]*?)<\/a>/i,
          /<a\b([^>]*)>([\s\S]*?)<\/a>/i,
        ];

  for (const pattern of patterns) {
    const match = pattern.exec(block);
    if (!match) continue;
    const tagAttrs = match[1] ?? "";
    const href = attr(`<a ${tagAttrs}>`, "href") ?? "";
    const title =
      stripTags(match[2] ?? "") ||
      attr(`<a ${tagAttrs}>`, "title") ||
      "";
    if (href && title) return { title, href };
  }
  return { title: "", href: "" };
};

export const parseListing = (
  cfg: ResolvedMadara,
  html: string,
  selector: string,
): Item[] => {
  const open = selectorToOpenRegex(selector);
  const tagName =
    selector
      .split("|")[0]
      ?.trim()
      .match(/^([a-z0-9]+)/i)?.[1]
      ?.toLowerCase() ?? "div";
  const blocks = collectBlocks(html, open, tagName);
  const items: Item[] = [];
  const seen = new Set<string>();

  for (const block of blocks) {
    const { title, href } = titleFromBlock(block, cfg.titleLinkSelector);
    if (!title || !href) continue;
    const id = parseMangaId(cfg, href);
    if (!id || seen.has(id)) continue;
    seen.add(id);
    const cover = absoluteUrl(cfg.baseUrl, firstImgIn(block));
    items.push(
      matureItem({
        id,
        title,
        coverImage: cover || undefined,
        webUrl: absoluteUrl(cfg.baseUrl, href),
      }),
    );
  }
  return items;
};

export const hasNextPage = (html: string): boolean =>
  /<(?:div|nav|a)\b[^>]*(?:nav-previous|navigation-ajax|nextpostslink|rel=["']next["'])/i.test(
    html,
  ) || /<a\b[^>]*rel=["']next["']/i.test(html);

export const listUrl = (
  cfg: ResolvedMadara,
  page: number,
  orderBy: string,
): string => {
  if (cfg.listViaSearch) {
    return withQuery(`${cfg.baseUrl}/${searchPage(page)}`, {
      s: "",
      post_type: "wp-manga",
      m_orderby: orderBy,
    });
  }
  return withQuery(
    joinUrl(cfg.baseUrl, cfg.mangaSubString, searchPage(page)) +
      (searchPage(page) ? "" : "/"),
    { m_orderby: orderBy },
  );
};

export const searchUrl = (
  cfg: ResolvedMadara,
  page: number,
  query: string,
  orderBy?: string,
): string =>
  withQuery(`${cfg.baseUrl}/${searchPage(page)}`, {
    s: query,
    post_type: "wp-manga",
    m_orderby: orderBy,
  });

export const fetchListing = async (
  cfg: ResolvedMadara,
  page: number,
  orderBy: string,
): Promise<PagedItemList> => {
  const html = await fetchText(listUrl(cfg, page, orderBy), fetchOpts(cfg));
  const items = parseListing(cfg, html, cfg.itemSelector);
  return { items, isLastPage: !hasNextPage(html) || items.length === 0 };
};

export const fetchSearch = async (
  cfg: ResolvedMadara,
  page: number,
  query: string,
  orderBy?: string,
): Promise<PagedItemList> => {
  const html = await fetchText(
    searchUrl(cfg, page, query, orderBy),
    fetchOpts(cfg),
  );
  const items = parseListing(
    cfg,
    html,
    query.trim() ? cfg.searchItemSelector : cfg.itemSelector,
  );
  return { items, isLastPage: !hasNextPage(html) || items.length === 0 };
};

const parseStatus = (raw: string): ContentStatus => {
  const value = raw.toLowerCase();
  if (/complete|完結|completo|terminé|finalizado/.test(value)) {
    return ContentStatus.COMPLETED;
  }
  if (/ongoing|on.?going|en curso|em andamento|en cours/.test(value)) {
    return ContentStatus.ONGOING;
  }
  if (/hiatus|on.?hold|pause/.test(value)) return ContentStatus.HIATUS;
  if (/cancel|drop/.test(value)) return ContentStatus.CANCELLED;
  return ContentStatus.UNKNOWN;
};

const textsMatching = (html: string, openPattern: RegExp): string[] => {
  const blocks = allMatches(html, openPattern);
  return blocks.map(stripTags).filter(Boolean);
};

export const fetchMadaraContent = async (
  cfg: ResolvedMadara,
  contentId: string,
): Promise<Content> => {
  const url = mangaUrl(cfg, contentId);
  const html = await fetchText(url, fetchOpts(cfg));

  const title =
    stripTags(
      firstMatch(
        html,
        /<(?:h1|h2|h3)\b[^>]*class=["'][^"']*post-title[^"']*["'][^>]*>([\s\S]*?)<\/(?:h1|h2|h3)>/i,
      ) ??
        firstMatch(html, /<h1\b[^>]*>([\s\S]*?)<\/h1>/i) ??
        "",
    ) || contentId;

  const authors = textsMatching(
    html,
    /<div[^>]*class=["'][^"']*author-content[^"']*["'][^>]*>[\s\S]*?<a\b[^>]*>([\s\S]*?)<\/a>/gi,
  );
  const artists = textsMatching(
    html,
    /<div[^>]*class=["'][^"']*artist-content[^"']*["'][^>]*>[\s\S]*?<a\b[^>]*>([\s\S]*?)<\/a>/gi,
  );
  const genres = textsMatching(
    html,
    /<div[^>]*class=["'][^"']*genres-content[^"']*["'][^>]*>[\s\S]*?<a\b[^>]*>([\s\S]*?)<\/a>/gi,
  );

  let summary =
    stripTags(
      firstMatch(
        html,
        /<div[^>]*class=["'][^"']*description-summary[^"']*["'][^>]*>([\s\S]*?)<\/div>/i,
      ) ??
        firstMatch(
          html,
          /<div[^>]*class=["'][^"']*summary__content[^"']*["'][^>]*>([\s\S]*?)<\/div>/i,
        ) ??
        "",
    ) || undefined;

  const alt =
    stripTags(
      firstMatch(
        html,
        /Alternative[^<]*<\/[^>]+>[\s\S]*?<div[^>]*class=["'][^"']*summary-content[^"']*["'][^>]*>([\s\S]*?)<\/div>/i,
      ) ?? "",
    ) || undefined;
  if (alt && alt !== "-") {
    summary = summary ? `${summary}\n\nAlternative: ${alt}` : `Alternative: ${alt}`;
  }

  let cover =
    absoluteUrl(
      cfg.baseUrl,
      firstImgIn(
        firstMatch(
          html,
          /(<div[^>]*class=["'][^"']*summary_image[^"']*["'][^>]*>[\s\S]*?<\/div>)/i,
        ) ?? "",
      ),
    ) ||
    metaContent(html, "og:image") ||
    metaContent(html, "twitter:image") ||
    "";

  const statusText =
    stripTags(
      firstMatch(
        html,
        /(?:Status|Estado)[\s\S]{0,120}?<div[^>]*class=["'][^"']*summary-content[^"']*["'][^>]*>([\s\S]*?)<\/div>/i,
      ) ?? "",
    ) || "";

  const credits = [
    ...authors.map((name) => ({ name, role: "Author" })),
    ...artists
      .filter((name) => !authors.includes(name))
      .map((name) => ({ name, role: "Artist" })),
  ];

  return {
    title,
    coverImage: cover,
    webUrl: url,
    rating: ContentRating.MATURE,
    status: parseStatus(statusText),
    summary,
    additionalTitles: alt && alt !== "-" ? [alt] : undefined,
    genres: genres.map((title) => ({
      id: title.toLowerCase().replace(/\s+/g, "-"),
      title,
    })),
    credits: credits.length ? credits : undefined,
  };
};

const parseChapterNumber = (title: string, id: string): number => {
  const match =
    title.match(/(?:chapter|episode|ch)[.\s-]*(\d+(?:\.\d+)?)/i) ??
    id.match(/(?:chapter|episode|ch)-(\d+(?:[.-]\d+)?)/i) ??
    title.match(/(\d+(?:\.\d+)?)/);
  if (!match?.[1]) return -1;
  return Number.parseFloat(match[1].replace("-", "."));
};

const parseRelativeDate = (text: string): Date | undefined => {
  const value = text.toLowerCase().trim();
  if (!value) return undefined;
  if (value.startsWith("today")) {
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    return d;
  }
  if (value.startsWith("yesterday")) {
    const d = new Date();
    d.setDate(d.getDate() - 1);
    d.setHours(0, 0, 0, 0);
    return d;
  }
  const relative = value.match(
    /(\d+)\s*(second|minute|hour|day|week|month|year)s?\s*ago/i,
  );
  if (relative) {
    const amount = Number(relative[1]);
    const unit = relative[2]!.toLowerCase();
    const d = new Date();
    switch (unit) {
      case "second":
        d.setSeconds(d.getSeconds() - amount);
        break;
      case "minute":
        d.setMinutes(d.getMinutes() - amount);
        break;
      case "hour":
        d.setHours(d.getHours() - amount);
        break;
      case "day":
        d.setDate(d.getDate() - amount);
        break;
      case "week":
        d.setDate(d.getDate() - amount * 7);
        break;
      case "month":
        d.setMonth(d.getMonth() - amount);
        break;
      case "year":
        d.setFullYear(d.getFullYear() - amount);
        break;
    }
    return d;
  }
  const parsed = Date.parse(text);
  return Number.isNaN(parsed) ? undefined : new Date(parsed);
};

const parseChapterBlocks = (
  cfg: ResolvedMadara,
  html: string,
  contentId: string,
): Chapter[] => {
  const open = selectorToOpenRegex(cfg.chapterListSelector);
  const tag =
    cfg.chapterListSelector.match(/^([a-z0-9]+)/i)?.[1]?.toLowerCase() ?? "li";
  const blocks = collectBlocks(html, open, tag);
  const chapters: Chapter[] = [];

  for (const [index, block] of blocks.entries()) {
    const linkMatch =
      /<a\b([^>]*)>([\s\S]*?)<\/a>/i.exec(block) ??
      null;
    if (!linkMatch) continue;
    const href = attr(`<a ${linkMatch[1]}>`, "href") ?? "";
    if (!href) continue;
    const title = stripTags(linkMatch[2] ?? "") || `Chapter ${index + 1}`;
    const id = parseChapterId(cfg, href, contentId);
    if (!id) continue;
    const dateText =
      stripTags(
        firstMatch(
          block,
          /<span[^>]*class=["'][^"']*chapter-release-date[^"']*["'][^>]*>([\s\S]*?)<\/span>/i,
        ) ??
          firstMatch(block, /title=["']([^"']+)["']/i) ??
          "",
      ) || "";
    chapters.push({
      id,
      index,
      number: parseChapterNumber(title, id),
      title,
      language: "en",
      date: parseRelativeDate(dateText),
      webUrl: absoluteUrl(cfg.baseUrl, href),
    });
  }
  return chapters;
};

export const fetchMadaraChapters = async (
  cfg: ResolvedMadara,
  contentId: string,
): Promise<Chapter[]> => {
  const url = mangaUrl(cfg, contentId);
  let html = await fetchText(url, fetchOpts(cfg));
  let chapters = parseChapterBlocks(cfg, html, contentId);

  if (chapters.length === 0) {
    const postId =
      firstMatch(
        html,
        /<div[^>]*id=["']manga-chapters-holder["'][^>]*data-id=["'](\d+)["']/i,
      ) ??
      firstMatch(html, /data-id=["'](\d+)["'][^>]*id=["']manga-chapters-holder/i);
    const order = cfg.useNewChapterEndpoint
      ? (["new", "legacy"] as const)
      : (["legacy", "new"] as const);

    for (const kind of order) {
      try {
        if (kind === "new") {
          html = await postEmpty(`${url.replace(/\/+$/, "")}/ajax/chapters`, {
            ...fetchOpts(cfg, url),
            headers: { "X-Requested-With": "XMLHttpRequest" },
          });
        } else if (postId) {
          html = await postForm(
            `${cfg.baseUrl}/wp-admin/admin-ajax.php`,
            {
              action: "manga_get_chapters",
              manga: postId,
            },
            {
              ...fetchOpts(cfg),
              headers: { "X-Requested-With": "XMLHttpRequest" },
            },
          );
        } else {
          continue;
        }
        chapters = parseChapterBlocks(cfg, html, contentId);
        if (chapters.length) break;
      } catch {
        // try next endpoint
      }
    }
  }

  return chapters;
};

export const chapterPageUrl = (
  cfg: ResolvedMadara,
  contentId: string,
  chapterId: string,
): string => {
  const decoded = safeDecode(chapterId);
  let base: string;
  if (decoded.startsWith("/")) {
    base = `${cfg.baseUrl}${decoded}`;
  } else if (decoded.startsWith("http")) {
    base = decoded;
  } else {
    base = `${mangaUrl(cfg, contentId).replace(/\/+$/, "")}/${decoded}`;
  }
  const path = base.replace(/\/+$/, "") + "/";
  if (!cfg.chapterUrlSuffix) return path;
  if (cfg.chapterUrlSuffix.startsWith("?")) {
    return path.replace(/\/$/, "") + "/" + cfg.chapterUrlSuffix;
  }
  return path + cfg.chapterUrlSuffix.replace(/^\//, "");
};

export const fetchMadaraPages = async (
  cfg: ResolvedMadara,
  contentId: string,
  chapterId: string,
): Promise<ChapterPage[]> => {
  const url = chapterPageUrl(cfg, contentId, chapterId);
  const html = await fetchText(url, fetchOpts(cfg, mangaUrl(cfg, contentId)));

  const pages: string[] = [];
  const imgTags = allMatches(html, /(<img\b[^>]*>)/gi);
  for (const tag of imgTags) {
    // Prefer images inside reading content / page-break containers.
    const src = imageFromTag(tag);
    if (!src) continue;
    if (
      /avatar|logo|icon|ads|banner|emoji|spinner|loading/i.test(src) &&
      !/wp-content\/uploads|manga|chapter|comic/i.test(src)
    ) {
      continue;
    }
    const abs = absoluteUrl(cfg.baseUrl, src);
    if (abs && !pages.includes(abs)) pages.push(abs);
  }

  // Narrow to page-break containers when present.
  const breakBlocks = collectBlocks(
    html,
    /<div\b[^>]*class=["'][^"']*page-break[^"']*["'][^>]*>/gi,
    "div",
  );
  if (breakBlocks.length) {
    const narrowed: string[] = [];
    for (const block of breakBlocks) {
      const src = absoluteUrl(cfg.baseUrl, firstImgIn(block));
      if (src && !narrowed.includes(src)) narrowed.push(src);
    }
    if (narrowed.length) {
      return narrowed.map((pageUrl) => ({ url: pageUrl }));
    }
  }

  if (!pages.length) {
    throw new Error("No pages found for this chapter");
  }
  return pages.map((pageUrl) => ({ url: pageUrl }));
};
