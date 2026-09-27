"use httpclient";

import {
  ContentRating,
  ContentStatus,
  ContentType,
  type Chapter,
  type ChapterPage,
  type Content,
  type HomePage,
  type Item,
  type ItemListRequest,
  type PagedItemList,
  type SearchRequest,
  type SourceConfiguration,
  type SourceInfo,
  type Tag,
} from "@suwatte/toolchain/types";
import { fetchBytes, fetchJson, withQuery } from "../_shared/http";
import { matureItem } from "../_shared/item";
import { extractZipImagePages } from "../_shared/zip";

const BASE_URL = "https://panda.chaika.moe";

type LongArchive = {
  id: number;
  title: string;
  title_jpn?: string | null;
  thumbnail?: string;
  posted?: number | null;
  public_date?: number | null;
  filecount?: number;
  filesize?: number;
  tags?: string[];
  uploader?: string;
  download?: string;
  category?: string;
  rating?: string | number;
};

type ArchiveResponse = {
  archives: LongArchive[];
  has_next: boolean;
};

type ArchiveDetail = {
  download: string;
  posted: number;
  title: string;
  title_jpn?: string;
  tags?: string[];
  filecount?: number;
  filesize?: number;
  uploader?: string;
  category?: string;
  rating?: number;
};

const filterTags = (
  tags: string[],
  include: string,
  exclude: string[] = [],
): string[] =>
  tags
    .filter(
      (tag) =>
        tag.startsWith(`${include}:`) &&
        !exclude.some((prefix) => tag.startsWith(`${prefix}:`)),
    )
    .map((tag) =>
      tag
        .substring(tag.indexOf(":") + 1)
        .replace(/_/g, " ")
        .split(" ")
        .map((part) =>
          part ? part[0]!.toUpperCase() + part.slice(1).toLowerCase() : part,
        )
        .join(" "),
    );

const readableSize = (bytes: number): string => {
  if (bytes >= 300_000_000) return `${(bytes / 1_000_000_000).toFixed(2)} GB`;
  if (bytes >= 100_000) return `${(bytes / 1_000_000).toFixed(2)} MB`;
  if (bytes >= 1000) return `${(bytes / 1000).toFixed(2)} kB`;
  return `${bytes} B`;
};

const archiveToItem = (archive: LongArchive): Item =>
  matureItem({
    id: String(archive.id),
    title: archive.title,
    coverImage: archive.thumbnail,
    subtitle: archive.category,
  });

const searchUrl = (
  page: number,
  options: {
    title?: string;
    tags?: string;
    sort?: string;
    category?: string;
  } = {},
): string =>
  withQuery(`${BASE_URL}/search/`, {
    title: options.title,
    tags: options.tags ?? "",
    sort: options.sort ?? "public_date",
    category: options.category,
    page: String(page),
    apply: "",
    json: "",
  });

const fetchArchiveList = async (url: string): Promise<PagedItemList> => {
  const data = await fetchJson<ArchiveResponse>(url, {
    referer: BASE_URL,
  });
  return {
    items: data.archives.map(archiveToItem),
    isLastPage: !data.has_next,
  };
};

