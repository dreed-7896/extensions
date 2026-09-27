/** Shared HTTP helpers for Suwatte JSC sources. */

import {
  cloudflareFromHeaders,
  isCloudflareError,
  looksLikeCloudflare,
  originOf,
  throwCloudflare,
} from "./cloudflare";

export type SourceHttpClient = InstanceType<typeof HttpClient>;

let fallbackClient: SourceHttpClient | undefined;
let boundClient: SourceHttpClient | undefined;

/** Fallback only — CF-protected sources MUST pass their own `client`. */
export const http = (): SourceHttpClient => {
  if (!fallbackClient) {
    fallbackClient = new HttpClient({
      timeout: 45_000,
      validateStatus: () => true,
    });
  }
  return fallbackClient;
};

/**
 * Bind the source's `this.client` so gallery fetches share the CF cookie jar.
 * Do NOT bind the R2/S3 client — that one uses validateStatus: () => true and
 * skips native CloudflareError throws.
 */
export const bindSourceHttpClient = (client: SourceHttpClient): void => {
  boundClient = client;
};

export type FetchOptions = {
  /** Prefer the source's `this.client` so CF cookies apply. */
  client?: SourceHttpClient;
  headers?: Record<string, string>;
  referer?: string;
  timeout?: number;
  /** Override CF resolution URL (defaults to request origin). */
  cloudflareResolutionURL?: string;
};

const resolveClient = (options?: FetchOptions): SourceHttpClient =>
  options?.client ?? boundClient ?? http();

const cloudflareResponse = (
  error: unknown,
):
  | { status?: number; headers?: unknown; text?: () => Promise<string> }
  | undefined =>
  (error as { response?: { status?: number; headers?: unknown; text?: () => Promise<string> } })
    .response;

const rethrowCloudflare = async (
  error: unknown,
  url: string,
  resolutionURL?: string,
): Promise<never> => {
  const resolution = resolutionURL ?? originOf(url);
  if (isCloudflareError(error)) throwCloudflare(resolution);
  const response = cloudflareResponse(error);
  if (response) {
    const body = (await response.text?.().catch(() => "")) ?? "";
    if (
      looksLikeCloudflare(body) ||
      cloudflareFromHeaders(response.status ?? 0, response.headers as never)
    ) {
      throwCloudflare(resolution);
    }
  }
  throw error;
};

const mergeHeaders = (
  options?: FetchOptions,
): Record<string, string> | undefined => {
  const headers: Record<string, string> = { ...(options?.headers ?? {}) };
  if (options?.referer) headers.Referer = options.referer;
  return Object.keys(headers).length ? headers : undefined;
};

const assertNotCloudflare = (
  url: string,
  status: number,
  body: string,
  headers:
    | { get(name: string): string | null }
    | Record<string, unknown>
    | undefined
    | null,
  resolutionURL?: string,
): void => {
  const resolution = resolutionURL ?? originOf(url);
  if (cloudflareFromHeaders(status, headers) || looksLikeCloudflare(body)) {
    throwCloudflare(resolution);
  }
};

export const fetchText = async (
  url: string,
  options?: FetchOptions,
): Promise<string> => {
  try {
    const response = await resolveClient(options).request({
      url,
      method: "GET",
      headers: mergeHeaders(options),
      timeout: options?.timeout,
      cloudflareResolutionURL: options?.cloudflareResolutionURL,
    } as Parameters<SourceHttpClient["request"]>[0]);
    // Throw on CF headers before waiting to fully materialize/parse body UI-side.
    if (cloudflareFromHeaders(response.status, response.headers)) {
      throwCloudflare(options?.cloudflareResolutionURL ?? originOf(url));
    }
    const body = await response.text();
    assertNotCloudflare(
      url,
      response.status,
      body,
      response.headers,
      options?.cloudflareResolutionURL,
    );
    if (!response.ok) {
      throw new Error(
        `GET ${url} failed (${response.status}): ${body.slice(0, 180)}`,
      );
    }
    return body;
  } catch (error) {
    return rethrowCloudflare(error, url, options?.cloudflareResolutionURL);
  }
};

export const postForm = async (
  url: string,
  fields: Record<string, string> | [string, string][],
  options?: FetchOptions,
): Promise<string> => {
  const entries = Array.isArray(fields) ? fields : Object.entries(fields);
  const body = entries
    .map(
      ([key, value]) =>
        `${encodeURIComponent(key)}=${encodeURIComponent(value)}`,
    )
    .join("&");

  const response = await resolveClient(options).request({
    url,
    method: "POST",
    body,
    headers: {
      "Content-Type": "application/x-www-form-urlencoded; charset=UTF-8",
      ...mergeHeaders(options),
    },
  });
  if (cloudflareFromHeaders(response.status, response.headers)) {
    throwCloudflare(options?.cloudflareResolutionURL ?? originOf(url));
  }
  const text = await response.text();
  assertNotCloudflare(
    url,
    response.status,
    text,
    response.headers,
    options?.cloudflareResolutionURL,
  );
  if (!response.ok) {
    throw new Error(
      `POST ${url} failed (${response.status}): ${text.slice(0, 180)}`,
    );
  }
  return text;
};

