/** Throw Suwatte's CloudflareError so the app auto-opens the challenge WebView. */

export const throwCloudflare = (resolutionURL: string): never => {
  throw new CloudflareError(resolutionURL);
};

export const isCloudflareError = (error: unknown): boolean => {
  const name = String((error as { name?: string })?.name ?? "");
  const message = String((error as { message?: string })?.message ?? error);
  return (
    name.includes("Cloudflare") ||
    message.includes("Cloudflare") ||
    message.includes("cloudflare")
  );
};

export const looksLikeCloudflare = (body: string): boolean =>
  /just a moment|cf-mitigated|challenge-platform|cdn-cgi\/challenge|cf-chl|cf-turnstile|challenges\.cloudflare\.com|attention required|verify you are human|checking your browser|enable javascript and cookies/i.test(
    body,
  );

export const originOf = (url: string): string => {
  const match = /^(https?:\/\/[^/?#]+)/i.exec(url);
  return match?.[1] ? `${match[1]}/` : url;
};

const header = (
  headers:
    | { get(name: string): string | null }
    | Record<string, unknown>
    | undefined
    | null,
  name: string,
): string => {
  if (!headers) return "";
  if (typeof (headers as { get?: unknown }).get === "function") {
    return String(
      (headers as { get(name: string): string | null }).get(name) ?? "",
    ).toLowerCase();
  }
  const obj = headers as Record<string, unknown>;
  const want = name.toLowerCase();
  for (const [key, value] of Object.entries(obj)) {
    if (key.toLowerCase() === want) return String(value ?? "").toLowerCase();
  }
  return "";
};

/** Prefer header signals so we can throw before waiting on a huge body. */
export const cloudflareFromHeaders = (
  status: number,
  headers:
    | { get(name: string): string | null }
    | Record<string, unknown>
    | undefined
    | null,
): boolean => {
  if (header(headers, "cf-mitigated").includes("challenge")) return true;
  if (
    header(headers, "server").includes("cloudflare") &&
    [403, 503, 429].includes(status)
  ) {
    return true;
  }
  const location = header(headers, "location");
  if (location.includes("cdn-cgi/challenge")) return true;
  return false;
};

/**
 * Hit one or more URLs so Suwatte surfaces the CF modal.
 * Always throw with `resolutionURL` (prefer a nested content path — root `/`
 * often renders a blank Turnstile shell in WKWebView).
 */
export const assertCloudflareCleared = async (
  client: InstanceType<typeof HttpClient>,
  resolutionURL: string,
  probeURLs: string[] = [resolutionURL],
): Promise<void> => {
  const urls = probeURLs.length ? probeURLs : [resolutionURL];
  for (const url of urls) {
    try {
      const response = await client.request({
        url,
        method: "GET",
        timeout: 20_000,
      });
      if (cloudflareFromHeaders(response.status, response.headers)) {
        throwCloudflare(resolutionURL);
      }
      const body = await response.text();
      if (
        looksLikeCloudflare(body) ||
        cloudflareFromHeaders(response.status, response.headers)
      ) {
        throwCloudflare(resolutionURL);
      }
      if (
        !response.ok &&
        header(response.headers, "server").includes("cloudflare")
      ) {
        throwCloudflare(resolutionURL);
      }
      // Non-CF success on any probe → cookies are good.
      if (response.ok) return;
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
      // Try next probe URL.
    }
  }
  // Every probe failed without a clear CF signal — still force Resolve on the
  // nested URL so the user isn't stuck on a black/empty source screen.
  throwCloudflare(resolutionURL);
};