const buildSummary = (archive: {
  title_jpn?: string | null;
  tags?: string[];
  uploader?: string;
  filecount?: number;
  filesize?: number;
  posted?: number | null;
  public_date?: number | null;
  category?: string;
}): string => {
  const tags = archive.tags ?? [];
  const artists = filterTags(tags, "artist");
  const groups = filterTags(tags, "group");
  const parodies = filterTags(tags, "parody");
  const characters = filterTags(tags, "character");
  const male = filterTags(tags, "male");
  const female = filterTags(tags, "female");
  const others = tags
    .filter(
      (tag) =>
        !["female", "male", "artist", "publisher", "group", "parody"].some(
          (prefix) => tag.startsWith(`${prefix}:`),
        ),
    )
    .map((tag) => tag.replace(/_/g, " "));

  const lines = [
    archive.uploader ? `Uploader: ${archive.uploader || "Anonymous"}` : "",
    archive.category ? `Category: ${archive.category}` : "",
    groups.length ? `Groups: ${groups.join(", ")}` : "",
    artists.length ? `Artists: ${artists.join(", ")}` : "",
    parodies.length ? `Parodies: ${parodies.join(", ")}` : "",
    characters.length ? `Characters: ${characters.join(", ")}` : "",
    male.length ? `Male tags: ${male.join(", ")}` : "",
    female.length ? `Female tags: ${female.join(", ")}` : "",
    others.length ? `Other tags: ${others.join(", ")}` : "",
    archive.title_jpn ? `Japanese Title: ${archive.title_jpn}` : "",
    archive.filecount != null ? `Pages: ${archive.filecount}` : "",
    archive.filesize != null ? `File Size: ${readableSize(archive.filesize)}` : "",
    archive.public_date
      ? `Public Date: ${new Date(archive.public_date * 1000).toUTCString()}`
      : "",
    archive.posted
      ? `Posted: ${new Date(archive.posted * 1000).toUTCString()}`
      : "",
  ];
  return lines.filter(Boolean).join("\n");
};

const tagsToGenres = (tags: string[] = []): Tag[] =>
  tags.map((tag) => ({
    id: tag.toLowerCase().replace(/\s+/g, "-"),
    title: tag.replace(/_/g, " "),
  }));

export default class Target {
  static info: SourceInfo = {
    id: "all.pandachaika",
    name: "PandaChaika",
    version: 1.2,
    website: BASE_URL,
    thumbnail: "pandachaika.png",
    languages: ["all"],
    rating: ContentRating.MATURE,
  };

  getConfiguration = (): SourceConfiguration => ({
    imageReferer: `${BASE_URL}/`,
  });

  getHomePage = async (): Promise<HomePage> => ({
    feeds: [
      {
        id: "latest",
        title: "Latest",
        content: {
          list: { key: "latest", disableSorting: true },
        },
      },
      {
        id: "popular",
        title: "Popular",
        content: {
          list: { key: "popular", disableSorting: true },
        },
      },
    ],
  });

  getSearchResults = async (
    request: SearchRequest,
    page: number,
  ): Promise<PagedItemList> => {
    const query = request.query?.trim() ?? "";

    // id:12345 (bare digits use title search like the Kotlin extension)
    if (query.startsWith("id:")) {
      if (page > 1) return { items: [], isLastPage: true };
      const id = query.slice(3).trim();
      try {
        const detail = await fetchJson<ArchiveDetail>(
          `${BASE_URL}/api?archive=${id}`,
          { referer: BASE_URL },
        );
        const listed = await fetchJson<ArchiveResponse>(
          searchUrl(1, { title: detail.title }),
          { referer: BASE_URL },
        );
        const match =
          listed.archives.find((entry) => entry.id === Number(id)) ??
          ({
            id: Number(id),
            title: detail.title,
          } satisfies LongArchive);
        return { items: [archiveToItem(match)], isLastPage: true };
      } catch {
        return { items: [], isLastPage: true };
      }
    }

    // ehentai: / fakku: / source: shortcuts
    if (/^(ehentai:|fakku:|source:|https?:\/\/)/i.test(query)) {
      if (page > 1) return { items: [], isLastPage: true };
      let qsearch = query;
      if (query.startsWith("ehentai:")) {
        qsearch = `https://e-hentai.org/g/${query.slice("ehentai:".length)}`;
      } else if (query.startsWith("fakku:")) {
        qsearch = `https://www.fakku.net/hentai/${query.slice("fakku:".length)}`;
      } else if (query.startsWith("source:")) {
        qsearch = query.slice("source:".length);
      }
      const url = withQuery(`${BASE_URL}/search/`, {
        qsearch,
        json: "",
      });
      const data = await fetchJson<ArchiveResponse>(url, { referer: BASE_URL });
      const first = data.archives[0];
      return {
        items: first ? [archiveToItem(first)] : [],
        isLastPage: true,
      };
    }

    return fetchArchiveList(searchUrl(page, { title: query || undefined }));
  };

