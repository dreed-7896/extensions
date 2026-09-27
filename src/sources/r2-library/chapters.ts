import { base64Decode } from "../_shared/base64";
import { basename, isArchiveName, parseChapterNumber } from "./archive";
import { bytesToBase64, utf8Encode } from "./crypto";
import {
  canonicalUrl,
  extractRemoteId,
  isAbsoluteHttpUrl,
  tryIdentifySite,
} from "./sites";

export type ParsedChapter = {
  title: string;
  number: number;
  url: string;
  scanlator?: string;
};

const toUrlSafeB64 = (value: string): string =>
  bytesToBase64(utf8Encode(value)).replaceAll("+", "-").replaceAll("/", "_").replaceAll("=", "");

const fromUrlSafeB64 = (value: string): string => {
  const padded = value.replaceAll("-", "+").replaceAll("_", "/");
  const pad = padded.length % 4 === 0 ? "" : "=".repeat(4 - (padded.length % 4));
  return base64Decode(`${padded}${pad}`);
};

export const encodePagesChapter = (urls: string[]): string =>
  `pages:${toUrlSafeB64(JSON.stringify(urls))}`;

export const decodePagesChapter = (url: string): string[] | null => {
  if (!url.startsWith("pages:")) return null;
  try {
    const parsed = JSON.parse(fromUrlSafeB64(url.slice(6)));
    return Array.isArray(parsed)
      ? parsed.filter((entry): entry is string => typeof entry === "string" && !!entry)
      : null;
  } catch {
    return null;
  }
};

export const resolveChapterTarget = (raw: string, seriesPrefix: string): string => {
  const trimmed = raw.trim();
  if (trimmed.startsWith("pages:") || isAbsoluteHttpUrl(trimmed)) return trimmed;
  const prefix = seriesPrefix.replace(/\/+$/, "");
  const relative = trimmed.replace(/^\/+/, "");
  return prefix ? `${prefix}/${relative}` : relative;
};

const stringField = (obj: Record<string, unknown>, keys: string[]): string => {
  for (const key of keys) {
    const value = obj[key];
    if (typeof value === "string" && value.trim()) return value.trim();
    if (typeof value === "number") return String(value);
  }
  return "";
};

const asObject = (entry: unknown): Record<string, unknown> | null =>
  entry && typeof entry === "object" && !Array.isArray(entry)
    ? (entry as Record<string, unknown>)
    : null;

const chapterList = (parsed: unknown): unknown[] => {
  if (Array.isArray(parsed)) return parsed;
  if (parsed && typeof parsed === "object") {
    const chapters = (parsed as { chapters?: unknown }).chapters;
    if (Array.isArray(chapters)) return chapters;
  }
  return [];
};

export const parseChaptersJson = (
  body: string,
  seriesPrefix = "",
): ParsedChapter[] => {
  let parsed: unknown;
  try {
    parsed = JSON.parse(body.trim().replace(/^\uFEFF/, ""));
  } catch {
    return [];
  }

  return chapterList(parsed).flatMap((entry, index) => {
    const obj = asObject(entry);
    const rawUrl =
      typeof entry === "string"
        ? entry.trim()
        : obj
          ? stringField(obj, ["url", "href", "link", "archive", "file", "key"])
          : "";
    const pages = Array.isArray(obj?.pages)
      ? obj.pages.filter((item): item is string => typeof item === "string" && !!item.trim())
      : [];
    const source = obj ? stringField(obj, ["source", "site", "host"]) : "";
    const rawId = obj ? stringField(obj, ["id"]) : "";
    const title = obj ? stringField(obj, ["title", "name"]) : "";

    let chapterUrl = "";
    let displayFallback = `Chapter ${index + 1}`;
    let siteLabel: string | undefined;

    if (pages.length) {
      chapterUrl = encodePagesChapter(pages);
      siteLabel = source || undefined;
    } else {
      const site = tryIdentifySite(rawUrl, source || undefined);
      if (site) {
        const remoteId = rawId || extractRemoteId(site, rawUrl);
        chapterUrl = rawUrl || canonicalUrl(site, remoteId);
        displayFallback = `${site} ${remoteId}`.trim();
        siteLabel = source || site;
      } else if (rawUrl) {
        chapterUrl = resolveChapterTarget(rawUrl, seriesPrefix);
        displayFallback = basename(chapterUrl).replace(/\.(cbz|zip)$/i, "") || `Chapter ${index + 1}`;
        siteLabel = source || undefined;
      } else if (rawId && tryIdentifySite("", source)) {
        const resolved = tryIdentifySite("", source)!;
        chapterUrl = canonicalUrl(resolved, rawId);
        displayFallback = `${resolved} ${rawId}`;
        siteLabel = source || resolved;
      } else if (rawId) {
        chapterUrl = resolveChapterTarget(rawId, seriesPrefix);
        displayFallback = basename(chapterUrl).replace(/\.(cbz|zip)$/i, "") || `Chapter ${index + 1}`;
        siteLabel = source || undefined;
      } else {
        return [];
      }
    }

    const display = title || displayFallback;
    const explicit = obj ? Number(stringField(obj, ["number"])) : Number.NaN;
    const number = Number.isFinite(explicit) && explicit > 0
      ? explicit
      : parseChapterNumber(display, index + 1);
    const scanlator = obj
      ? stringField(obj, ["scanlator", "group"]) || siteLabel
      : siteLabel;

    return [
      {
        title: display,
        number,
        url: chapterUrl,
        scanlator: scanlator || undefined,
      },
    ];
  });
};

export const isChaptersJsonName = (name: string): boolean =>
  /^(chapters|chapter-list|chapter_list)\.json$/i.test(name);

export const isFolderChapter = (url: string): boolean =>
  !isAbsoluteHttpUrl(url) && !url.startsWith("pages:") && url.endsWith("/");

export const isBucketArchive = (url: string): boolean =>
  !isAbsoluteHttpUrl(url) && !url.startsWith("pages:") && isArchiveName(url);
