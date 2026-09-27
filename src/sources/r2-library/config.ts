export const SETTINGS = {
  accountId: "accountId",
  accessKeyId: "accessKeyId",
  secretAccessKey: "secretAccessKey",
  bucket: "bucket",
  endpoint: "endpoint",
  prefix: "prefix",
  publicBaseUrl: "publicBaseUrl",
} as const;

export type R2Config = {
  accountId: string;
  accessKeyId: string;
  secretAccessKey: string;
  bucket: string;
  endpoint: string;
  /** Empty string = list from bucket root. */
  prefix: string;
  /** Optional r2.dev / custom domain for unsigned image URLs. */
  publicBaseUrl: string;
};

const readString = async (key: string): Promise<string> => {
  const value = await ObjectStore.string(key);
  return value?.trim() ?? "";
};

export const loadConfig = async (): Promise<R2Config> => {
  const accountId = await readString(SETTINGS.accountId);
  const accessKeyId = await readString(SETTINGS.accessKeyId);
  const secretAccessKey = await readString(SETTINGS.secretAccessKey);
  const bucket = await readString(SETTINGS.bucket);
  const endpointOverride = await readString(SETTINGS.endpoint);
  // Default empty: titles often live at bucket root when the bucket is already named "manga".
  const prefixRaw = await readString(SETTINGS.prefix);

  if (!accountId || !accessKeyId || !secretAccessKey || !bucket) {
    throw new Error(
      "R2 Library is not configured. Open source settings and set Account ID, Access Key ID, Secret Access Key, and Bucket.",
    );
  }

  return {
    accountId,
    accessKeyId,
    secretAccessKey,
    bucket,
    endpoint:
      endpointOverride.replace(/\/+$/, "") ||
      `https://${accountId}.r2.cloudflarestorage.com`,
    prefix: prefixRaw.replace(/^\/+|\/+$/g, ""),
    publicBaseUrl: (await readString(SETTINGS.publicBaseUrl)).replace(/\/+$/, ""),
  };
};

export const saveConfig = async (
  data: Record<string, unknown>,
): Promise<void> => {
  const write = async (key: string, allowEmpty = false) => {
    if (!(key in data)) return;
    const value = data[key];
    if (typeof value !== "string") return;
    if (!allowEmpty && value.trim() === "") return;
    await ObjectStore.set(key, value.trim());
  };

  await write(SETTINGS.accountId);
  await write(SETTINGS.accessKeyId);
  await write(SETTINGS.secretAccessKey);
  await write(SETTINGS.bucket);
  await write(SETTINGS.endpoint, true);
  await write(SETTINGS.prefix, true);
  await write(SETTINGS.publicBaseUrl, true);
};
