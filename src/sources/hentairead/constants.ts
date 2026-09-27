export const BASE = "https://hentairead.com";
export const MANGA = "hentai";

/** Nested listing — WKWebView often blanks on `/`. Keiyoushi latest URL. */
export const CF_RESOLVE = `${BASE}/hentai/?sortby=new`;

export const IMAGE_ACCEPT =
  "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8";

export const SORTS = [
  { id: "new", title: "Latest" },
  { id: "alphabet", title: "A-Z" },
  { id: "rating", title: "Rating" },
  { id: "views", title: "Views" },
] as const;

export const TYPES = [
  { id: "4", title: "Doujinshi" },
  { id: "52", title: "Manga" },
  { id: "4798", title: "Artist CG" },
  { id: "36278", title: "Western" },
] as const;

export const TEXT_FILTERS = [
  { id: "tags", name: "Tags", type: "manga_tag", hint: "Comma-separated. Prefix with - to exclude." },
  { id: "artists", name: "Artists", type: "artist", hint: "Comma-separated" },
  { id: "circles", name: "Circles", type: "circle", hint: "Comma-separated" },
  { id: "characters", name: "Characters", type: "character", hint: "Comma-separated" },
  { id: "collections", name: "Collections", type: "collection", hint: "Comma-separated" },
  { id: "scanlators", name: "Scanlators", type: "scanlator", hint: "Comma-separated" },
  { id: "conventions", name: "Conventions", type: "convention", hint: "Comma-separated" },
] as const;
