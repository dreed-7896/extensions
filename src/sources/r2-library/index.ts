"use httpclient";

import {
  ContentRating,
  ContentStatus,
  ContentType,
  UITextField,
  UIWebViewButton,
  type Chapter,
  type ChapterPage,
  type Content,
  type HomePage,
  type Item,
  type ItemListRequest,
  type NetworkRequest,
  type PagedItemList,
  type PopulatedForm,
  type SearchRequest,
  type SourceConfiguration,
  type SourceInfo,
  type UIForm,
} from "@suwatte/toolchain/types";
import {
  basename,
  folderIdFromPrefix,
  isArchiveName,
  isChaptersFileName,
  isCoverName,
  isDetailsName,
  isImageName,
  naturalCompare,
  parseChapterNumber,
} from "./archive";
import {
  decodePagesChapter,
  isBucketArchive,
  isFolderChapter,
  parseChaptersJson,
  type ParsedChapter,
} from "./chapters";
import { loadConfig, saveConfig, SETTINGS, type R2Config } from "./config";
import { resolveCoverImage } from "./cover";
import {
  contentFromDetails,
  parseDetailsJson,
  type DetailsFile,
} from "./details";
import { pagesForChapterUrl } from "./gallery";
import { b64ForPage, dataUrlForPage, listImageKeys, openArchiveSession } from "./pages";
import { getObjectBytes, getObjectText, imageUrl, listAll } from "./r2";
import { isAbsoluteHttpUrl, isRemoteArchiveUrl, refererForImage } from "./sites";
import { browserHeaders } from "../_shared/client";
import { bindSourceHttpClient, fetchBytes } from "../_shared/http";
import { CF_RESOLVE } from "../hentairead/constants";

type ChapterMeta = {
  name: string;
  number?: number;
  scanlator?: string;
};

type ChapterRef =
  | (ChapterMeta & { kind: "zip"; key: string })
  | (ChapterMeta & { kind: "dir"; prefix: string })
  | (ChapterMeta & { kind: "loose"; prefix: string })
  | (ChapterMeta & { kind: "url"; url: string })
  | (ChapterMeta & { kind: "pages"; urls: string[] });

type EntryAssets = {
  id: string;
  prefix: string;
  coverKey?: string;
  detailsKey?: string;
  chaptersJsonKey?: string;
  /** Zip/cbz archives kept for cover extraction fallbacks. */
  archives: { key: string; name: string }[];
  chapters: ChapterRef[];
};

const encodeChapterId = (chapter: ChapterRef): string => {
  if (chapter.kind === "zip") return `zip:${encodeURIComponent(chapter.key)}`;
  if (chapter.kind === "dir") return `dir:${encodeURIComponent(chapter.prefix)}`;
  if (chapter.kind === "url") return `url:${encodeURIComponent(chapter.url)}`;
  if (chapter.kind === "pages") {
    return `pages:${encodeURIComponent(JSON.stringify(chapter.urls))}`;
  }
  return `loose:${encodeURIComponent(chapter.prefix)}`;
};

const decodeChapterId = (
  chapterId: string,
): { kind: ChapterRef["kind"]; value: string } => {
  if (chapterId.startsWith("zip:")) {
    return { kind: "zip", value: decodeURIComponent(chapterId.slice(4)) };
  }
  if (chapterId.startsWith("dir:")) {
    return { kind: "dir", value: decodeURIComponent(chapterId.slice(4)) };
  }
  if (chapterId.startsWith("loose:")) {
    return { kind: "loose", value: decodeURIComponent(chapterId.slice(6)) };
  }
  if (chapterId.startsWith("url:")) {
    return { kind: "url", value: decodeURIComponent(chapterId.slice(4)) };
  }
  if (chapterId.startsWith("pages:")) {
    return { kind: "pages", value: decodeURIComponent(chapterId.slice(6)) };
  }
  // Legacy ids were bare encodeURIComponent(archiveKey).
  return { kind: "zip", value: decodeURIComponent(chapterId) };
};

