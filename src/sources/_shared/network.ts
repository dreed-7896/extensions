/**
 * NetworkClient helpers — AF + HTTPCookieStorage.shared (same jar Suwatte's
 * CF Resolve WebView writes into via Alamofire).
 */

import {
  cloudflareFromHeaders,
  looksLikeCloudflare,
  originOf,
  throwCloudflare,
} from "./cloudflare";
import { browserHeaders } from "./client";

export type NetFetchOptions = {
  headers?: Record<string, string>;
  referer?: string;
  timeout?: number;
  /** Prefer a nested path — root `/` often blanks the CF WebView. */
  cloudflareResolutionURL?: string;
};

const headerGet = (
  headers: Record<string, unknown> | undefined,
  name: string,
): string | null => {
  if (!headers) return null;
  const want = name.toLowerCase();
  for (const [key, value] of Object.entries(headers)) {
    if (key.toLowerCase() === want) {
      return value == null ? null : String(value);
    }
  }
  return null;
};

const asHeaderBag = (headers: Record<string, unknown> | undefined) => ({
  get: (name: string) => headerGet(headers, name),
});

const rethrowCloudflare = (error: unknown, resolutionURL: string): never => {
  const message = String((error as { message?: string })?.message ?? error);
  const name = String(
    (error as { name?: string })?.name ??
      (error as { constructor?: { name?: string } })?.constructor?.name ??
      "",
  );
  if (
    name.includes("Cloudflare") ||
    message.includes("Cloudflare") ||
    message.includes("cloudflare")
  ) {
    throwCloudflare(resolutionURL);
  }
  throw error;
};

export const createNetworkClient = (): InstanceType<typeof NetworkClient> =>
  new NetworkClient();

/** GET text; always rethrows CloudflareError with the Resolve URL. */
export const netGetText = async (
  client: InstanceType<typeof NetworkClient>,
  url: string,
  options?: NetFetchOptions,
): Promise<string> => {
  const resolution = options?.cloudflareResolutionURL ?? originOf(url);
  try {
    const response = await client.get(url, {
      timeout: options?.timeout ?? 45_000,
      headers: browserHeaders({
        ...(options?.referer ? { Referer: options.referer } : {}),
        ...(options?.headers ?? {}),
      }),
    });
    const headers = asHeaderBag(response.headers as Record<string, unknown>);
    if (cloudflareFromHeaders(response.status, headers)) {
      throwCloudflare(resolution);
    }
    const body = response.data ?? "";
    if (looksLikeCloudflare(body)) {
      throwCloudflare(resolution);
    }
    if (response.status < 200 || response.status >= 300) {
      throw new Error(
        `GET ${url} failed (${response.status}): ${body.slice(0, 180)}`,
      );
    }
    return body;
  } catch (error) {
    return rethrowCloudflare(error, resolution);
  }
};

export const netGetJson = async <T>(
  client: InstanceType<typeof NetworkClient>,
  url: string,
  options?: NetFetchOptions,
): Promise<T> => JSON.parse(await netGetText(client, url, options)) as T;

/**
 * Probe nested content path first, then root. Always throw with the nested
 * Resolve URL so WKWebView loads a real page (not the blank root challenge).
 */
export const assertNetworkCloudflareCleared = async (
  client: InstanceType<typeof NetworkClient>,
  resolutionURL: string,
  probeURLs: string[] = [resolutionURL],
): Promise<void> => {
  const urls = probeURLs.length ? probeURLs : [resolutionURL];
  for (const url of urls) {
    try {
      await netGetText(client, url, {
        cloudflareResolutionURL: resolutionURL,
        timeout: 20_000,
      });
      return;
    } catch (error) {
      const message = String((error as { message?: string })?.message ?? error);
      const name = String((error as { name?: string })?.name ?? "");
      if (
        name.includes("Cloudflare") ||
        message.includes("Cloudflare") ||
        message.includes("cloudflare")
      ) {
        throwCloudflare(resolutionURL);
      }
    }
  }
  throwCloudflare(resolutionURL);
};
