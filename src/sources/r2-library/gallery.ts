import type { ChapterPage } from "@suwatte/toolchain/types";
import { isCloudflareError, throwCloudflare } from "../_shared/cloudflare";
import { allMatches, attr, decodeEntities, firstMatch, stripTags } from "../_shared/html";
import { absoluteUrl, fetchBytes, fetchJson, fetchText } from "../_shared/http";
import { extractZipImagePages } from "../_shared/zip";
import { galleryById, imageServer } from "../nhentai/api";
import {
  contentUrl,
  parseChapterPages,
  parseMangaId,
  readerLanguageOrder,
  readerUrl,
} from "../hentairead/parse";
import { CF_RESOLVE } from "../hentairead/constants";
import { pagesFromReaderHtml } from "../hentainexus/parse";
import { hitomiHeaders, LTN_URL, resolveImageUrl } from "../hitomi/gg";
import { mapPool } from "../_shared/pool";
import { decodePagesChapter } from "./chapters";
import {
  cloudflareResolveUrl,
  extractRemoteId,
  isRemoteArchiveUrl,
  tryIdentifySite,
  type SiteId,
} from "./sites";

const HR_BASE = "https://hentairead.com";
const HN_BASE = "https://hentainexus.com";
const H2R_BASE = "https://hentai2read.com";
const H2R_IMAGE = "https://static.hentaicdn.com/hentai";
const CHAIKA_BASE = "https://panda.chaika.moe";
const EH_BASE = "https://e-hentai.org";
const EH_HEADERS = {
  Cookie: "nw=1; uconfig=prn_n",
  "User-Agent":
    "Mozilla/5.0 (compatible; Suwatte/1.0; +https://suwatte.mantton.com)",
};

const pagesUrlPattern = /'images'\s*:\s*\["(.*?),?"]/;

type HitomiFile = { hash: string; name?: string };
type HitomiGallery = { files?: HitomiFile[] };
type ChaikaDetail = { download?: string };

const fetchHitomiPages = async (id: string): Promise<ChapterPage[]> => {
  const body = await fetchText(`${LTN_URL}/galleries/${id}.js`, {
    headers: hitomiHeaders,
  });
  const json = body.replace(/^[\s\S]*?var galleryinfo\s*=\s*/, "").trim();
  const gallery = JSON.parse(json) as HitomiGallery;
  const files = gallery.files ?? [];
  if (!files.length) return [];
  await resolveImageUrl(files[0]!.hash);
  return mapPool(files, 12, async (file) => ({
    url: await resolveImageUrl(file.hash),
  }));
};

const collectEhPageLinks = async (galleryPath: string): Promise<string[]> => {
  const urls: string[] = [];
  let next: string | null = absoluteUrl(EH_BASE, galleryPath);
  let hops = 0;
  while (next && hops < 200) {
    hops += 1;
    const html = await fetchText(next, { headers: EH_HEADERS, referer: EH_BASE });
    const gdt =
      firstMatch(html, /id="gdt"[^>]*>([\s\S]*?)(?:<table|<div id="c)/i) ?? html;
    const pageHrefs = allMatches(gdt, /<a[^>]+href="([^"]+\/s\/[^"]+)"/gi).map(
      (href) => absoluteUrl(EH_BASE, decodeEntities(href)),
    );
    for (const href of pageHrefs) {
      if (!urls.includes(href)) urls.push(href);
    }
    const navLinks =
      html.match(
        /<a[^>]*onclick="return false"[^>]*href="([^"]*)"[^>]*>([^<]*)<\/a>/gi,
      ) ?? [];
    let following: string | null = null;
    for (const tag of navLinks) {
      const href = attr(tag, "href");
      const label = stripTags(tag);
      if (label === ">" && href) {
        following = absoluteUrl(EH_BASE, href);
        break;
      }
    }
    next = following;
  }
  return urls;
};

const resolveEhImage = async (pageUrl: string): Promise<string> => {
  const html = await fetchText(pageUrl, { headers: EH_HEADERS, referer: EH_BASE });
  const img =
    firstMatch(html, /id="img"[^>]*src="([^"]+)"/i) ??
    firstMatch(html, /id="img"[^>]*data-src="([^"]+)"/i);
  if (!img) throw new Error(`No image on page ${pageUrl}`);
  return decodeEntities(img);
};

