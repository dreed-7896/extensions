/** Browser-ish defaults so CF / WAFs don't treat us as a naked bot. */

/** Shared Accept header used by Mihon HttpSource-style clients. */
export const BROWSER_ACCEPT =
  "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8";

export const BROWSER_ACCEPT_LANGUAGE = "en-US,en;q=0.9";

/**
 * Headers that match what Suwatte's CF WebView / Mihon typically send.
 * Intentionally omits User-Agent — Suwatte injects Preferences.userAgent so
 * it stays in sync with the challenge WebView (cookie binding).
 */
export const browserHeaders = (
  extras: Record<string, string> = {},
): Record<string, string> => ({
  Accept: BROWSER_ACCEPT,
  "Accept-Language": BROWSER_ACCEPT_LANGUAGE,
  ...extras,
});

/**
 * HttpClient owned by a source class (`this.client`).
 *
 * Per Suwatte docs: set cloudflareResolutionURL here (and in getConfiguration).
 * When a challenge appears, the app opens Resolve and attaches cookies to this
 * client. Do NOT set validateStatus: () => true — that skips native CF throws.
 * Requires `"use httpclient"` and useClientForImageRequests for image cookies.
 */
export const createProtectedClient = (
  resolutionURL: string,
  extras: Record<string, string> = {},
): InstanceType<typeof HttpClient> =>
  new HttpClient({
    timeout: 45_000,
    cloudflareResolutionURL: resolutionURL,
    headers: browserHeaders({
      Referer: resolutionURL.endsWith("/")
        ? resolutionURL
        : `${resolutionURL}/`,
      ...extras,
    }),
  });
