import { bytesToBase64 as encodeBase64 } from "../_shared/base64";

export const bytesToBase64 = encodeBase64;


/** JSC has no TextEncoder — encode UTF-8 manually. */
export const utf8Encode = (value: string): Uint8Array => {
  const bytes: number[] = [];
  for (let index = 0; index < value.length; index += 1) {
    let code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff && index + 1 < value.length) {
      const next = value.charCodeAt(index + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        code = 0x10000 + ((code - 0xd800) << 10) + (next - 0xdc00);
        index += 1;
      }
    }

    if (code <= 0x7f) {
      bytes.push(code);
    } else if (code <= 0x7ff) {
      bytes.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
    } else if (code <= 0xffff) {
      bytes.push(
        0xe0 | (code >> 12),
        0x80 | ((code >> 6) & 0x3f),
        0x80 | (code & 0x3f),
      );
    } else {
      bytes.push(
        0xf0 | (code >> 18),
        0x80 | ((code >> 12) & 0x3f),
        0x80 | ((code >> 6) & 0x3f),
        0x80 | (code & 0x3f),
      );
    }
  }
  return Uint8Array.from(bytes);
};

/** JSC has no TextDecoder — decode UTF-8 manually. */
export const utf8Decode = (bytes: Uint8Array): string => {
  let result = "";
  for (let index = 0; index < bytes.length; ) {
    const byte = bytes[index]!;
    if (byte <= 0x7f) {
      result += String.fromCharCode(byte);
      index += 1;
      continue;
    }

    if ((byte & 0xe0) === 0xc0 && index + 1 < bytes.length) {
      const code = ((byte & 0x1f) << 6) | (bytes[index + 1]! & 0x3f);
      result += String.fromCharCode(code);
      index += 2;
      continue;
    }

    if ((byte & 0xf0) === 0xe0 && index + 2 < bytes.length) {
      const code =
        ((byte & 0x0f) << 12) |
        ((bytes[index + 1]! & 0x3f) << 6) |
        (bytes[index + 2]! & 0x3f);
      result += String.fromCharCode(code);
      index += 3;
      continue;
    }

    if ((byte & 0xf8) === 0xf0 && index + 3 < bytes.length) {
      let code =
        ((byte & 0x07) << 18) |
        ((bytes[index + 1]! & 0x3f) << 12) |
        ((bytes[index + 2]! & 0x3f) << 6) |
        (bytes[index + 3]! & 0x3f);
      code -= 0x10000;
      result += String.fromCharCode(0xd800 + (code >> 10), 0xdc00 + (code & 0x3ff));
      index += 4;
      continue;
    }

    result += "\ufffd";
    index += 1;
  }
  return result;
};

export const toBytes = (value: string | Uint8Array): Uint8Array =>
  typeof value === "string" ? utf8Encode(value) : value;

export const toHex = (bytes: Uint8Array): string =>
  Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");

export const sha256Hex = (value: string | Uint8Array): string =>
  toHex(Crypto.sha256(toBytes(value)));

export const hmacSha256 = (
  key: string | Uint8Array,
  message: string | Uint8Array,
): Uint8Array => {
  const blockSize = 64;
  let keyBytes = toBytes(key);
  if (keyBytes.length > blockSize) {
    keyBytes = Crypto.sha256(keyBytes);
  }

  const keyed = new Uint8Array(blockSize);
  keyed.set(keyBytes);

  const ipad = new Uint8Array(blockSize);
  const opad = new Uint8Array(blockSize);
  for (let index = 0; index < blockSize; index += 1) {
    ipad[index] = keyed[index]! ^ 0x36;
    opad[index] = keyed[index]! ^ 0x5c;
  }

  const messageBytes = toBytes(message);
  const inner = new Uint8Array(blockSize + messageBytes.length);
  inner.set(ipad);
  inner.set(messageBytes, blockSize);

  const outer = new Uint8Array(blockSize + 32);
  outer.set(opad);
  outer.set(Crypto.sha256(inner), blockSize);
  return Crypto.sha256(outer);
};

export const hmacSha256Hex = (
  key: string | Uint8Array,
  message: string | Uint8Array,
): string => toHex(hmacSha256(key, message));
