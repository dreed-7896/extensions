import type { Chapter, ChapterPage, Content } from "@suwatte/toolchain/types";
import {
  ContentRating,
  ContentStatus,
  ReadingMode,
} from "@suwatte/toolchain/types";
import { base64Decode } from "../_shared/html";
import { matureItem } from "../_shared/item";
import { BASE, MANGA } from "./constants";

export type MangaDto = {
  id?: number | string;
  slug?: string;
  title?: string;
  synopsis?: string | null;
  coverUrl?: string | null;
  status?: string | null;
  type?: string | null;
  contentRating?: string | null;
  genres?: string[] | null;
  authors?: string[] | null;
  artists?: string[] | null;
  alternativeTitles?: string[] | string | null;
  year?: number | null;
  score?: number | null;
  views?: number | null;
};

export type ChapterDto = {
  id?: number | string;
  seriesId?: number;
  number?: number;
  title?: string | null;
  createdAt?: string | null;
  language?: string | null;
};

export type PageDto = {
  pageOrder?: number;
  webpUrl?: string | null;
  avifUrl?: string | null;
};

export type SearchPayload = {
  query: string;
  sort: string;
  genres: string[];
  type?: string;
  status?: string;
  contentRating?: string;
  year?: string;
  maxRating: string;
  page: number;
  limit: number;
};

const TITLE_VERSION_RE =
  /^(?:\s*(?:\([^()]*\)|\{[^{}]*\}|\[(?:(?!]).)*]|«[^»]*»|〘[^〙]*〙|「[^」]*」|『[^』]*』|≪[^≫]*≫|﹛[^﹜]*﹜|〖[^〖〗]*〗|𖤍.+?𖤍|《[^》]*》|⌜.+?⌝|⟨[^⟩]*⟩)\s*)+|(?:\s*(?:\([^()]*\)|\{[^{}]*\}|\[(?:(?!]).)*]|«[^»]*»|〘[^〙]*〙|「[^」]*」|『[^』]*』|≪[^≫]*≫|﹛[^﹜]*﹜|〖[^〖〗]*〗|𖤍.+?𖤍|《[^》]*》|⌜.+?⌝|⟨[^⟩]*⟩|\/\s*Official)\s*)+$/i;

