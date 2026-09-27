/** Hitomi `gg.js` subdomain / path helpers (ported from Mihon Hitomi.kt). */

import { fetchText } from "../_shared/http";

const CDN = "gold-usergeneratedcontent.net";
export const LTN_URL = `https://ltn.${CDN}`;
export const HITOMI_BASE = "https://hitomi.la";

const HITOMI_HEADERS = {
  Referer: `${HITOMI_BASE}/`,
  Origin: HITOMI_BASE,
};

type GgState = {
  fetchedAt: number;
  subdomainOffsetDefault: number;
  subdomainOffsetMap: Map<number, number>;
  commonImageId: string;
};

let ggState: GgState | null = null;

const refreshGg = async (): Promise<GgState> => {
  const now = Date.now();
  if (ggState && now - ggState.fetchedAt < 60_000) return ggState;

  const script = await fetchText(`${LTN_URL}/gg.js?_=${now}`, {
    headers: HITOMI_HEADERS,
  });

  const defaultMatch = /var o = (\d)/.exec(script);
  const caseMatch = /o = (\d); break;/.exec(script);
  const commonMatch = /b: '(.+)'/.exec(script);
  if (!defaultMatch || !caseMatch || !commonMatch) {
    throw new Error("Failed to parse Hitomi gg.js");
  }

  const subdomainOffsetDefault = Number(defaultMatch[1]);
  const caseOffset = Number(caseMatch[1]);
  const subdomainOffsetMap = new Map<number, number>();
  for (const match of script.matchAll(/case (\d+):/g)) {
    subdomainOffsetMap.set(Number(match[1]), caseOffset);
  }

  ggState = {
    fetchedAt: now,
    subdomainOffsetDefault,
    subdomainOffsetMap,
    commonImageId: commonMatch[1]!,
  };
  return ggState;
};

const subdomainOffset = async (imageId: number): Promise<number> => {
  const state = await refreshGg();
  return state.subdomainOffsetMap.get(imageId) ?? state.subdomainOffsetDefault;
};

const commonImageId = async (): Promise<string> => {
  const state = await refreshGg();
  return state.commonImageId;
};

/** `s` from gg.js */
export const imageIdFromHash = (hash: string): number => {
  const match = /(..)(.)$/.exec(hash);
  if (!match) throw new Error(`Invalid Hitomi hash: ${hash}`);
  return Number.parseInt(`${match[2]}${match[1]}`, 16);
};

/** `real_full_path_from_hash` thumb path segment */
export const thumbPathFromHash = (hash: string): string =>
  hash.replace(/^.*(..)(.)$/, "$2/$1");

export const resolveImageUrl = async (
  hash: string,
  options: { thumbnail?: boolean; isGif?: boolean } = {},
): Promise<string> => {
  // Prefer webp — Suwatte/Nuke's default decoder often cannot decode AVIF.
  const type = "webp";
  const imageId = imageIdFromHash(hash);
  const offset = await subdomainOffset(imageId);

  if (options.thumbnail) {
    const subDomain = `${String.fromCharCode("a".charCodeAt(0) + offset)}tn`;
    return `https://${subDomain}.${CDN}/${type}bigtn/${thumbPathFromHash(hash)}/${hash}.${type}`;
  }

  const commonId = await commonImageId();
  const subDomain = `w${offset + 1}`;
  return `https://${subDomain}.${CDN}/${commonId}${imageId}/${hash}.${type}`;
};

export const hitomiHeaders = HITOMI_HEADERS;