const pagesForSite = async (site: SiteId, url: string): Promise<ChapterPage[]> => {
  const id = extractRemoteId(site, url);
  switch (site) {
    case "nhentai": {
      const data = await galleryById(id);
      const server = await imageServer();
      return data.pages.map((page) => ({ url: `${server}/${page.path}` }));
    }
    case "hentairead": {
      const slug = id || parseMangaId(url);
      const detailsHtml = await fetchText(contentUrl(slug), {
        referer: HR_BASE,
        cloudflareResolutionURL: CF_RESOLVE,
      });
      const order = readerLanguageOrder(detailsHtml);
      let lastError: unknown;
      for (const language of order) {
        try {
          const html = await fetchText(readerUrl(slug, language), {
            referer: contentUrl(slug),
            cloudflareResolutionURL: CF_RESOLVE,
          });
          const pages = parseChapterPages(html);
          if (pages.length) return pages;
        } catch (error) {
          if (isCloudflareError(error)) throw error;
          lastError = error;
        }
      }
      throw lastError ?? new Error("HentaiRead reader scripts were missing.");
    }
    case "hentainexus": {
      const html = await fetchText(`${HN_BASE}/read/${id}`, {
        referer: HN_BASE,
        cloudflareResolutionURL: HN_BASE,
      });
      return pagesFromReaderHtml(html).map((image) => ({ url: image }));
    }
    case "hentai2read": {
      const chapterUrl = url.startsWith("http")
        ? url
        : `${H2R_BASE}/${id.replace(/^\/+/, "")}/`;
      const html = await fetchText(chapterUrl, {
        referer: `${H2R_BASE}/`,
        cloudflareResolutionURL: `${H2R_BASE}/`,
      });
      const pages: ChapterPage[] = [];
      const global = new RegExp(pagesUrlPattern.source, "g");
      let match: RegExpExecArray | null;
      while ((match = global.exec(html))) {
        const blob = match[1] ?? "";
        for (const part of blob.split(",")) {
          const path = part.trim().replace(/^"|"$/g, "").replace(/\\\//g, "/");
          if (path) pages.push({ url: H2R_IMAGE + path });
        }
      }
      if (!pages.length) throw new Error("No hentai2read pages found");
      return pages;
    }
    case "pandachaika": {
      const detail = await fetchJson<ChaikaDetail>(`${CHAIKA_BASE}/api?archive=${id}`, {
        referer: CHAIKA_BASE,
      });
      let download = detail.download ?? `${CHAIKA_BASE}/archive/${id}/download/`;
      if (!/^https?:\/\//i.test(download)) {
        download = `${CHAIKA_BASE}${download.startsWith("/") ? "" : "/"}${download}`;
      }
      if (!/\/download\/?$/i.test(download)) {
        download = `${download.replace(/\/+$/, "")}/download/`;
      }
      const bytes = await fetchBytes(download, {
        referer: CHAIKA_BASE,
        timeout: 180_000,
      });
      return extractZipImagePages(bytes).map((page) => ({ b64: page.b64 }));
    }
    case "ehentai": {
      const [gid, token] = id.split("/");
      const origin = /exhentai\.org/i.test(url) ? "https://exhentai.org" : EH_BASE;
      const path = token
        ? `/g/${gid}/${token}/?nw=always`
        : `/g/${gid}/?nw=always`;
      const links = await collectEhPageLinks(
        origin === EH_BASE ? path : `${origin}${path}`,
      );
      const pages = (
        await mapPool(links, 6, async (pageUrl) => {
          try {
            return { url: await resolveEhImage(pageUrl) };
          } catch {
            return null;
          }
        })
      ).filter((page): page is { url: string } => !!page);
      if (!pages.length) throw new Error(`No readable pages for ${url}`);
      return pages;
    }
    case "hitomi":
      return fetchHitomiPages(id);
    default:
      throw new Error(`Unsupported gallery host for ${url}`);
  }
};

export const pagesForChapterUrl = async (url: string): Promise<ChapterPage[]> => {
  try {
    const listed = decodePagesChapter(url);
    if (listed) return listed.map((image) => ({ url: image }));

    if (isRemoteArchiveUrl(url)) {
      const bytes = await fetchBytes(url, { timeout: 180_000 });
      return extractZipImagePages(bytes).map((page) => ({ b64: page.b64 }));
    }

    const site = tryIdentifySite(url);
    if (!site) {
      throw new Error(
        `Unknown chapter host. Use nhentai, hentairead, hentainexus, hentai2read, pandachaika, ehentai, hitomi, a .cbz/.zip, or a pages list.`,
      );
    }
    return await pagesForSite(site, url);
  } catch (error) {
    if (isCloudflareError(error)) throw error;
    const resolve = cloudflareResolveUrl(url);
    const message = String((error as { message?: string })?.message ?? error);
    if (
      resolve &&
      /cloudflare|cf-mitigated|just a moment|failed \(403\)|failed \(503\)|failed \(429\)/i.test(
        message,
      )
    ) {
      throwCloudflare(resolve);
    }
    throw error;
  }
};

export const imageUrlsOf = (pages: ChapterPage[]): string[] =>
  pages
    .map((page) => page.url)
    .filter((url): url is string => typeof url === "string" && !!url);
