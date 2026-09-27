"use httpclient";

import {
  ContentRating,
  ContentStatus,
  PickerFilter,
  SearchFilter,
  SelectFilter,
  TextFilter,
  type Chapter,
  type ChapterPage,
  type Content,
  type HomePage,
  type ItemListRequest,
  type PagedItemList,
  type SearchRequest,
  type SortOptions,
  type SourceConfiguration,
  type SourceInfo,
} from "@suwatte/toolchain/types";
import {
  allMatches,
  attr,
  collectBlocks,
  firstMatch,
  imageFromTag,
  stripTags,
} from "../_shared/html";
import { createProtectedClient } from "../_shared/client";
import { assertCloudflareCleared } from "../_shared/cloudflare";
import { absoluteUrl, fetchText, postForm } from "../_shared/http";
import { matureItem } from "../_shared/item";
import { CATEGORIES, TAGS } from "./tags";

const BASE = "https://hentai2read.com";
const IMAGE_BASE = "https://static.hentaicdn.com/hentai";
const PREFIX_ID = "id:";

const pagesUrlPattern = /'images'\s*:\s*\["(.*?),?"]/;

/** Session-scoped next-page URL for advanced search pagination. */
let nextSearchPage: string | undefined;

const parseMangaId = (href: string): string => {
  const cleaned = href.replace(/[?#].*$/, "").replace(/\/+$/, "");
  const path = cleaned.replace(/^https?:\/\/[^/]+/i, "");
  const segment = path.split("/").filter(Boolean)[0];
  return segment ?? cleaned;
};

const parseListing = (html: string) => {
  const blocks = collectBlocks(
    html,
    /<div\b[^>]*class=["'][^"']*\bbook-grid-item\b[^"']*["'][^>]*>/gi,
    "div",
  );
  const items = [];
  const seen = new Set<string>();
  for (const block of blocks) {
    const overlay =
      /<div\b[^>]*class=["'][^"']*overlay-title[^"']*["'][^>]*>[\s\S]*?<a\b([^>]*)>([\s\S]*?)<\/a>/i.exec(
        block,
      );
    if (!overlay) continue;
    const href = attr(`<a ${overlay[1]}>`, "href") ?? "";
    const title = stripTags(overlay[2] ?? "");
    if (!href || !title) continue;
    const id = parseMangaId(href);
    if (!id || seen.has(id)) continue;
    seen.add(id);
    const img = firstMatch(block, /(<img\b[^>]*>)/i);
    const cover = img
      ? absoluteUrl(BASE, imageFromTag(img))
      : undefined;
    items.push(
      matureItem({
        id,
        title,
        coverImage: cover,
        webUrl: absoluteUrl(BASE, href),
      }),
    );
  }
  return items;
};

const hasNext = (html: string): boolean => {
  const next = firstMatch(
    html,
    /<a\b[^>]*id=["']js-linkNext["'][^>]*href=["']([^"']+)["']/i,
  );
  if (next) {
    nextSearchPage = absoluteUrl(BASE, next);
    return true;
  }
  nextSearchPage = undefined;
  return false;
};

const parseStatus = (raw: string): ContentStatus => {
  if (/ongoing/i.test(raw)) return ContentStatus.ONGOING;
  if (/completed/i.test(raw)) return ContentStatus.COMPLETED;
  return ContentStatus.UNKNOWN;
};

const parseChapterDate = (date: string): Date | undefined => {
  const value = Number.parseInt(date.replace(/\D/g, ""), 10);
  if (Number.isNaN(value)) return undefined;
  const d = new Date();
  if (date.includes("second")) d.setSeconds(d.getSeconds() - value);
  else if (date.includes("minute")) d.setMinutes(d.getMinutes() - value);
  else if (date.includes("hour")) d.setHours(d.getHours() - value);
  else if (date.includes("day")) d.setDate(d.getDate() - value);
  else if (date.includes("week")) d.setDate(d.getDate() - value * 7);
  else if (date.includes("month")) d.setMonth(d.getMonth() - value);
  else if (date.includes("year")) d.setFullYear(d.getFullYear() - value);
  else return undefined;
  return d;
};

const listLiTexts = (infoHtml: string, label: string): string[] => {
  const pattern = new RegExp(
    `<li\\b[^>]*>\\s*(?:<b[^>]*>)?\\s*${label}[\\s\\S]*?<\\/b>\\s*([\\s\\S]*?)<\\/li>`,
    "i",
  );
  const block = firstMatch(infoHtml, pattern);
  if (!block) return [];
  const links = allMatches(block, /<a\b[^>]*>([\s\S]*?)<\/a>/gi).map(stripTags);
  if (links.length) return links.filter((t) => t && t !== "-");
  const text = stripTags(block);
  return text && text !== "-" ? [text] : [];
};

export default class Target {
  client = createProtectedClient(`${BASE}/`);

  static info: SourceInfo = {
    id: "en.hentai2read",
    name: "Hentai2Read",
    version: 1.4,
    website: BASE,
    thumbnail: "hentai2read.png",
    languages: ["en"],
    rating: ContentRating.MATURE,
  };

  getConfiguration = (): SourceConfiguration =>
    ({
      imageReferer: `${BASE}/`,
      cloudflareResolutionURL: `${BASE}/`,
      useClientForImageRequests: true,
    }) as SourceConfiguration;

  getHomePage = async (): Promise<HomePage> => {
    await assertCloudflareCleared(this.client, `${BASE}/`);
    return {
      feeds: [
        {
          id: "popular",
          title: "Most Popular",
          content: { list: { key: "popular", disableSorting: true } },
        },
        {
          id: "latest",
          title: "Last Updated",
          content: { list: { key: "latest", disableSorting: true } },
        },
      ],
    };
  };

  getSortOptions = async (): Promise<SortOptions> => ({
    options: [
      { id: "alphabetical", title: "Alphabetical" },
      { id: "most popular", title: "Most Popular" },
      { id: "last updated", title: "Last Updated" },
    ],
    disableOrdering: true,
  });

  getSearchFilters = async () => [
    SearchFilter(
      "nameMode",
      "Manga Name",
      PickerFilter([
        { id: "0", title: "Contains" },
        { id: "1", title: "Starts With" },
        { id: "2", title: "Ends With" },
      ]),
    ),
    SearchFilter("artist", "Artist", TextFilter()),
    SearchFilter(
      "artistMode",
      "Artist Name",
      PickerFilter([
        { id: "0", title: "Contains" },
        { id: "1", title: "Starts With" },
        { id: "2", title: "Ends With" },
      ]),
    ),
    SearchFilter("character", "Character", TextFilter()),
    SearchFilter(
      "characterMode",
      "Character Name",
      PickerFilter([
        { id: "0", title: "Contains" },
        { id: "1", title: "Starts With" },
        { id: "2", title: "Ends With" },
      ]),
    ),
    SearchFilter("year", "Release Year", TextFilter()),
    SearchFilter(
      "yearMode",
      "Release Year Mode",
      PickerFilter([
        { id: "0", title: "In" },
        { id: "1", title: "Before" },
        { id: "2", title: "After" },
      ]),
    ),
    SearchFilter(
      "status",
      "Status",
      PickerFilter([
        { id: "0", title: "Any" },
        { id: "1", title: "Completed" },
        { id: "2", title: "Ongoing" },
      ]),
    ),
    SearchFilter(
      "tagMode",
      "Tag Search Mode",
      PickerFilter([
        { id: "0", title: "AND" },
        { id: "1", title: "OR" },
      ]),
    ),
    SearchFilter("categories", "Categories", SelectFilter(CATEGORIES, true)),
    SearchFilter("tags", "Tags", SelectFilter(TAGS, true)),
  ];

  getItemList = async (
    request: ItemListRequest,
    page: number,
  ): Promise<PagedItemList> => {
    const sort =
      request.key === "latest" ? "last-updated" : "most-popular";
    const url = `${BASE}/hentai-list/all/any/all/${sort}/${page}/`;
    const html = await fetchText(url, { client: this.client, referer: `${BASE}/`, cloudflareResolutionURL: `${BASE}/` });
    const items = parseListing(html);
    return { items, isLastPage: !hasNext(html) || items.length === 0 };
  };

  getSearchResults = async (
    request: SearchRequest,
    page: number,
  ): Promise<PagedItemList> => {
    const query = request.query?.trim() ?? "";

    if (query.startsWith("https://")) {
      let host = "";
      let path = "";
      const match = query.match(/^https?:\/\/([^/?#]+)(\/[^?#]*)?/i);
      if (!match) throw new Error("Unsupported url");
      host = match[1] ?? "";
      path = match[2] ?? "/";
      if (host !== "hentai2read.com" && host !== "www.hentai2read.com") {
        throw new Error("Unsupported url");
      }
      const id = path.split("/").filter(Boolean)[0];
      if (!id) throw new Error("Unsupported url");
      return this.getSearchResults(
        { ...request, query: `${PREFIX_ID}${id}` },
        page,
      );
    }

    if (query.startsWith(PREFIX_ID)) {
      const id = query.slice(PREFIX_ID.length);
      const content = await this.getContent(id);
      return {
        items: [
          matureItem({
            id,
            title: content.title,
            coverImage: content.coverImage,
            webUrl: content.webUrl,
          }),
        ],
        isLastPage: true,
      };
    }

    const filters = request.filters ?? {};
    const sortOrder =
      request.sort?.key && request.sort.key !== "alphabetical"
        ? request.sort.key
        : undefined;

    let html: string;
    if (page === 1) {
      const fields: [string, string][] = [
        ["cmd_wpm_wgt_mng_sch_sbm", "Search"],
        ["txt_wpm_wgt_mng_sch_nme", ""],
        ["cmd_wpm_pag_mng_sch_sbm", ""],
        ["txt_wpm_pag_mng_sch_nme", query],
        [
          "cbo_wpm_pag_mng_sch_nme",
          String((filters.nameMode as string | undefined) ?? "0"),
        ],
        [
          "txt_wpm_pag_mng_sch_ats",
          String((filters.artist as string | undefined) ?? ""),
        ],
        [
          "cbo_wpm_pag_mng_sch_ats",
          String((filters.artistMode as string | undefined) ?? "0"),
        ],
        [
          "txt_wpm_pag_mng_sch_chr",
          String((filters.character as string | undefined) ?? ""),
        ],
        [
          "cbo_wpm_pag_mng_sch_chr",
          String((filters.characterMode as string | undefined) ?? "0"),
        ],
        [
          "txt_wpm_pag_mng_sch_rls_yer",
          String((filters.year as string | undefined) ?? ""),
        ],
        [
          "cbo_wpm_pag_mng_sch_rls_yer",
          String((filters.yearMode as string | undefined) ?? "0"),
        ],
        [
          "rad_wpm_pag_mng_sch_sts",
          String((filters.status as string | undefined) ?? "0"),
        ],
        [
          "rad_wpm_pag_mng_sch_tag_mde",
          (filters.tagMode as string | undefined) === "1" ? "or" : "and",
        ],
      ];

      const categories = filters.categories as
        | { include?: string[]; exclude?: string[] }
        | undefined;
      for (const id of categories?.include ?? []) {
        fields.push(["chk_wpm_pag_mng_sch_mng_tag_inc[]", id]);
      }
      for (const id of categories?.exclude ?? []) {
        fields.push(["chk_wpm_pag_mng_sch_mng_tag_exc[]", id]);
      }
      const tags = filters.tags as
        | { include?: string[]; exclude?: string[] }
        | undefined;
      for (const id of tags?.include ?? []) {
        fields.push(["chk_wpm_pag_mng_sch_mng_tag_inc[]", id]);
      }
      for (const id of tags?.exclude ?? []) {
        fields.push(["chk_wpm_pag_mng_sch_mng_tag_exc[]", id]);
      }

      html = await postForm(`${BASE}/hentai-list/advanced-search`, fields, {
        client: this.client,
        referer: `${BASE}/`,
        cloudflareResolutionURL: `${BASE}/`,
      });

      if (sortOrder) {
        const sortHref = firstMatch(
          html,
          new RegExp(
            `<li\\b[^>]*class=["'][^"']*dropdown[^"']*["'][^>]*>[\\s\\S]*?<a\\b[^>]*href=["']([^"']+)["'][^>]*>\\s*${sortOrder.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`,
            "i",
          ),
        );
        // Upstream: li.dropdown li:contains(sortOrder) a
        const loose = firstMatch(
          html,
          new RegExp(
            `<a\\b[^>]*href=["']([^"']+)["'][^>]*>\\s*[^<]*${sortOrder.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`,
            "i",
          ),
        );
        const target = sortHref ?? loose;
        if (target) {
          html = await fetchText(absoluteUrl(BASE, target), {
            client: this.client,
            referer: `${BASE}/`,
            cloudflareResolutionURL: `${BASE}/`,
          });
        }
      }
    } else {
      if (!nextSearchPage) {
        return { items: [], isLastPage: true };
      }
      html = await fetchText(nextSearchPage, { client: this.client, referer: `${BASE}/`, cloudflareResolutionURL: `${BASE}/` });
    }

    const items = parseListing(html);
    return { items, isLastPage: !hasNext(html) || items.length === 0 };
  };

  getContent = async (contentId: string): Promise<Content> => {
    const url = `${BASE}/${contentId}/`;
    const html = await fetchText(url, { client: this.client, referer: `${BASE}/`, cloudflareResolutionURL: `${BASE}/` });

    const title =
      stripTags(
        firstMatch(
          html,
          /<h3\b[^>]*class=["'][^"']*block-title[^"']*["'][^>]*>\s*<i\b[^>]*fa-book[^>]*>[\s\S]*?<a\b[^>]*>([\s\S]*?)<(?:small|\/a)/i,
        ) ??
          firstMatch(
            html,
            new RegExp(
              `<h3\\b[^>]*class=["'][^"']*block-title[^"']*["'][^>]*>[\\s\\S]*?<a\\b[^>]*href=["'][^"']*/${contentId.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}/["'][^>]*>([\\s\\S]*?)<(?:small|\\/a)`,
              "i",
            ),
          ) ??
          "",
      ) || contentId;

    const info =
      firstMatch(
        html,
        /(<ul\b[^>]*class=["'][^"']*list-simple-mini[^"']*["'][^>]*>[\s\S]*?<\/ul>)/i,
      ) ?? "";

    const author = listLiTexts(info, "Author").join(", ") || undefined;
    const artist = listLiTexts(info, "Artist").join(", ") || undefined;
    const genres = [
      ...listLiTexts(info, "Category"),
      ...listLiTexts(info, "Content"),
    ];

    const altTitles = (() => {
      const firstLi = firstMatch(info, /<li\b[^>]*>([\s\S]*?)<\/li>/i);
      if (!firstLi) return [];
      const text = stripTags(firstLi);
      if (!text || text === "-") return [];
      return text.split(",").map((t) => t.trim()).filter(Boolean);
    })();

    const storyline =
      stripTags(
        firstMatch(
          info,
          /Storyline[\s\S]*?<p\b[^>]*>([\s\S]*?)<\/p>/i,
        ) ?? "",
      ) || "";

    const descParts: string[] = [];
    if (altTitles.length) {
      descParts.push(`Alternative Title:\n${altTitles.join(", ")}`);
    }
    if (storyline) descParts.push(`Storyline:\n${storyline}`);
    for (const label of ["Parody", "Page", "Character", "Language"] as const) {
      const values = listLiTexts(info, label);
      if (values.length) descParts.push(`${label}:\n${values.join(", ")}`);
    }

    const status = parseStatus(listLiTexts(info, "Status").join(" "));

    const cover =
      firstMatch(
        html,
        /<a\b[^>]*id=["']js-linkNext["'][^>]*>[\s\S]*?<img\b[^>]*src=["']([^"']+)["']/i,
      ) ??
      firstMatch(html, /<img\b[^>]*class=["'][^"']*img-responsive[^"']*["'][^>]*src=["']([^"']+)["']/i) ??
      "";

    return {
      title,
      coverImage: absoluteUrl(BASE, cover),
      webUrl: url,
      rating: ContentRating.MATURE,
      status,
      summary: descParts.join("\n\n") || undefined,
      additionalTitles: altTitles.length ? altTitles : undefined,
      genres: genres.map((g) => ({
        id: g.toLowerCase().replace(/\s+/g, "-"),
        title: g,
      })),
      credits: [
        ...(author ? [{ name: author, role: "Author" }] : []),
        ...(artist && artist !== author
          ? [{ name: artist, role: "Artist" }]
          : []),
      ],
    };
  };

  getChapters = async (contentId: string): Promise<Chapter[]> => {
    const url = `${BASE}/${contentId}/`;
    const html = await fetchText(url, { client: this.client, referer: `${BASE}/`, cloudflareResolutionURL: `${BASE}/` });
    const list =
      firstMatch(
        html,
        /(<ul\b[^>]*class=["'][^"']*nav-chapters[^"']*["'][^>]*>[\s\S]*?<\/ul>)/i,
      ) ?? "";

    const chapters: Chapter[] = [];
    const items = collectBlocks(list, /<li\b[^>]*>/gi, "li");
    for (const [index, item] of items.entries()) {
      const media =
        firstMatch(
          item,
          /(<div\b[^>]*class=["'][^"']*\bmedia\b[^"']*["'][^>]*>[\s\S]*?<\/div>\s*<\/div>)/i,
        ) ?? item;
      // Direct child-ish: prefer pull-left chapter link over download/thumbnail.
      const link =
        /<a\b([^>]*class=["'][^"']*pull-left[^"']*["'][^>]*)>([\s\S]*?)<\/a>/i.exec(
          media,
        ) ??
        /<a\b([^>]*href=["'][^"']*\/\d+\/["'][^>]*)>([\s\S]*?)<\/a>/i.exec(
          media,
        );
      if (!link) continue;
      const href = attr(`<a ${link[1]}>`, "href") ?? "";
      if (!href || /\/download\/|\/thumbnails\//i.test(href)) continue;
      const title = stripTags(
        (link[2] ?? "").replace(/<div[\s\S]*$/i, ""),
      ).trim();
      const timeText =
        stripTags(
          firstMatch(link[2] ?? "", /<small\b[^>]*>([\s\S]*?)<\/small>/i) ??
            "",
        ) || "";
      const about = timeText.includes("about")
        ? timeText.split("about")[1]?.split("ago")[0] ?? ""
        : "";
      const id = href
        .replace(/^https?:\/\/[^/]+/i, "")
        .replace(/\/+$/, "")
        .replace(/^\//, "");
      chapters.push({
        id,
        index,
        number: parseChapterNumber(title, id),
        title: title || `Chapter ${index + 1}`,
        language: "en",
        date: about ? parseChapterDate(about) : undefined,
        webUrl: absoluteUrl(BASE, href),
      });
    }
    return chapters;
  };

  getChapterPages = async (
    _contentId: string,
    chapterId: string,
  ): Promise<ChapterPage[]> => {
    const url = chapterId.startsWith("http")
      ? chapterId
      : `${BASE}/${chapterId.replace(/^\/+/, "")}/`;
    const html = await fetchText(url, { client: this.client, referer: `${BASE}/`, cloudflareResolutionURL: `${BASE}/` });
    const pages: ChapterPage[] = [];
    const global = new RegExp(pagesUrlPattern.source, "g");
    let match: RegExpExecArray | null;
    while ((match = global.exec(html))) {
      const blob = match[1] ?? "";
      for (const part of blob.split(",")) {
        const path = part
          .trim()
          .replace(/^"|"$/g, "")
          .replace(/\\\//g, "/");
        if (!path) continue;
        pages.push({ url: IMAGE_BASE + path });
      }
    }
    if (!pages.length) throw new Error("No pages found");
    return pages;
  };
}

const parseChapterNumber = (title: string, id: string): number => {
  const fromTitle = title.match(/^(\d+(?:\.\d+)?)/);
  if (fromTitle) return Number.parseFloat(fromTitle[1]!);
  const fromId = id.match(/\/(\d+)(?:\/|$)/);
  if (fromId) return Number.parseFloat(fromId[1]!);
  return -1;
};
