import { unzipSync } from "fflate";
import { bytesToBase64 } from "./crypto";

const IMAGE_EXTENSIONS = new Set([
  ".jpg",
  ".jpeg",
  ".png",
  ".webp",
  ".gif",
  ".avif",
  ".bmp",
]);

export const isArchiveName = (name: string): boolean => {
  const lower = name.toLowerCase();
  return lower.endsWith(".cbz") || lower.endsWith(".zip");
};

export const isImageName = (name: string): boolean => {
  if (!name || name.endsWith("/")) return false;
  if (name.startsWith(".")) return false;
  const lower = name.toLowerCase();
  const dot = lower.lastIndexOf(".");
  if (dot < 0) return false;
  return IMAGE_EXTENSIONS.has(lower.slice(dot));
};

export const isCoverName = (name: string): boolean =>
  /^cover\.(png|jpe?g|webp|gif|avif)$/i.test(name);

export const isDetailsName = (name: string): boolean =>
  /^(details|info|metadata|series)\.json$/i.test(name);

export const isChaptersFileName = (name: string): boolean =>
  /^(chapters|chapter-list|chapter_list)\.json$/i.test(name);

export type ChapterPageRef = {
  chapter: string;
  page: string;
};

/**
 * Parse cover refs:
 * - `Chapter 1_1` → chapter name + 1-based page index
 * - `chapter 4_24.png` → chapter name + page filename
 */
export const parseChapterPageRef = (
  value: string,
): ChapterPageRef | null => {
  const trimmed = value.trim();
  if (
    !trimmed ||
    /^https?:\/\//i.test(trimmed) ||
    trimmed.startsWith("data:") ||
    isCoverName(trimmed) ||
    trimmed.includes("/") ||
    trimmed.includes("\\")
  ) {
    return null;
  }

  const separator = trimmed.lastIndexOf("_");
  if (separator <= 0 || separator === trimmed.length - 1) return null;

  const chapter = trimmed.slice(0, separator).trim();
  const page = trimmed.slice(separator + 1).trim();
  if (!chapter || !page) return null;

  const lower = page.toLowerCase();
  const dot = lower.lastIndexOf(".");
  if (dot < 0) {
    return /^\d+$/.test(page) ? { chapter, page } : null;
  }
  if (!IMAGE_EXTENSIONS.has(lower.slice(dot))) return null;

  return { chapter, page };
};

export const chapterNameMatches = (name: string, needle: string): boolean => {
  const a = name.replace(/\.(cbz|zip)$/i, "").trim().toLowerCase();
  const b = needle.replace(/\.(cbz|zip)$/i, "").trim().toLowerCase();
  if (!a || !b) return false;
  if (a === b) return true;
  const strip = (value: string) =>
    value.replace(/^\d+(?:\.\d+)?\s*[-._:)\]\s]+/, "").trim();
  if (strip(a) === b || a.endsWith(b) || b.endsWith(a)) return true;
  const left = parseChapterNumber(name, -1);
  const right = parseChapterNumber(needle, -1);
  return left >= 0 && right >= 0 && left === right;
};

export const archiveStem = (name: string): string =>
  name.replace(/\.(cbz|zip)$/i, "").trim();

export const findArchiveByChapterName = <T extends { name: string }>(
  archives: T[],
  chapterName: string,
): T | undefined => {
  const needle = chapterName.trim().toLowerCase();
  if (!needle) return undefined;

  const exact = archives.find(
    (archive) => archiveStem(archive.name).toLowerCase() === needle,
  );
  if (exact) return exact;

  return archives.find((archive) => chapterNameMatches(archive.name, chapterName));
};

const mimeForPage = (pageName: string): string => {
  const lower = pageName.toLowerCase();
  if (lower.endsWith(".png")) return "image/png";
  if (lower.endsWith(".webp")) return "image/webp";
  if (lower.endsWith(".gif")) return "image/gif";
  if (lower.endsWith(".avif")) return "image/avif";
  if (lower.endsWith(".bmp")) return "image/bmp";
  return "image/jpeg";
};

/** Pull one page out of a cbz/zip and return a data URL usable as coverImage. */
export const extractArchivePageDataUrl = (
  archiveBytes: Uint8Array,
  pageName: string,
): string => {
  const files = unzipSync(archiveBytes);
  const images = Object.entries(files)
    .filter(([name, data]) => {
      if (!data?.length || name.endsWith("/")) return false;
      if (name.startsWith("__MACOSX/")) return false;
      return isImageName(basename(name));
    })
    .sort(([left], [right]) => naturalCompare(left, right));

  const target = pageName.toLowerCase();
  const indexed = /^\d+$/.test(pageName)
    ? images[Number(pageName) - 1]
    : images.find(([name]) => basename(name).toLowerCase() === target);

  if (!indexed) {
    throw new Error(`Page "${pageName}" not found inside archive`);
  }

  const [name, data] = indexed;
  return `data:${mimeForPage(name)};base64,${bytesToBase64(data as Uint8Array)}`;
};

export const basename = (key: string): string => {
  const parts = key.replace(/\/+$/, "").split("/");
  return parts[parts.length - 1] ?? key;
};

export const folderIdFromPrefix = (
  prefix: string,
  rootPrefix: string,
): string => {
  const cleaned = rootPrefix.replace(/^\/+|\/+$/g, "");
  const normalized = prefix.replace(/^\/+/, "").replace(/\/+$/, "");
  if (!cleaned) {
    return normalized.split("/")[0] ?? "";
  }

  const root = `${cleaned}/`;
  const full = prefix.replace(/^\/+/, "");
  const relative = full.startsWith(root) ? full.slice(root.length) : full;
  return relative.replace(/\/+$/, "").split("/")[0] ?? "";
};

export const naturalCompare = (left: string, right: string): number =>
  left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" });

export const parseChapterNumber = (name: string, fallback: number): number => {
  const stem = name.replace(/\.(cbz|zip)$/i, "");
  const match = stem.match(/(\d+(?:\.\d+)?)/);
  return match ? Number(match[1]) : fallback;
};

export const extractArchivePages = (
  archiveBytes: Uint8Array,
): { name: string; b64: string }[] => {
  const files = unzipSync(archiveBytes);
  const pages = Object.entries(files)
    .filter(([name, data]) => {
      if (!data?.length) return false;
      if (name.endsWith("/")) return false;
      if (name.startsWith("__MACOSX/")) return false;
      if (name.split("/").some((part) => part.startsWith("."))) return false;
      const lower = name.toLowerCase();
      const dot = lower.lastIndexOf(".");
      if (dot < 0) return false;
      return IMAGE_EXTENSIONS.has(lower.slice(dot));
    })
    .map(([name, data]) => ({ name, data: data as Uint8Array }))
    .sort((left, right) => naturalCompare(left.name, right.name))
    .map(({ name, data }) => ({
      name,
      b64: bytesToBase64(data),
    }));

  if (!pages.length) {
    throw new Error("Archive contained no readable image pages");
  }

  return pages;
};
