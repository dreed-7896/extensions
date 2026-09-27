export type SiteId =
  | "nhentai"
  | "hentairead"
  | "hentainexus"
  | "hentai2read"
  | "pandachaika"
  | "ehentai"
  | "hitomi";

const ARCHIVE_SOURCES = new Set([
  "zip",
  "cbz",
  "archive",
  "r2",
  "folder",
  "dir",
  "file",
  "r2-library",
]);

export const isAbsoluteHttpUrl = (url: string): boolean =>
  url.startsWith("http://") || url.startsWith("https://");

export const isRemoteArchiveUrl = (url: string): boolean => {
  const path = url.trim().split("?")[0]?.split("#")[0]?.toLowerCase() ?? "";
  return path.endsWith(".zip") || path.endsWith(".cbz");
};

export const hostOf = (url: string): string => {
  const match = /^https?:\/\/([^/?#]+)/i.exec(url.trim());
  return (match?.[1] ?? "").toLowerCase().replace(/^www\./, "");
};

export const normalizeSite = (value?: string | null): SiteId | null => {
  if (!value?.trim()) return null;
  const key = value.trim().toLowerCase().replace(/\s+/g, "-");
  switch (key) {
    case "nhentai":
    case "nh":
    case "n-hentai":
    case "nhentai.net":
      return "nhentai";
    case "hentairead":
    case "hr":
    case "hentai-read":
    case "hentairead.com":
      return "hentairead";
    case "hentainexus":
    case "hn":
    case "nexus":
    case "hentai-nexus":
    case "hentainexus.com":
      return "hentainexus";
    case "hentai2read":
    case "h2r":
    case "hentai-2-read":
    case "hentai2read.com":
      return "hentai2read";
    case "pandachaika":
    case "panda-chaika":
    case "chaika":
    case "panda":
    case "panda.chaika.moe":
    case "chaika.moe":
      return "pandachaika";
    case "ehentai":
    case "e-hentai":
    case "eh":
    case "exhentai":
    case "ex":
    case "e-hentai.org":
    case "exhentai.org":
      return "ehentai";
    case "hitomi":
    case "hitomi.la":
      return "hitomi";
    default:
      return null;
  }
};

export const siteFromHost = (host: string): SiteId | null => {
  const h = host.toLowerCase().replace(/^www\./, "");
  if (h === "nhentai.net" || h.endsWith(".nhentai.net")) return "nhentai";
  if (
    h === "hentairead.com" ||
    h.endsWith(".hentairead.com") ||
    h === "hencover.xyz" ||
    h === "henread.xyz"
  ) {
    return "hentairead";
  }
  if (h === "hentainexus.com" || h.endsWith(".hentainexus.com")) {
    return "hentainexus";
  }
  if (
    h === "hentai2read.com" ||
    h.endsWith(".hentai2read.com") ||
    h.endsWith(".hentaicdn.com")
  ) {
    return "hentai2read";
  }
  if (h === "panda.chaika.moe" || h === "chaika.moe" || h.endsWith(".chaika.moe")) {
    return "pandachaika";
  }
  if (
    h === "e-hentai.org" ||
    h === "exhentai.org" ||
    h.endsWith(".e-hentai.org") ||
    h.endsWith(".exhentai.org")
  ) {
    return "ehentai";
  }
  if (
    h === "hitomi.la" ||
    h.endsWith(".hitomi.la") ||
    h === "gold-usergeneratedcontent.net" ||
    h.endsWith(".gold-usergeneratedcontent.net")
  ) {
    return "hitomi";
  }
  return null;
};

export const isArchiveSource = (value?: string | null): boolean => {
  const v = value?.trim().toLowerCase().replace(/\s+/g, "-") ?? "";
  return ARCHIVE_SOURCES.has(v);
};

export const tryIdentifySite = (
  url: string,
  explicit?: string | null,
): SiteId | null => {
  if (isArchiveSource(explicit)) return null;
  const named = normalizeSite(explicit);
  if (named) return named;
  if (!url || isRemoteArchiveUrl(url)) return null;
  return siteFromHost(hostOf(url));
};

export const extractRemoteId = (site: SiteId, url: string): string => {
  const trimmed = url.trim();
  switch (site) {
    case "nhentai":
      return (
        /\/g\/(\d+)/i.exec(trimmed)?.[1] ??
        /\b(\d{4,})\b/.exec(trimmed)?.[1] ??
        ""
      );
    case "hentairead": {
      const slug = /\/hentai\/([^/?#]+)/i.exec(trimmed)?.[1];
      if (slug) return decodeURIComponent(slug);
      const last = trimmed.replace(/[?#].*$/, "").replace(/\/+$/, "").split("/").pop();
      return last ? decodeURIComponent(last) : "";
    }
    case "hentainexus":
      return (
        /\/(?:view|read)\/(\d+)/i.exec(trimmed)?.[1] ??
        /\b(\d+)\b/.exec(trimmed)?.[1] ??
        ""
      );
    case "hentai2read":
      return trimmed
        .replace(/[?#].*$/, "")
        .replace(/\/+$/, "")
        .replace(/^https?:\/\/[^/]+/i, "")
        .replace(/^\/+/, "");
    case "pandachaika":
      return /\/archive\/(\d+)/i.exec(trimmed)?.[1] ?? /\b(\d+)\b/.exec(trimmed)?.[1] ?? "";
    case "ehentai": {
      const match = /\/g\/(\d+)\/([0-9a-f]+)/i.exec(trimmed);
      return match ? `${match[1]}/${match[2]}` : "";
    }
    case "hitomi":
      return (
        /galleries\/(\d+)/i.exec(trimmed)?.[1] ??
        /\/(\d+)\.html/i.exec(trimmed)?.[1] ??
        /\b(\d{4,})\b/.exec(trimmed)?.[1] ??
        ""
      );
    default:
      return "";
  }
};

export const canonicalUrl = (site: SiteId, id: string): string => {
  switch (site) {
    case "nhentai":
      return `https://nhentai.net/g/${id}/`;
    case "hentairead":
      return `https://hentairead.com/hentai/${encodeURI(id)}/`;
    case "hentainexus":
      return `https://hentainexus.com/view/${id}`;
    case "hentai2read":
      return `https://hentai2read.com/${id.replace(/^\/+/, "")}/`;
    case "pandachaika":
      return `https://panda.chaika.moe/archive/${id}`;
    case "ehentai": {
      const [gid, token] = id.split("/");
      return token
        ? `https://e-hentai.org/g/${gid}/${token}/`
        : `https://e-hentai.org/g/${gid}/`;
    }
    case "hitomi":
      return `https://hitomi.la/galleries/${id}.html`;
    default:
      return id;
  }
};

export const cloudflareResolveUrl = (url: string): string | undefined => {
  switch (tryIdentifySite(url)) {
    case "hentairead":
      return "https://hentairead.com/hentai/?sortby=new";
    case "hentainexus":
      return "https://hentainexus.com/";
    case "hentai2read":
      return "https://hentai2read.com/";
    case "nhentai":
      return "https://nhentai.net/";
    default:
      return undefined;
  }
};

export const refererForImage = (url: string): string | undefined => {
  const site = siteFromHost(hostOf(url));
  switch (site) {
    case "nhentai":
      return "https://nhentai.net/";
    case "hentairead":
      return "https://hentairead.com/";
    case "hentainexus":
      return "https://hentainexus.com/";
    case "hentai2read":
      return "https://hentai2read.com/";
    case "pandachaika":
      return "https://panda.chaika.moe/";
    case "ehentai":
      return hostOf(url).includes("exhentai")
        ? "https://exhentai.org/"
        : "https://e-hentai.org/";
    case "hitomi":
      return "https://hitomi.la/";
    default:
      return undefined;
  }
};