export const parseSlug = (value: string): string => {
  const cleaned = value.replace(/[?#].*$/, "").replace(/\/+$/, "");
  const marker = `/${MANGA}/`;
  const idx = cleaned.indexOf(marker);
  if (idx !== -1) {
    return decodeURIComponent(
      cleaned.slice(idx + marker.length).split("/")[0] ?? cleaned,
    );
  }
  return decodeURIComponent(cleaned.split("/").filter(Boolean).pop() ?? cleaned);
};

export const formatChapterNumber = (n: number): string =>
  String(n).replace(/\.0$/, "");

export const chapterIdOf = (number: number, id: number | string): string =>
  `${formatChapterNumber(number)}:${id}`;

export const parseChapterId = (
  chapterId: string,
): { number: number; id?: string } => {
  const cleaned = chapterId.replace(/\/+$/, "");
  const colon = cleaned.lastIndexOf(":");
  if (colon > 0) {
    const number = Number.parseFloat(cleaned.slice(0, colon));
    const id = cleaned.slice(colon + 1);
    return {
      number: Number.isFinite(number) ? number : 1,
      id: id || undefined,
    };
  }
  const number = Number.parseFloat(cleaned);
  return { number: Number.isFinite(number) ? number : 1 };
};

export const contentUrl = (slug: string): string =>
  `${BASE}/${MANGA}/${encodeURI(parseSlug(slug))}/`;

export const chapterUrl = (slug: string, number: number): string =>
  `${BASE}/${MANGA}/${encodeURI(parseSlug(slug))}/${formatChapterNumber(number)}`;

export const trpcUrl = (procedures: string, input: unknown): string => {
  const encoded = encodeURIComponent(JSON.stringify(input));
  return `${BASE}/api/trpc/${procedures}?batch=1&input=${encoded}`;
};

export const searchInput = (payload: SearchPayload) => {
  const genres = payload.genres;
  const type = payload.type || null;
  const status = payload.status || null;
  const rating = payload.contentRating || null;
  const year = payload.year?.trim() || null;
  return {
    "0": {
      json: {
        q: payload.query,
        sort: payload.sort,
        filters: {
          genres: genres.length ? genres : null,
          type,
          status,
          contentRating: rating,
          author: null,
          artist: null,
          year: year && /^\d{4}$/.test(year) ? Number(year) : null,
        },
        limit: payload.limit,
        offset: (Math.max(1, payload.page) - 1) * payload.limit,
        maxRating: payload.maxRating,
      },
      meta: {
        values: {
          ...(genres.length ? {} : { "filters.genres": ["undefined"] }),
          ...(type ? {} : { "filters.type": ["undefined"] }),
          ...(status ? {} : { "filters.status": ["undefined"] }),
          ...(rating ? {} : { "filters.contentRating": ["undefined"] }),
          "filters.author": ["undefined"],
          "filters.artist": ["undefined"],
          ...(year ? {} : { "filters.year": ["undefined"] }),
        },
      },
    },
  };
};

export const detailsInput = (slug: string) => ({
  "0": { json: null, meta: { values: ["undefined"] } },
  "1": { json: { slug } },
});

export const chaptersInput = (seriesId: number | string) => ({
  "0": { json: { values: ["undefined"] } },
  "1": {
    json: {
      seriesId,
      chapterId: null,
      sort: "best",
      page: 1,
      limit: 1000,
    },
    meta: { values: { chapterId: ["undefined"] } },
  },
  "2": { json: { seriesId } },
});

export const pagesInput = (
  slug: string,
  chapterNumber: number,
  chapterId?: string,
) => ({
  "0": { json: null, meta: { values: ["undefined"] } },
  "1": { json: { slug } },
  "2": {
    json: {
      seriesSlug: slug,
      chapterNumber,
      ...(chapterId ? { chapterId: Number(chapterId) || chapterId } : {}),
    },
  },
  "3": { json: { position: "footer_bottom" } },
});

export const unwrapBatch = <T>(data: unknown, index = -1): T | undefined => {
  if (!Array.isArray(data) || !data.length) return undefined;
  const element = index < 0 ? data[data.length + index] : data[index];
  if (!element || typeof element !== "object") return undefined;
  const record = element as Record<string, unknown>;
  if ("error" in record) return undefined;
  const result = record.result as Record<string, unknown> | undefined;
  const wrapped = result?.data as Record<string, unknown> | undefined;
  return (wrapped?.json as T) ?? undefined;
};

export const parseAltTitles = (raw: MangaDto["alternativeTitles"]): string[] => {
  if (!raw) return [];
  if (Array.isArray(raw)) return raw.map(String).filter(Boolean);
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (Array.isArray(parsed)) return parsed.map(String).filter(Boolean);
  } catch {
    // plain string
  }
  return raw.trim() ? [raw.trim()] : [];
};

export const cleanTitle = (
  title: string,
  options: { stripVersion: boolean; customRegex: string },
): string => {
  let next = title;
  if (options.customRegex) {
    try {
      next = next.replace(new RegExp(options.customRegex, "g"), "");
    } catch {
      // invalid user regex — ignore
    }
  }
  if (options.stripVersion) next = next.replace(TITLE_VERSION_RE, "");
  return next.trim() || title.trim();
};

export const parseStatus = (raw?: string | null): ContentStatus => {
  switch ((raw ?? "").toLowerCase()) {
    case "ongoing":
      return ContentStatus.ONGOING;
    case "completed":
      return ContentStatus.COMPLETED;
    case "hiatus":
      return ContentStatus.HIATUS;
    case "cancelled":
    case "canceled":
      return ContentStatus.CANCELLED;
    default:
      return ContentStatus.UNKNOWN;
  }
};

const readingModeOf = (type?: string | null): ReadingMode | undefined => {
  const value = (type ?? "").toLowerCase();
  if (value === "manhwa" || value === "webtoon" || value === "manhua") {
    return ReadingMode.VERTICAL;
  }
  if (value === "manga") return ReadingMode.PAGED_MANGA;
  return undefined;
};

export const itemFromManga = (
  dto: MangaDto,
  title: string,
) => {
  const slug = dto.slug ?? "";
  return matureItem({
    id: slug,
    title,
    coverImage: dto.coverUrl ?? undefined,
    webUrl: slug ? contentUrl(slug) : undefined,
    subtitle: dto.type ?? undefined,
  });
};

export const contentFromManga = (
  dto: MangaDto,
  title: string,
): Content => {
  const slug = dto.slug ?? "";
  const alts = parseAltTitles(dto.alternativeTitles);
  const authors = dto.authors?.filter(Boolean) ?? [];
  const artists = dto.artists?.filter(Boolean) ?? [];
  const genres = [
    ...(dto.genres ?? []),
    dto.type,
    dto.contentRating,
  ].filter((g): g is string => !!g);
  const credits = [
    ...authors.map((name) => ({ name, role: "Author" })),
    ...artists
      .filter((name) => !authors.includes(name))
      .map((name) => ({ name, role: "Artist" })),
  ];
  const additionalDetails: Record<string, string> = {
    ...(dto.year ? { Year: String(dto.year) } : {}),
    ...(dto.type ? { Type: dto.type } : {}),
    ...(dto.contentRating ? { Rating: dto.contentRating } : {}),
  };
  return {
    title,
    coverImage: dto.coverUrl ?? "",
    webUrl: slug ? contentUrl(slug) : undefined,
    rating: ContentRating.MATURE,
    status: parseStatus(dto.status),
    readingMode: readingModeOf(dto.type),
    summary: dto.synopsis || undefined,
    additionalTitles: alts.length ? alts : undefined,
    additionalDetails: Object.keys(additionalDetails).length
      ? additionalDetails
      : undefined,
    statistics:
      dto.score != null || dto.views != null
        ? {
            ...(dto.score != null ? { rating: dto.score } : {}),
            ...(dto.views != null ? { views: dto.views } : {}),
          }
        : undefined,
    genres: genres.map((g) => ({
      id: g.toLowerCase().replace(/\s+/g, "-"),
      title: g,
    })),
    ...(credits.length ? { credits } : {}),
    context: {
      ...(dto.id != null ? { mangaId: dto.id } : {}),
      slug,
    },
  };
};

export const chapterFromDto = (dto: ChapterDto, slug: string): Chapter | undefined => {
  if (dto.id == null || dto.number == null) return undefined;
  const number = Number(dto.number);
  const label = `Chapter ${formatChapterNumber(number)}`;
  const rawTitle = (dto.title ?? "").trim();
  const title =
    rawTitle && /\d/.test(rawTitle) ? rawTitle : rawTitle ? `${label} ${rawTitle}` : label;
  const date = dto.createdAt ? new Date(dto.createdAt) : undefined;
  return {
    id: chapterIdOf(number, dto.id),
    index: 0,
    number,
    title,
    language: dto.language || "en",
    date: date && !Number.isNaN(date.getTime()) ? date : undefined,
    webUrl: chapterUrl(slug, number),
    data: { chapterId: String(dto.id), number, slug },
  };
};

export const pagesFromDto = (pages: PageDto[]): ChapterPage[] =>
  [...pages]
    .sort((a, b) => (a.pageOrder ?? 0) - (b.pageOrder ?? 0))
    .map((page) => page.avifUrl || page.webpUrl || "")
    .filter(Boolean)
    .map((url) => ({ url }));

export const sessionCookieFromHeaders = (
  headers: { get(name: string): string | null } | Record<string, unknown> | null | undefined,
): string | undefined => {
  if (!headers) return undefined;
  const raw =
    typeof (headers as { get?: unknown }).get === "function"
      ? String(
          (headers as { get(name: string): string | null }).get("set-cookie") ??
            (headers as { get(name: string): string | null }).get("Set-Cookie") ??
            "",
        )
      : Object.entries(headers as Record<string, unknown>)
          .filter(([key]) => key.toLowerCase() === "set-cookie")
          .map(([, value]) => String(value ?? ""))
          .join(", ");
  return raw.match(/__st=[^;,\s]+/)?.[0];
};

/** Live bundle: `atob("eWNlcXQ3cWd1MA==")+"04"` next to x-cfg-auth char codes. */
export const parseAuthFromBundle = (js: string): string | undefined => {
  const tagged = js.match(
    /\[120,45,99,102,103,45,97,117,116,104\][\s\S]{0,120}atob\("([A-Za-z0-9+/=]+)"\)\s*\+\s*"([^"]*)"/,
  );
  const plain = tagged ?? js.match(/atob\("([A-Za-z0-9+/=]+)"\)\s*\+\s*"(\d+)"/);
  if (!plain?.[1]) return undefined;
  try {
    return `${base64Decode(plain[1])}${plain[2] ?? ""}`;
  } catch {
    return undefined;
  }
};

export const scriptSrcFromHome = (html: string): string | undefined => {
  const match = html.match(
    /<script[^>]+src=["']([^"']*\/assets\/index-[^"']+\.js)["']/i,
  );
  return match?.[1];
};

export const isApiKeyError = (status: number, body: string): boolean =>
  status === 401 || /invalid or missing api key/i.test(body);
