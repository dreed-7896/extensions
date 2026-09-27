import assert from "node:assert/strict";
import { zipSync } from "fflate";
import { installEmulatorGlobals } from "@suwatte/toolchain/emulator";
import {
  ContentRating,
  ContentStatus,
  ContentType,
} from "@suwatte/toolchain/types";
import {
  extractArchivePages,
  naturalCompare,
  parseChapterNumber,
} from "../src/sources/r2-library/archive.ts";
import { signRequest } from "../src/sources/r2-library/aws4.ts";
import { hmacSha256Hex, sha256Hex } from "../src/sources/r2-library/crypto.ts";
import {
  contentFromDetails,
  parseDetailsJson,
} from "../src/sources/r2-library/details.ts";

installEmulatorGlobals();

assert.equal(
  sha256Hex(""),
  "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
);
assert.equal(
  hmacSha256Hex("key", "The quick brown fox jumps over the lazy dog"),
  "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8",
);

const signed = signRequest({
  method: "GET",
  endpoint: "https://abc123.r2.cloudflarestorage.com",
  bucket: "library",
  key: "manga/demo/cover.jpg",
  accessKeyId: "AKIA_TEST",
  secretAccessKey: "secret_test",
  expiresSeconds: 3600,
});
assert.match(signed.url, /X-Amz-Algorithm=AWS4-HMAC-SHA256/);
assert.match(signed.url, /X-Amz-Signature=/);
assert.match(signed.url, /\/manga\/demo\/cover\.jpg/);

const headerSigned = signRequest({
  method: "GET",
  endpoint: "https://abc123.r2.cloudflarestorage.com",
  bucket: "library",
  accessKeyId: "AKIA_TEST",
  secretAccessKey: "secret_test",
  query: { "list-type": "2", prefix: "manga/" },
});
assert.ok(headerSigned.headers.Authorization?.startsWith("AWS4-HMAC-SHA256"));
assert.equal(headerSigned.headers["x-amz-content-sha256"], "UNSIGNED-PAYLOAD");

assert.equal(parseChapterNumber("012 - Night Ferry.cbz", 99), 12);
assert.ok(naturalCompare("2.cbz", "10.cbz") < 0);

const archive = zipSync({
  "page-02.png": new Uint8Array([137, 80, 78, 71]),
  "page-01.png": new Uint8Array([137, 80, 78, 71]),
  "__MACOSX/._page-01.png": new Uint8Array([1, 2, 3]),
});
const pages = extractArchivePages(archive);
assert.equal(pages.length, 2);
assert.equal(pages[0]?.name, "page-01.png");
assert.ok(pages[0]?.b64.length);

const details = parseDetailsJson(
  JSON.stringify({
    title: "Harbor Lights",
    rating: "suggestive",
    status: "ongoing",
    contentType: "manhwa",
    statistics: { favorites: 10, views: 20 },
  }),
);
const content = contentFromDetails(details, {
  id: "harbor-lights",
  coverImage: "https://example.org/cover.jpg",
});
assert.equal(content.title, "Harbor Lights");
assert.equal(content.rating, ContentRating.SUGGESTIVE);
assert.equal(content.status, ContentStatus.ONGOING);
assert.equal(content.contentType, ContentType.MANHWA);
assert.equal(content.statistics?.favorites, 10);

console.log("r2 unit tests ok");
