import { signRequest } from "./aws4";
import type { R2Config } from "./config";
import { utf8Decode } from "./crypto";

export type R2Object = {
  key: string;
  size: number;
  lastModified?: string;
};

export type ListResult = {
  prefixes: string[];
  objects: R2Object[];
};

let client: InstanceType<typeof HttpClient> | undefined;

const http = (): InstanceType<typeof HttpClient> => {
  if (!client) {
    client = new HttpClient({
      timeout: 60_000,
      validateStatus: () => true,
    });
  }
  return client;
};

const decodeXml = (value: string): string =>
  value
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&");

const xmlValues = (xml: string, tag: string): string[] => {
  const values: string[] = [];
  const pattern = new RegExp(`<${tag}(?:\\s[^>]*)?>([^<]*)</${tag}>`, "g");
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(xml))) {
    values.push(decodeXml(match[1] ?? ""));
  }
  return values;
};

const firstXml = (xml: string, tag: string): string | undefined =>
  xmlValues(xml, tag)[0];

const assertListXml = (xml: string, status: number): void => {
  if (/<Error[\s>]/i.test(xml)) {
    const code = firstXml(xml, "Code") ?? "Unknown";
    const message = firstXml(xml, "Message") ?? xml.slice(0, 240);
    throw new Error(`R2 list error (${status}) ${code}: ${message}`);
  }

  if (!/<ListBucketResult[\s>]/i.test(xml)) {
    throw new Error(
      `Unexpected R2 list response (${status}): ${xml.slice(0, 240)}`,
    );
  }
};

const parseListXml = (
  xml: string,
): ListResult & { continuationToken?: string } => {
  const commonBlocks =
    xml.match(/<CommonPrefixes>[\s\S]*?<\/CommonPrefixes>/gi) ?? [];
  const prefixes = commonBlocks
    .map((block) => firstXml(block, "Prefix"))
    .filter((value): value is string => !!value);

  const contentBlocks = xml.match(/<Contents>[\s\S]*?<\/Contents>/gi) ?? [];
  const objects: R2Object[] = [];
  for (const block of contentBlocks) {
    const key = firstXml(block, "Key");
    if (!key || key.endsWith("/")) continue;
    objects.push({
      key,
      size: Number(firstXml(block, "Size") ?? "0"),
      lastModified: firstXml(block, "LastModified"),
    });
  }

  return {
    prefixes,
    objects,
    continuationToken:
      firstXml(xml, "IsTruncated")?.toLowerCase() === "true"
        ? firstXml(xml, "NextContinuationToken")
        : undefined,
  };
};

const signedGet = (
  config: R2Config,
  options: { key?: string; query?: Record<string, string> },
) =>
  signRequest({
    method: "GET",
    endpoint: config.endpoint,
    bucket: config.bucket,
    key: options.key,
    accessKeyId: config.accessKeyId,
    secretAccessKey: config.secretAccessKey,
    query: options.query,
    expiresSeconds: 60 * 15,
  });

const listPage = async (
  config: R2Config,
  prefix: string,
  options: { delimiter?: string; continuationToken?: string } = {},
): Promise<ListResult & { continuationToken?: string }> => {
  const query: Record<string, string> = {
    "list-type": "2",
    "max-keys": "1000",
    prefix,
  };
  if (options.delimiter) query.delimiter = options.delimiter;
  if (options.continuationToken) {
    query["continuation-token"] = options.continuationToken;
  }

  const signed = signedGet(config, { query });
  const response = await http().request({
    url: signed.url,
    method: "GET",
  });

  const xml = await response.text();
  assertListXml(xml, response.status);

  if (!response.ok) {
    throw new Error(
      `R2 list failed (${response.status}): ${xml.slice(0, 240)}`,
    );
  }

  return parseListXml(xml);
};

export const listAll = async (
  config: R2Config,
  prefix: string,
  delimiter?: string,
): Promise<ListResult> => {
  const prefixes: string[] = [];
  const objects: R2Object[] = [];
  let continuationToken: string | undefined;

  do {
    const page = await listPage(config, prefix, { delimiter, continuationToken });
    prefixes.push(...page.prefixes);
    objects.push(...page.objects);
    continuationToken = page.continuationToken;
  } while (continuationToken);

  return { prefixes, objects };
};

export const getObjectBytes = async (
  config: R2Config,
  key: string,
): Promise<Uint8Array> => {
  const signed = signedGet(config, { key });
  const response = await http().request({
    url: signed.url,
    method: "GET",
  });

  if (!response.ok) {
    const body = await response.text();
    if (/<Error[\s>]/i.test(body)) {
      const code = firstXml(body, "Code") ?? "Unknown";
      const message = firstXml(body, "Message") ?? body.slice(0, 240);
      throw new Error(`R2 get error for ${key}: ${code}: ${message}`);
    }
    throw new Error(
      `R2 get failed (${response.status}) for ${key}: ${body.slice(0, 240)}`,
    );
  }

  return response.bytes();
};

export const getObjectText = async (
  config: R2Config,
  key: string,
): Promise<string> => utf8Decode(await getObjectBytes(config, key));

export const presignGet = (
  config: R2Config,
  key: string,
  expiresSeconds = 60 * 60,
): string =>
  signRequest({
    method: "GET",
    endpoint: config.endpoint,
    bucket: config.bucket,
    key,
    accessKeyId: config.accessKeyId,
    secretAccessKey: config.secretAccessKey,
    expiresSeconds,
  }).url;

export const imageUrl = (
  config: R2Config,
  key: string,
  expiresSeconds = 60 * 60,
): string => {
  const base = config.publicBaseUrl.replace(/\/+$/, "");
  if (base) return `${base}/${key.replace(/^\/+/, "")}`;
  return presignGet(config, key, expiresSeconds);
};

export const __parseListXmlForTests = parseListXml;
export const __assertListXmlForTests = assertListXml;
