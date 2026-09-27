import { getJson, withQuery } from "../_shared/http";
import type {
  GalleryItem,
  Hentai,
  NHConfig,
  PaginatedResponse,
} from "./types";

export const BASE_URL = "https://nhentai.net";
export const API_URL = `${BASE_URL}/api/v2`;

const DEFAULT_CONFIG: NHConfig = {
  image_servers: [1, 2, 3, 4].map((n) => `https://i${n}.nhentai.net`),
  thumb_servers: [1, 2, 3, 4].map((n) => `https://t${n}.nhentai.net`),
};

let cachedConfig: NHConfig | undefined;

export const headers = {
  Referer: `${BASE_URL}/`,
  Accept: "application/json",
};

export const loadConfig = async (): Promise<NHConfig> => {
  if (cachedConfig) return cachedConfig;
  try {
    cachedConfig = await getJson<NHConfig>(`${API_URL}/config`, {
      headers,
      referer: `${BASE_URL}/`,
    });
  } catch {
    cachedConfig = DEFAULT_CONFIG;
  }
  return cachedConfig;
};

export const imageServer = async (): Promise<string> => {
  const config = await loadConfig();
  const servers = config.image_servers?.length
    ? config.image_servers
    : DEFAULT_CONFIG.image_servers;
  return servers[Math.floor(Math.random() * servers.length)]!;
};

export const thumbServer = async (): Promise<string> => {
  const config = await loadConfig();
  const servers = config.thumb_servers?.length
    ? config.thumb_servers
    : DEFAULT_CONFIG.thumb_servers;
  return servers[Math.floor(Math.random() * servers.length)]!;
};

export const searchGalleries = async (options: {
  query: string;
  page: number;
  sort?: string;
}): Promise<PaginatedResponse<GalleryItem>> => {
  // JSC has no URLSearchParams — build the query with encodeURIComponent.
  const url = withQuery(`${API_URL}/search`, {
    query: options.query.trim() || '""',
    page: String(options.page),
    sort: options.sort,
  });
  return getJson<PaginatedResponse<GalleryItem>>(url, {
    headers,
    referer: `${BASE_URL}/`,
  });
};

export const latestGalleries = async (
  page: number,
): Promise<PaginatedResponse<GalleryItem>> =>
  getJson<PaginatedResponse<GalleryItem>>(
    `${API_URL}/galleries?page=${page}`,
    { headers, referer: `${BASE_URL}/` },
  );

export const galleryById = async (id: string): Promise<Hentai> =>
  getJson<Hentai>(`${API_URL}/galleries/${id}`, {
    headers,
    referer: `${BASE_URL}/`,
  });

export const isLastPage = (
  response: PaginatedResponse<unknown>,
  page: number,
): boolean => {
  if (response.num_pages != null) return page >= response.num_pages;
  if (response.total != null && response.per_page > 0) {
    return page * response.per_page >= response.total;
  }
  return response.result.length === 0;
};
