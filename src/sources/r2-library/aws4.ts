import { hmacSha256, hmacSha256Hex, sha256Hex } from "./crypto";

export type SignedRequest = {
  url: string;
  method: string;
  headers: Record<string, string>;
};

export type SignOptions = {
  method?: string;
  endpoint: string;
  bucket: string;
  key?: string;
  accessKeyId: string;
  secretAccessKey: string;
  query?: Record<string, string>;
  headers?: Record<string, string>;
  bodyHash?: string;
  expiresSeconds?: number;
  region?: string;
  service?: string;
};

const encodeRfc3986 = (value: string): string =>
  encodeURIComponent(value).replace(/[!'()*]/g, (char) =>
    `%${char.charCodeAt(0).toString(16).toUpperCase()}`,
  );

const encodePath = (path: string): string =>
  path
    .split("/")
    .map((segment) => encodeRfc3986(segment))
    .join("/");

const amzDate = (date: Date): { amz: string; stamp: string } => {
  const iso = date.toISOString().replace(/[:-]|\.\d{3}/g, "");
  return { amz: iso, stamp: iso.slice(0, 8) };
};

const canonicalQuery = (params: Record<string, string>): string =>
  Object.keys(params)
    .sort()
    .map((key) => `${encodeRfc3986(key)}=${encodeRfc3986(params[key] ?? "")}`)
    .join("&");

const signingKey = (
  secretAccessKey: string,
  stamp: string,
  region: string,
  service: string,
): Uint8Array => {
  const kDate = hmacSha256(`AWS4${secretAccessKey}`, stamp);
  const kRegion = hmacSha256(kDate, region);
  const kService = hmacSha256(kRegion, service);
  return hmacSha256(kService, "aws4_request");
};

const parseHttpUrl = (value: string): { origin: string; host: string } => {
  const match = value
    .trim()
    .replace(/\/+$/, "")
    .match(/^(https?):\/\/([^/?#]+)/i);
  if (!match) {
    throw new Error(`Invalid endpoint URL: ${value}`);
  }
  return {
    origin: `${match[1]!.toLowerCase()}://${match[2]!}`,
    host: match[2]!,
  };
};

export const signRequest = (options: SignOptions): SignedRequest => {
  const method = (options.method ?? "GET").toUpperCase();
  const region = options.region ?? "auto";
  const service = options.service ?? "s3";
  const { amz, stamp } = amzDate(new Date());
  const payloadHash = options.bodyHash ?? "UNSIGNED-PAYLOAD";

  const { origin, host } = parseHttpUrl(options.endpoint);
  const keyPath = options.key ? `/${options.key.replace(/^\/+/, "")}` : "";
  const canonicalUri = encodePath(`/${options.bucket}${keyPath}`);

  const query: Record<string, string> = { ...(options.query ?? {}) };
  const headers: Record<string, string> = {
    host,
    ...(options.headers ?? {}),
  };

  if (options.expiresSeconds != null) {
    query["X-Amz-Algorithm"] = "AWS4-HMAC-SHA256";
    query["X-Amz-Credential"] =
      `${options.accessKeyId}/${stamp}/${region}/${service}/aws4_request`;
    query["X-Amz-Date"] = amz;
    query["X-Amz-Expires"] = String(options.expiresSeconds);
    query["X-Amz-SignedHeaders"] = "host";
  } else {
    headers["x-amz-content-sha256"] = payloadHash;
    headers["x-amz-date"] = amz;
  }

  const signedHeaderNames = Object.keys(headers)
    .map((name) => name.toLowerCase())
    .sort();
  const canonicalHeaders = signedHeaderNames
    .map((name) => {
      const matched = Object.entries(headers).find(
        ([headerName]) => headerName.toLowerCase() === name,
      );
      return `${name}:${(matched?.[1] ?? "").trim()}\n`;
    })
    .join("");
  const signedHeaders = signedHeaderNames.join(";");
  const queryString = canonicalQuery(query);

  const canonicalRequest = [
    method,
    canonicalUri,
    queryString,
    canonicalHeaders,
    signedHeaders,
    payloadHash,
  ].join("\n");

  const stringToSign = [
    "AWS4-HMAC-SHA256",
    amz,
    `${stamp}/${region}/${service}/aws4_request`,
    sha256Hex(canonicalRequest),
  ].join("\n");

  const signature = hmacSha256Hex(
    signingKey(options.secretAccessKey, stamp, region, service),
    stringToSign,
  );

  if (options.expiresSeconds != null) {
    query["X-Amz-Signature"] = signature;
    return {
      url: `${origin}${canonicalUri}?${canonicalQuery(query)}`,
      method,
      headers: { host },
    };
  }

  headers.Authorization =
    `AWS4-HMAC-SHA256 Credential=${options.accessKeyId}/${stamp}/${region}/${service}/aws4_request, ` +
    `SignedHeaders=${signedHeaders}, Signature=${signature}`;

  return {
    url: `${origin}${canonicalUri}${queryString ? `?${queryString}` : ""}`,
    method,
    headers,
  };
};
