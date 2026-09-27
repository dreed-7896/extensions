/** Hitomi nozomi / galleriesindex binary helpers. */

import { fetchBytesRange, fetchText } from "../_shared/http";
import { hitomiHeaders, LTN_URL } from "./gg";

const readU32 = (bytes: Uint8Array, offset: number): number =>
  ((bytes[offset]! << 24) |
    (bytes[offset + 1]! << 16) |
    (bytes[offset + 2]! << 8) |
    bytes[offset + 3]!) >>>
  0;

const readI32 = (bytes: Uint8Array, offset: number): number =>
  (readU32(bytes, offset) << 0) >> 0;

const readU64 = (bytes: Uint8Array, offset: number): number => {
  // Offsets fit comfortably in JS number (index files are not multi-GB).
  const high = readU32(bytes, offset);
  const low = readU32(bytes, offset + 4);
  return high * 0x1_0000_0000 + low;
};

export const parseNozomiIds = (bytes: Uint8Array): number[] => {
  const ids: number[] = [];
  for (let offset = 0; offset + 4 <= bytes.length; offset += 4) {
    ids.push(readI32(bytes, offset));
  }
  return ids;
};

export const pageByteRange = (
  page: number,
  pageSize = 25,
): { start: number; end: number } => {
  const start = (page - 1) * pageSize * 4;
  return { start, end: start + pageSize * 4 - 1 };
};

export const getGalleryIdsFromNozomi = async (
  area: string | null,
  tag: string,
  language: string,
  range?: { start: number; end: number },
): Promise<number[]> => {
  const url =
    area == null
      ? `${LTN_URL}/${tag}-${language}.nozomi`
      : `${LTN_URL}/${area}/${tag}-${language}.nozomi`;
  const bytes = await fetchBytesRange(url, range, { headers: hitomiHeaders });
  return parseNozomiIds(bytes);
};

let galleriesIndexVersion: string | null = null;

export const getGalleriesIndexVersion = async (): Promise<string> => {
  if (galleriesIndexVersion) return galleriesIndexVersion;
  galleriesIndexVersion = (
    await fetchText(`${LTN_URL}/galleriesindex/version?_=${Date.now()}`, {
      headers: hitomiHeaders,
    })
  ).trim();
  return galleriesIndexVersion;
};

type Node = {
  keys: Uint8Array[];
  datas: { offset: number; length: number }[];
  subNodeAddresses: number[];
};

const decodeNode = (data: Uint8Array): Node => {
  let pos = 0;
  const numberOfKeys = readI32(data, pos);
  pos += 4;
  const keys: Uint8Array[] = [];
  for (let i = 0; i < numberOfKeys; i += 1) {
    const keySize = readI32(data, pos);
    pos += 4;
    if (keySize <= 0 || keySize > 32) {
      throw new Error("fatal: !keySize || keySize > 32");
    }
    keys.push(data.slice(pos, pos + keySize));
    pos += keySize;
  }

  const numberOfDatas = readI32(data, pos);
  pos += 4;
  const datas: { offset: number; length: number }[] = [];
  for (let i = 0; i < numberOfDatas; i += 1) {
    const offset = readU64(data, pos);
    pos += 8;
    const length = readI32(data, pos);
    pos += 4;
    datas.push({ offset, length });
  }

  const subNodeAddresses: number[] = [];
  for (let i = 0; i < 17; i += 1) {
    subNodeAddresses.push(readU64(data, pos));
    pos += 8;
  }

  return { keys, datas, subNodeAddresses };
};

const getGalleryNodeAtAddress = async (address: number): Promise<Node> => {
  const version = await getGalleriesIndexVersion();
  const url = `${LTN_URL}/galleriesindex/galleries.${version}.index`;
  const nodedata = await fetchBytesRange(
    url,
    { start: address, end: address + 463 },
    { headers: hitomiHeaders },
  );
  return decodeNode(nodedata);
};

const compareBuffers = (left: Uint8Array, right: Uint8Array): number => {
  const top = Math.min(left.length, right.length);
  for (let i = 0; i < top; i += 1) {
    if (left[i]! < right[i]!) return -1;
    if (left[i]! > right[i]!) return 1;
  }
  return 0;
};

const bSearch = async (
  key: Uint8Array,
  node: Node,
): Promise<{ offset: number; length: number } | null> => {
  if (!node.keys.length) return null;

  let where = node.keys.length;
  let found = false;
  for (let i = 0; i < node.keys.length; i += 1) {
    const cmp = compareBuffers(key, node.keys[i]!);
    if (cmp <= 0) {
      found = cmp === 0;
      where = i;
      break;
    }
  }

  if (found) return node.datas[where] ?? null;

  const isLeaf = node.subNodeAddresses.every((address) => address === 0);
  if (isLeaf) return null;

  const next = await getGalleryNodeAtAddress(node.subNodeAddresses[where] ?? 0);
  return bSearch(key, next);
};

const getGalleryIdsFromData = async (data: {
  offset: number;
  length: number;
}): Promise<number[]> => {
  const version = await getGalleriesIndexVersion();
  const url = `${LTN_URL}/galleriesindex/galleries.${version}.data`;
  const { offset, length } = data;
  if (length < 1 || length > 100_000_000) {
    throw new Error(`Length ${length} is too long`);
  }
  const inbuf = await fetchBytesRange(
    url,
    { start: offset, end: offset + length - 1 },
    { headers: hitomiHeaders },
  );
  const numberOfGalleryIDs = readI32(inbuf, 0);
  if (numberOfGalleryIDs < 1 || numberOfGalleryIDs > 10_000_000) {
    throw new Error(`number_of_galleryids ${numberOfGalleryIDs} is too long`);
  }
  const expected = numberOfGalleryIDs * 4 + 4;
  if (inbuf.length !== expected) {
    throw new Error(
      `inbuf.byteLength ${inbuf.length} != expected_length ${expected}`,
    );
  }
  const ids: number[] = [];
  for (let i = 0; i < numberOfGalleryIDs; i += 1) {
    ids.push(readI32(inbuf, 4 + i * 4));
  }
  return ids;
};

const hashTerm = (term: string): Uint8Array => {
  const digest = Crypto.sha256(
    Uint8Array.from(Array.from(term, (ch) => ch.charCodeAt(0))),
  );
  return digest.slice(0, 4);
};

export const getGalleryIdsForQuery = async (
  query: string,
  language = "all",
): Promise<number[]> => {
  const normalized = query.replace(/_/g, " ");
  if (normalized.includes(":")) {
    const [ns, rawTag] = normalized.split(":");
    let area: string | null = ns ?? null;
    let tag = rawTag ?? "";
    let lang = language;

    if (ns === "female" || ns === "male") {
      area = "tag";
      tag = normalized;
    } else if (ns === "language") {
      area = null;
      lang = tag;
      tag = "index";
    }

    return getGalleryIdsFromNozomi(area, tag, lang);
  }

  const key = hashTerm(normalized);
  const root = await getGalleryNodeAtAddress(0);
  const data = await bSearch(key, root);
  if (!data) return [];
  return getGalleryIdsFromData(data);
};
