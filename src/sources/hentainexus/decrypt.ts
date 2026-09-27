/** Reader payload decryption ported from Keiyoushi / Paperback HentaiNexus. */

const PRIME_NUMBERS = [2, 3, 5, 7, 11, 13, 17, 19];
const PRIME_IDX_XOR_MASK = 12;

const base64ToBytes = (b64: string): number[] => {
  const chars =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const clean = b64.replace(/[^A-Za-z0-9+/=]/g, "");
  const bytes: number[] = [];
  let buffer = 0;
  let bits = 0;
  for (let i = 0; i < clean.length; i++) {
    const c = clean[i]!;
    if (c === "=") break;
    const idx = chars.indexOf(c);
    if (idx === -1) continue;
    buffer = (buffer << 6) | idx;
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      bytes.push((buffer >>> bits) & 0xff);
    }
  }
  return bytes;
};

export const decryptReaderPayload = (encoded: string): string => {
  const data = base64ToBytes(encoded);

  const hostname = "hentainexus.com";
  for (let i = 0; i < hostname.length; i++) {
    data[i] = data[i]! ^ hostname.charCodeAt(i);
  }

  const keyStream: number[] = [];
  for (let i = 0; i < 64; i++) keyStream.push(data[i]! & 0xff);
  const ciphertext: number[] = [];
  for (let i = 64; i < data.length; i++) ciphertext.push(data[i]! & 0xff);

  const digest: number[] = [];
  for (let i = 0; i <= 255; i++) digest.push(i);

  let primeIdx = 0;
  for (let i = 0; i < 64; i++) {
    primeIdx = primeIdx ^ keyStream[i]!;
    for (let j = 0; j < 8; j++) {
      if ((primeIdx & 1) !== 0) {
        primeIdx = (primeIdx >>> 1) ^ PRIME_IDX_XOR_MASK;
      } else {
        primeIdx = primeIdx >>> 1;
      }
    }
  }
  primeIdx = primeIdx & 7;

  let temp: number;
  let key = 0;
  for (let i = 0; i <= 255; i++) {
    key = (key + digest[i]! + keyStream[i % 64]!) % 256;
    temp = digest[i]!;
    digest[i] = digest[key]!;
    digest[key] = temp;
  }

  const q = PRIME_NUMBERS[primeIdx]!;
  let k = 0;
  let n = 0;
  let p = 0;
  let xorKey = 0;
  let result = "";
  for (let i = 0; i < ciphertext.length; i++) {
    k = (k + q) % 256;
    n = (p + digest[(n + digest[k]!) % 256]!) % 256;
    p = (p + k + digest[k]!) % 256;

    temp = digest[k]!;
    digest[k] = digest[n]!;
    digest[n] = temp;

    xorKey =
      digest[(n + digest[(k + digest[(xorKey + p) % 256]!) % 256]!) % 256]!;
    result += String.fromCharCode(ciphertext[i]! ^ xorKey);
  }
  return result;
};