/** POST without a body (Madara `/ajax/chapters`). */
export const postEmpty = async (
  url: string,
  options?: FetchOptions,
): Promise<string> => {
  const response = await resolveClient(options).request({
    url,
    method: "POST",
    headers: {
      "X-Requested-With": "XMLHttpRequest",
      ...mergeHeaders(options),
    },
  });
  if (cloudflareFromHeaders(response.status, response.headers)) {
    throwCloudflare(options?.cloudflareResolutionURL ?? originOf(url));
  }
  const text = await response.text();
  assertNotCloudflare(
    url,
    response.status,
    text,
    response.headers,
    options?.cloudflareResolutionURL,
  );
  if (!response.ok) {
    throw new Error(
      `POST ${url} failed (${response.status}): ${text.slice(0, 180)}`,
    );
  }
  return text;
};

export const fetchJson = async <T>(
  url: string,
  options?: FetchOptions,
): Promise<T> => JSON.parse(await fetchText(url, options)) as T;

export const fetchBytes = async (
  url: string,
  options?: FetchOptions,
): Promise<Uint8Array> => {
  try {
    const response = await resolveClient(options).request({
      url,
      method: "GET",
      headers: mergeHeaders(options),
      timeout: options?.timeout,
    });
    if (!response.ok) {
      const body = await response.text();
      assertNotCloudflare(
        url,
        response.status,
        body,
        response.headers,
        options?.cloudflareResolutionURL,
      );
      throw new Error(
        `GET ${url} failed (${response.status}): ${body.slice(0, 180)}`,
      );
    }
    if (response.headers.get("cf-mitigated")) {
      throwCloudflare(options?.cloudflareResolutionURL ?? originOf(url));
    }
    return response.bytes();
  } catch (error) {
    return rethrowCloudflare(error, url, options?.cloudflareResolutionURL);
  }
};

/** GET with optional `Range` header (Hitomi nozomi / index slices). */
export const fetchBytesRange = async (
  url: string,
  range: { start: number; end: number } | undefined,
  options?: FetchOptions,
): Promise<Uint8Array> => {
  const headers = { ...(mergeHeaders(options) ?? {}) };
  if (range) {
    headers.Range = `bytes=${range.start}-${range.end}`;
  }
  const response = await resolveClient(options).request({
    url,
    method: "GET",
    headers,
    timeout: options?.timeout,
  });
  if (!(response.ok || response.status === 206)) {
    const body = await response.text();
    assertNotCloudflare(
      url,
      response.status,
      body,
      response.headers,
      options?.cloudflareResolutionURL,
    );
    throw new Error(
      `GET ${url} failed (${response.status}): ${body.slice(0, 180)}`,
    );
  }
  return response.bytes();
};

/** Aliases matching common port naming. */
export const getText = fetchText;
export const getJson = fetchJson;

export const absoluteUrl = (base: string, maybeRelative: string): string => {
  const value = maybeRelative.trim();
  if (!value) return value;
  if (/^https?:\/\//i.test(value) || value.startsWith("data:")) return value;
  if (value.startsWith("//")) {
    const protocol = base.startsWith("https") ? "https:" : "http:";
    return `${protocol}${value}`;
  }
  const origin = base.match(/^(https?:\/\/[^/?#]+)/i)?.[1] ?? base;
  if (value.startsWith("/")) return `${origin}${value}`;
  return `${origin}/${value}`;
};

export const joinUrl = (base: string, ...parts: string[]): string => {
  let url = base.replace(/\/+$/, "");
  for (const part of parts) {
    const cleaned = part.replace(/^\/+|\/+$/g, "");
    if (!cleaned) continue;
    url += `/${cleaned}`;
  }
  return url;
};

export const withQuery = (
  url: string,
  params: Record<string, string | undefined | null>,
): string => {
  const pairs = Object.entries(params).filter(
    (entry): entry is [string, string] => entry[1] != null,
  );
  if (!pairs.length) return url;
  const query = pairs
    .map(
      ([key, value]) =>
        `${encodeURIComponent(key)}=${encodeURIComponent(value)}`,
    )
    .join("&");
  return url.includes("?") ? `${url}&${query}` : `${url}?${query}`;
};