/** Empty prefix = bucket root. Never emit a bare "/" list prefix. */
const rootPrefix = (prefix: string): string => {
  const value = prefix.replace(/^\/+|\/+$/g, "");
  return value ? `${value}/` : "";
};

const buildEntries = async (
  config: R2Config,
  folderPrefixes: string[],
  root: string,
): Promise<EntryAssets[]> => {
  const entries: EntryAssets[] = [];
  const rootId = root.replace(/\/+$/, "");

  for (const prefix of folderPrefixes) {
    const id = folderIdFromPrefix(prefix, rootId);
    if (!id || id.includes("/")) continue;

    // Delimiter listing: chapter subfolders as common prefixes + files at this level.
    const listed = await listAll(config, prefix, "/");
    const coverKey = listed.objects.find((object) =>
      isCoverName(basename(object.key)),
    )?.key;
    const detailsKey = listed.objects.find((object) =>
      isDetailsName(basename(object.key)),
    )?.key;
    const chaptersJsonKey = listed.objects.find((object) =>
      isChaptersFileName(basename(object.key)),
    )?.key;
    const archives = listed.objects
      .filter((object) => isArchiveName(basename(object.key)))
      .map((object) => ({
        key: object.key,
        name: basename(object.key),
      }))
      .sort((left, right) => naturalCompare(left.name, right.name));

    const folders: ChapterRef[] = listed.prefixes
      .map((childPrefix) => {
        const name = folderIdFromPrefix(childPrefix, prefix.replace(/\/+$/, ""));
        if (!name || name.includes("/")) return null;
        return {
          kind: "dir" as const,
          prefix: childPrefix.endsWith("/") ? childPrefix : `${childPrefix}/`,
          name,
        };
      })
      .filter((value): value is ChapterRef & { kind: "dir" } => !!value)
      .sort((left, right) => naturalCompare(left.name, right.name));

    const looseImages = listed.objects.filter((object) => {
      const name = basename(object.key);
      return isImageName(name) && !isCoverName(name);
    });

    const chapters: ChapterRef[] = [
      ...folders,
      ...archives.map((archive) => ({
        kind: "zip" as const,
        key: archive.key,
        name: archive.name,
      })),
    ];

    // Images directly under the title folder = single loose chapter.
    if (looseImages.length && !folders.length) {
      chapters.unshift({
        kind: "loose",
        prefix,
        name: "Chapter",
      });
    } else if (looseImages.length && folders.length) {
      // Mixed layout: keep loose images as their own chapter at the end.
      chapters.push({
        kind: "loose",
        prefix,
        name: "Root",
      });
    }

    chapters.sort((left, right) => naturalCompare(left.name, right.name));

    // Prefer explicit cover; else first loose image at title root.
    const inferredCover =
      coverKey ??
      looseImages.sort((a, b) =>
        naturalCompare(basename(a.key), basename(b.key)),
      )[0]?.key;

    if (!chapters.length && !detailsKey && !chaptersJsonKey && !inferredCover) continue;

    entries.push({
      id,
      prefix,
      coverKey: inferredCover,
      detailsKey,
      chaptersJsonKey,
      archives,
      chapters,
    });
  }

  return entries.sort((left, right) => naturalCompare(left.id, right.id));
};

const listEntriesAtRoot = async (
  config: R2Config,
  root: string,
): Promise<EntryAssets[]> => {
  const listed = await listAll(config, root, "/");
  let folderPrefixes = listed.prefixes;

  if (!folderPrefixes.length) {
    const flat = await listAll(config, root);
    const ids = new Set<string>();
    for (const object of flat.objects) {
      const rest =
        root && object.key.startsWith(root)
          ? object.key.slice(root.length)
          : object.key;
      const id = rest.split("/").find((part) => part.length > 0);
      if (id) ids.add(id);
    }
    folderPrefixes = [...ids].map((id) => `${root}${id}/`);
  }

  return buildEntries(config, folderPrefixes, root);
};

