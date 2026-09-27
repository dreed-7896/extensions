/** In-memory chapter page sessions so getChapterPages stays bridge-light. */

import { unzipSync } from "fflate";
import { bytesToBase64 } from "./crypto";
import {
  basename,
  isArchiveName,
  isImageName,
  naturalCompare,
} from "./archive";

const MAX_ARCHIVE_BYTES = 80 * 1024 * 1024; // 80 MiB compressed
const MAX_PAGES = 400;
const SESSION_TTL_MS = 30 * 60 * 1000;

export const PAGE_HOST = "https://r2-library.pages";

type PageBlob = {
  mime: string;
  bytes: Uint8Array;
};

type Session = {
  createdAt: number;
  pages: PageBlob[];
};

const sessions = new Map<string, Session>();

const mimeForName = (name: string): string => {
  const lower = name.toLowerCase();
  if (lower.endsWith(".png")) return "image/png";
  if (lower.endsWith(".webp")) return "image/webp";
  if (lower.endsWith(".gif")) return "image/gif";
  if (lower.endsWith(".avif")) return "image/avif";
  if (lower.endsWith(".bmp")) return "image/bmp";
  return "image/jpeg";
};

const isImagePath = (name: string): boolean => {
  if (!name || name.endsWith("/")) return false;
  if (name.startsWith("__MACOSX/")) return false;
  if (name.split("/").some((part) => part.startsWith("."))) return false;
  return isImageName(basename(name));
};

const pruneSessions = (): void => {
  const cutoff = Date.now() - SESSION_TTL_MS;
  for (const [id, session] of sessions) {
    if (session.createdAt < cutoff) sessions.delete(id);
  }
};

const newSessionId = (): string =>
  `s${Date.now().toString(36)}${Math.floor(Math.random() * 1e9).toString(36)}`;

export const pageUrl = (sessionId: string, index: number): string =>
  `${PAGE_HOST}/${sessionId}/${index}`;

export const parsePageUrl = (
  url: string,
): { sessionId: string; index: number } | null => {
  const match = /^https:\/\/r2-library\.pages\/([^/]+)\/(\d+)(?:\?.*)?$/i.exec(
    url,
  );
  if (!match) return null;
  return { sessionId: match[1]!, index: Number(match[2]) };
};

export const dataUrlForPage = (url: string): string | null => {
  const parsed = parsePageUrl(url);
  if (!parsed) return null;
  const session = sessions.get(parsed.sessionId);
  const page = session?.pages[parsed.index];
  if (!page) return null;
  return `data:${page.mime};base64,${bytesToBase64(page.bytes)}`;
};

export const b64ForPage = (url: string): string | null => {
  const parsed = parsePageUrl(url);
  if (!parsed) return null;
  const session = sessions.get(parsed.sessionId);
  const page = session?.pages[parsed.index];
  return page ? bytesToBase64(page.bytes) : null;
};

/** Unzip a CBZ/ZIP into a session; returns lightweight page URLs. */
export const openArchiveSession = (
  archiveBytes: Uint8Array,
): { sessionId: string; urls: string[] } => {
  pruneSessions();

  if (archiveBytes.byteLength > MAX_ARCHIVE_BYTES) {
    throw new Error(
      `Chapter archive is too large (${Math.round(archiveBytes.byteLength / (1024 * 1024))} MiB). Keep CBZs under ${MAX_ARCHIVE_BYTES / (1024 * 1024)} MiB, or use a chapter folder of images instead (no size limit).`,
    );
  }

  let files: Record<string, Uint8Array>;
  try {
    files = unzipSync(archiveBytes, {
      filter: (file) => isImagePath(file.name),
    });
  } catch (error) {
    throw new Error(
      `Failed to unzip chapter archive: ${error instanceof Error ? error.message : String(error)}`,
    );
  }

  const pages = Object.entries(files)
    .filter(([, data]) => !!data?.length)
    .map(([name, data]) => ({
      name: basename(name),
      full: name,
      data: data as Uint8Array,
    }))
    .sort((left, right) => naturalCompare(left.full, right.full))
    .map(({ name, data }) => ({
      mime: mimeForName(name),
      bytes: data,
    }));

  if (!pages.length) {
    throw new Error("Archive contained no readable image pages");
  }
  if (pages.length > MAX_PAGES) {
    throw new Error(
      `Archive has ${pages.length} pages (max ${MAX_PAGES}). Split the CBZ or store images as a chapter folder.`,
    );
  }

  const sessionId = newSessionId();
  sessions.set(sessionId, { createdAt: Date.now(), pages });
  return {
    sessionId,
    urls: pages.map((_, index) => pageUrl(sessionId, index)),
  };
};

/** Image object keys under a prefix (natural order by filename). */
export const listImageKeys = (objectKeys: string[]): string[] =>
  objectKeys
    .filter((key) => {
      const name = basename(key);
      return isImageName(name) && !isArchiveName(name) && !name.startsWith(".");
    })
    .sort((left, right) => naturalCompare(basename(left), basename(right)));
