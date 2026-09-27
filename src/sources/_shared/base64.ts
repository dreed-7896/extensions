/** Pure JS base64 helpers — JSC has no reliable atob/btoa/Buffer. */

const alphabet =
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

const decodeMap = (() => {
  const map = new Uint8Array(128);
  map.fill(255);
  for (let i = 0; i < alphabet.length; i += 1) {
    map[alphabet.charCodeAt(i)] = i;
  }
  return map;
})();

export const bytesToBase64 = (bytes: Uint8Array): string => {
  let output = "";
  const length = bytes.length;
  for (let i = 0; i < length; i += 3) {
    const a = bytes[i]!;
    const b = i + 1 < length ? bytes[i + 1]! : 0;
    const c = i + 2 < length ? bytes[i + 2]! : 0;
    const triplet = (a << 16) | (b << 8) | c;
    output += alphabet[(triplet >> 18) & 63];
    output += alphabet[(triplet >> 12) & 63];
    output += i + 1 < length ? alphabet[(triplet >> 6) & 63] : "=";
    output += i + 2 < length ? alphabet[triplet & 63] : "=";
  }
  return output;
};

/** Decode standard base64 to a UTF-8 string (for JSON payloads). */
export const base64Decode = (value: string): string => {
  const cleaned = value.replace(/[^A-Za-z0-9+/=]/g, "");
  if (!cleaned) return "";

  const bytes: number[] = [];
  for (let i = 0; i < cleaned.length; i += 4) {
    const enc1 = decodeMap[cleaned.charCodeAt(i)] ?? 255;
    const enc2 = decodeMap[cleaned.charCodeAt(i + 1)] ?? 255;
    const enc3 =
      cleaned[i + 2] === "="
        ? 64
        : (decodeMap[cleaned.charCodeAt(i + 2)] ?? 255);
    const enc4 =
      cleaned[i + 3] === "="
        ? 64
        : (decodeMap[cleaned.charCodeAt(i + 3)] ?? 255);
    if (enc1 === 255 || enc2 === 255 || enc3 === 255 || enc4 === 255) {
      throw new Error("Invalid base64 input");
    }

    bytes.push((enc1 << 2) | (enc2 >> 4));
    if (enc3 !== 64) bytes.push(((enc2 & 15) << 4) | (enc3 >> 2));
    if (enc4 !== 64) bytes.push(((enc3 & 3) << 6) | enc4);
  }

  // UTF-8 decode without TextDecoder (also missing on some JSC builds).
  let result = "";
  for (let i = 0; i < bytes.length; ) {
    const c = bytes[i]!;
    if (c < 0x80) {
      result += String.fromCharCode(c);
      i += 1;
    } else if (c < 0xe0 && i + 1 < bytes.length) {
      result += String.fromCharCode(((c & 0x1f) << 6) | (bytes[i + 1]! & 0x3f));
      i += 2;
    } else if (c < 0xf0 && i + 2 < bytes.length) {
      result += String.fromCharCode(
        ((c & 0x0f) << 12) |
          ((bytes[i + 1]! & 0x3f) << 6) |
          (bytes[i + 2]! & 0x3f),
      );
      i += 3;
    } else if (i + 3 < bytes.length) {
      const code =
        ((c & 0x07) << 18) |
        ((bytes[i + 1]! & 0x3f) << 12) |
        ((bytes[i + 2]! & 0x3f) << 6) |
        (bytes[i + 3]! & 0x3f);
      const offset = code - 0x10000;
      result += String.fromCharCode(
        0xd800 + (offset >> 10),
        0xdc00 + (offset & 0x3ff),
      );
      i += 4;
    } else {
      break;
    }
  }
  return result;
};