const listEntries = async (config: R2Config): Promise<EntryAssets[]> => {
  const configuredRoot = rootPrefix(config.prefix);
  let entries = await listEntriesAtRoot(config, configuredRoot);

  // If Root Prefix is still "manga" but titles sit at bucket root, fall back.
  if (!entries.length && configuredRoot) {
    entries = await listEntriesAtRoot(config, "");
  }

  return entries;
};

const loadDetails = async (
  config: R2Config,
  entry: EntryAssets,
): Promise<DetailsFile | null> => {
  if (!entry.detailsKey) return null;
  try {
    return parseDetailsJson(await getObjectText(config, entry.detailsKey));
  } catch (error) {
    console.log(`Failed to parse details for ${entry.id}: ${String(error)}`);
    return null;
  }
};

const findEntry = async (
  config: R2Config,
  contentId: string,
): Promise<EntryAssets> => {
  const entry = (await listEntries(config)).find((item) => item.id === contentId);
  if (!entry) {
    const root = rootPrefix(config.prefix) || "(bucket root)/";
    throw new Error(`Manga folder not found under ${root}${contentId}/`);
  }
  return entry;
};

const jsonChapterToRef = (chapter: ParsedChapter, seriesPrefix: string): ChapterRef => {
  const url = chapter.url;
  const meta = {
    name: chapter.title,
    number: chapter.number,
    scanlator: chapter.scanlator,
  };
  const listed = decodePagesChapter(url);
  if (listed) return { kind: "pages", urls: listed, ...meta };
  if (isFolderChapter(url)) {
    return {
      kind: "dir",
      prefix: url.endsWith("/") ? url : `${url}/`,
      ...meta,
    };
  }
  if (isBucketArchive(url)) return { kind: "zip", key: url, ...meta };
  if (isAbsoluteHttpUrl(url) || url.startsWith("pages:")) {
    return { kind: "url", url, ...meta };
  }
  const prefix = seriesPrefix.replace(/\/+$/, "");
  const resolved = url.startsWith(prefix) || !prefix ? url : `${prefix}/${url.replace(/^\/+/, "")}`;
  if (isArchiveName(basename(resolved))) return { kind: "zip", key: resolved, ...meta };
  if (resolved.endsWith("/")) return { kind: "dir", prefix: resolved, ...meta };
  return { kind: "url", url: resolved, ...meta };
};

const loadJsonChapters = async (
  config: R2Config,
  entry: EntryAssets,
  details?: DetailsFile | null,
): Promise<ChapterRef[]> => {
  const parsed: ParsedChapter[] = [];
  if (details) {
    if (details.chapters != null) {
      parsed.push(
        ...parseChaptersJson(
          JSON.stringify({ chapters: details.chapters }),
          entry.prefix,
        ),
      );
    }
  } else if (entry.detailsKey) {
    try {
      parsed.push(
        ...parseChaptersJson(await getObjectText(config, entry.detailsKey), entry.prefix),
      );
    } catch (error) {
      console.log(`Failed to parse details.json chapters for ${entry.id}: ${String(error)}`);
    }
  }
  if (entry.chaptersJsonKey) {
    try {
      parsed.push(
        ...parseChaptersJson(
          await getObjectText(config, entry.chaptersJsonKey),
          entry.prefix,
        ),
      );
    } catch (error) {
      console.log(`Failed to parse chapters.json for ${entry.id}: ${String(error)}`);
    }
  }
  return parsed.map((chapter) => jsonChapterToRef(chapter, entry.prefix));
};

const chapterFingerprint = (chapter: ChapterRef): string => {
  if (chapter.kind === "zip") return `zip:${chapter.key}`;
  if (chapter.kind === "dir") return `dir:${chapter.prefix}`;
  if (chapter.kind === "loose") return `loose:${chapter.prefix}`;
  if (chapter.kind === "url") return `url:${chapter.url}`;
  return `pages:${chapter.urls.join("|")}`;
};

const chapterSortNumber = (chapter: ChapterRef): number =>
  chapter.number ?? parseChapterNumber(chapter.name, 0);