  getItemList = async (
    request: ItemListRequest,
    page: number,
  ): Promise<PagedItemList> => {
    const sort = request.key === "popular" ? "rating" : "public_date";
    return fetchArchiveList(searchUrl(page, { sort }));
  };

  getContent = async (contentId: string): Promise<Content> => {
    const detail = await fetchJson<ArchiveDetail>(
      `${BASE_URL}/api?archive=${contentId}`,
      { referer: BASE_URL },
    );

    // Prefer listing payload for thumbnail when available.
    let thumbnail: string | undefined;
    try {
      const listed = await fetchJson<ArchiveResponse>(
        searchUrl(1, { title: detail.title }),
        { referer: BASE_URL },
      );
      thumbnail = listed.archives.find(
        (entry) => String(entry.id) === contentId,
      )?.thumbnail;
    } catch {
      // ignore
    }

    const tags = detail.tags ?? [];
    return {
      title: detail.title,
      coverImage:
        thumbnail ??
        `https://static.chaika.moe/media/images/thumbs/archive_${contentId}/thumb2.jpg`,
      webUrl: `${BASE_URL}/archive/${contentId}`,
      rating: ContentRating.MATURE,
      status: ContentStatus.COMPLETED,
      contentType: ContentType.COMIC,
      summary: buildSummary(detail),
      additionalTitles: detail.title_jpn ? [detail.title_jpn] : undefined,
      genres: tags.length ? tagsToGenres(tags) : undefined,
      statistics:
        detail.rating != null
          ? { rating: Number(detail.rating) || undefined }
          : undefined,
    };
  };

  getChapters = async (contentId: string): Promise<Chapter[]> => {
    const detail = await fetchJson<ArchiveDetail>(
      `${BASE_URL}/api?archive=${contentId}`,
      { referer: BASE_URL },
    );
    const downloadPath = detail.download.startsWith("http")
      ? (detail.download.match(/^https?:\/\/[^/?#]+(\/[^?#]*)?/i)?.[1] ??
          detail.download)
          .replace(/\/download\/?$/, "")
      : detail.download.replace(/\/download\/?$/, "");

    return [
      {
        id: downloadPath || `/archive/${contentId}`,
        index: 0,
        number: 1,
        title: "Chapter",
        language: "all",
        date: new Date((detail.posted || 0) * 1000),
        webUrl: `${BASE_URL}/archive/${contentId}`,
      },
    ];
  };

  getChapterPages = async (
    contentId: string,
    chapterId: string,
  ): Promise<ChapterPage[]> => {
    const downloadUrl = chapterId.includes("/download")
      ? absoluteDownload(chapterId)
      : `${BASE_URL}/archive/${contentId}/download/`;

    const bytes = await fetchBytes(downloadUrl, {
      referer: BASE_URL,
      timeout: 180_000,
    });
    return extractZipImagePages(bytes).map((page) => ({ b64: page.b64 }));
  };
}

const absoluteDownload = (pathOrUrl: string): string => {
  if (/^https?:\/\//i.test(pathOrUrl)) {
    return pathOrUrl.endsWith("/download/") || pathOrUrl.endsWith("/download")
      ? pathOrUrl
      : `${pathOrUrl.replace(/\/+$/, "")}/download/`;
  }
  const base = pathOrUrl.startsWith("/") ? pathOrUrl : `/${pathOrUrl}`;
  if (base.endsWith("/download/") || base.endsWith("/download")) {
    return `${BASE_URL}${base}`;
  }
  return `${BASE_URL}${base.replace(/\/+$/, "")}/download/`;
};
