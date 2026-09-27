import { unzipSync } from "fflate";
import { bytesToBase64 } from "./base64";

const IMAGE_EXTENSIONS = new Set([
  ".jpg",
  ".jpeg",
  ".png",
  ".webp",
  ".gif",
  ".avif",
  ".bmp",
]);

const basename = (path: string): string => {
  const parts = path.replace(/\/+$/, "").split("/");
  return parts[parts.length - 1] ?? path;
};

const naturalCompare = (left: string, right: string): number =>
  left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" });

/** Unzip an archive and return image pages as base64 (natural name order). */
export const extractZipImagePages = (
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
    .map(([name, data]) => ({ name: basename(name), data: data as Uint8Array }))
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