const mergedChapters = async (
  config: R2Config,
  entry: EntryAssets,
  details?: DetailsFile | null,
): Promise<ChapterRef[]> => {
  const extra = await loadJsonChapters(config, entry, details);
  const seen = new Set<string>();
  const merged: ChapterRef[] = [];
  for (const chapter of [...entry.chapters, ...extra]) {
    const key = chapterFingerprint(chapter);
    if (seen.has(key)) continue;
    seen.add(key);
    merged.push(chapter);
  }
  merged.sort((left, right) => {
    const delta = chapterSortNumber(left) - chapterSortNumber(right);
    return delta !== 0 ? delta : naturalCompare(left.name, right.name);
  });
  return merged;
};

const coverTargets = (chapters: ChapterRef[]) => ({
  folders: chapters
    .filter((chapter): chapter is ChapterRef & { kind: "dir" } => chapter.kind === "dir")
    .map((chapter) => ({ prefix: chapter.prefix, name: chapter.name })),
  remotes: chapters
    .filter((chapter): chapter is ChapterRef & { kind: "url" } => chapter.kind === "url")
    .map((chapter) => ({ url: chapter.url, name: chapter.name })),
});

const toItem = async (
  config: R2Config,
  entry: EntryAssets,
  details: DetailsFile | null,
  title?: string,
): Promise<Item> => ({
  id: entry.id,
  title: title ?? details?.title ?? entry.id,
  coverImage: await resolveCoverImage({
    config,
    contentId: entry.id,
    seriesPrefix: entry.prefix,
    coverKey: entry.coverKey,
    details,
    archives: entry.archives,
    folders: entry.chapters
      .filter((chapter): chapter is ChapterRef & { kind: "dir" } => chapter.kind === "dir")
      .map((chapter) => ({ prefix: chapter.prefix, name: chapter.name })),
    remotes: [],
    allowArchiveExtract: false,
  }),
  rating: ContentRating.MATURE,
});

export default class Target {
  /**
   * Owned client (no validateStatus: true) so native CloudflareError fires
   * and Resolve cookies stick here. Gallery fetches bind to this client.
   */
  client = (() => {
    const http = new HttpClient({
      timeout: 45_000,
      headers: browserHeaders(),
    });
    bindSourceHttpClient(http);
    return http;
  })();

  static info: SourceInfo = {
    id: "en.r2-library",
    name: "R2 Library",
    version: 2.0,
    website: "https://developers.cloudflare.com/r2/",
    thumbnail: "r2-library.png",
    languages: ["en"],
    rating: ContentRating.MATURE,
  };

  getConfiguration = (): SourceConfiguration => ({
    endpoint: ["anilist", "mal"],
    useClientForImageRequests: true,
  });

  getSettingsPage = async (): Promise<UIForm> => {
    const accountId = (await ObjectStore.string(SETTINGS.accountId)) ?? "";
    const accessKeyId = (await ObjectStore.string(SETTINGS.accessKeyId)) ?? "";
    const bucket = (await ObjectStore.string(SETTINGS.bucket)) ?? "";
    const endpoint = (await ObjectStore.string(SETTINGS.endpoint)) ?? "";
    const prefix = (await ObjectStore.string(SETTINGS.prefix)) ?? "";
    const publicBaseUrl = (await ObjectStore.string(SETTINGS.publicBaseUrl)) ?? "";
    const hasSecret = !!(await ObjectStore.string(SETTINGS.secretAccessKey));

    return {
      sections: [
        {
          header: "Cloudflare R2",
          footer:
            "Object Read API token required. Leave Endpoint blank for the default R2 S3 URL. Leave Root Prefix empty when title folders sit at the bucket root. Mix folder/.cbz chapters with gallery URLs in details.json chapters[].",
          views: [
            UITextField({
              id: SETTINGS.accountId,
              title: "Account ID",
              currentValue: accountId,
              placeholder: "Cloudflare account id",
            }),
            UITextField({
              id: SETTINGS.accessKeyId,
              title: "Access Key ID",
              currentValue: accessKeyId,
            }),
            UITextField({
              id: SETTINGS.secretAccessKey,
              title: "Secret Access Key",
              placeholder: hasSecret
                ? "Leave blank to keep the saved secret"
                : "R2 secret access key",
              isSecure: true,
            }),
            UITextField({
              id: SETTINGS.bucket,
              title: "Bucket",
              currentValue: bucket,
              placeholder: "manga",
            }),
            UITextField({
              id: SETTINGS.endpoint,
              title: "S3 Endpoint (optional)",
              currentValue: endpoint,
              placeholder: "https://<accountId>.r2.cloudflarestorage.com",
            }),
            UITextField({
              id: SETTINGS.prefix,
              title: "Root Prefix",
              currentValue: prefix,
              placeholder: "(empty = bucket root)",
            }),
            UITextField({
              id: SETTINGS.publicBaseUrl,
              title: "Public image URL (optional)",
              currentValue: publicBaseUrl,
              placeholder: "https://pub-….r2.dev",
            }),
          ],
        },
        {
          header: "Cloudflare",
          footer:
            "Gallery hosts (HentaiRead, etc.) use the same Resolve WebView as the standalone sources. Complete the check, then reopen the chapter. Safari cookies are not this source's jar.",
          views: [
            UIWebViewButton({
              title: "Open HentaiRead Challenge",
              url: { url: CF_RESOLVE },
            }),
          ],
        },
      ],
    };
  };

  onFormSubmitted = async (_id: string, data: PopulatedForm): Promise<void> => {
    await saveConfig(data as Record<string, unknown>);
  };

  getHomePage = async (): Promise<HomePage> => ({
    feeds: [
      {
        id: "library",
        title: "Library",
        content: {
          list: {
            key: "all",
            disableSorting: true,
          },
        },
      },
    ],
  });

  getSearchResults = async (
    request: SearchRequest,
    page: number,
  ): Promise<PagedItemList> => {
    if (page > 1) {
      return { items: [], isLastPage: true, total: 0 };
    }

    const config = await loadConfig();
    const entries = await listEntries(config);
    const query = request.query?.trim().toLowerCase();

    const items: Item[] = [];
    for (const entry of entries) {
      const details = await loadDetails(config, entry);
      const title = details?.title ?? entry.id;
      if (query) {
        const haystack = [
          title,
          entry.id,
          ...(details?.additionalTitles ?? []),
          details?.summary ?? "",
        ]
          .join(" ")
          .toLowerCase();
        if (!haystack.includes(query)) continue;
      }
      items.push(await toItem(config, entry, details, title));
    }

    return { items, isLastPage: true, total: items.length };
  };

  getItemList = async (
    _request: ItemListRequest,
    page: number,
  ): Promise<PagedItemList> => {
    if (page > 1) {
      return { items: [], isLastPage: true, total: 0 };
    }

    const config = await loadConfig();
    const entries = await listEntries(config);
    const items: Item[] = [];
    for (const entry of entries) {
      const details = await loadDetails(config, entry);
      items.push(
        await toItem(config, entry, details, details?.title ?? entry.id),
      );
    }

    return { items, isLastPage: true, total: items.length };
  };

  getContent = async (contentId: string): Promise<Content> => {
    const config = await loadConfig();
    const entry = await findEntry(config, contentId);
    const details = await loadDetails(config, entry);
    const chapters = await mergedChapters(config, entry, details);
    const { folders } = coverTargets(chapters);

    const coverImage = await resolveCoverImage({
      config,
      contentId,
      seriesPrefix: entry.prefix,
      coverKey: entry.coverKey,
      details,
      archives: entry.archives,
      folders,
      remotes: [],
      allowArchiveExtract: false,
    });

    if (details) {
      return contentFromDetails(
        { ...details, cover: undefined },
        { id: contentId, coverImage: coverImage ?? "" },
      );
    }

    return {
      title: contentId,
      coverImage: coverImage ?? "",
      rating: ContentRating.MATURE,
      status: ContentStatus.UNKNOWN,
      contentType: ContentType.MANGA,
      summary: `Imported from R2 folder ${contentId}/`,
      additionalDetails: {
        Folder: contentId,
        Chapters: String(chapters.length),
      },
    };
  };

  getChapters = async (contentId: string): Promise<Chapter[]> => {
    const config = await loadConfig();
    const entry = await findEntry(config, contentId);
    const details = await loadDetails(config, entry);
    const chapters = await mergedChapters(config, entry, details);

    return chapters.map((chapter, index) => ({
      id: encodeChapterId(chapter),
      index,
      number: chapter.number ?? parseChapterNumber(chapter.name, index + 1),
      language: "en",
      title: chapter.name.replace(/\.(cbz|zip)$/i, ""),
      date: new Date(),
      providers: chapter.scanlator
        ? [
            {
              id: chapter.scanlator.toLowerCase().replace(/\s+/g, "-"),
              name: chapter.scanlator,
              links: [],
            },
          ]
        : undefined,
    }));
  };

  getChapterPages = async (
    _contentId: string,
    chapterId: string,
  ): Promise<ChapterPage[]> => {
    const config = await loadConfig();
    const decoded = decodeChapterId(chapterId);

    if (decoded.kind === "dir" || decoded.kind === "loose") {
      const prefix = decoded.value.endsWith("/")
        ? decoded.value
        : `${decoded.value}/`;
      // Folder chapters: list image objects and return presigned URLs.
      // No unzip / no 80 MiB cap — Suwatte loads pages like any remote source.
      const listed =
        decoded.kind === "loose"
          ? await listAll(config, prefix, "/")
          : await listAll(config, prefix);
      const keys =
        decoded.kind === "loose"
          ? listImageKeys(
              listed.objects
                .filter((object) => {
                  const name = basename(object.key);
                  return isImageName(name) && !isCoverName(name);
                })
                .map((object) => object.key),
            )
          : listImageKeys(listed.objects.map((object) => object.key));
      if (!keys.length) {
        throw new Error(`No images found under ${prefix}`);
      }
      return keys.map((key) => ({
        url: imageUrl(config, key, 60 * 60 * 6),
      }));
    }

    if (decoded.kind === "pages") {
      const urls = JSON.parse(decoded.value) as unknown;
      if (!Array.isArray(urls)) throw new Error("Invalid pages chapter");
      return urls
        .filter((entry): entry is string => typeof entry === "string" && !!entry)
        .map((url) => ({ url }));
    }

    if (decoded.kind === "url") {
      if (isRemoteArchiveUrl(decoded.value)) {
        const bytes = await fetchBytes(decoded.value, { timeout: 180_000 });
        const { urls } = openArchiveSession(bytes);
        return urls.map((url) => {
          const b64 = b64ForPage(url);
          return b64 ? { b64 } : { url };
        });
      }
      return pagesForChapterUrl(decoded.value);
    }

    const bytes = await getObjectBytes(config, decoded.value);
    const { urls } = openArchiveSession(bytes);
    return urls.map((url) => {
      const b64 = b64ForPage(url);
      return b64 ? { b64 } : { url };
    });
  };

  /**
   * Session zip pages → data URLs. Gallery CDNs get a Referer so CF/hotlink
   * checks match chapter reads.
   */
  willRequestImage = async (
    request: NetworkRequest | string,
  ): Promise<NetworkRequest> => {
    const url = typeof request === "string" ? request : request.url;
    const dataUrl = dataUrlForPage(url);
    if (dataUrl) {
      return typeof request === "string"
        ? { url: dataUrl }
        : { ...request, url: dataUrl };
    }
    const referer = refererForImage(url);
    if (!referer) {
      return typeof request === "string" ? { url: request } : request;
    }
    const headers = typeof request === "string" ? {} : { ...(request.headers ?? {}) };
    if (!headers.Referer && !headers.referer) headers.Referer = referer;
    return typeof request === "string"
      ? { url, headers }
      : { ...request, url, headers };
  };
}
