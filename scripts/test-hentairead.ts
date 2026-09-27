import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import {
  CloudflareError,
  installEmulatorGlobals,
} from "@suwatte/toolchain/emulator";
import {
  ChapterSchema,
  ContentSchema,
  ItemSchema,
} from "@suwatte/toolchain/validate";
import {
  hasNextPage,
  listingUrl,
  parseChapterPages,
  parseDetails,
  parseGalleryId,
  parseListing,
  parseMangaId,
  parsePageRange,
  parseUploadedDate,
  readerLanguageOrder,
  readerUrl,
  searchUrl,
} from "../src/sources/hentairead/parse.ts";
import { CF_RESOLVE } from "../src/sources/hentairead/constants.ts";
import HentaiRead from "../src/sources/hentairead/index.ts";
import { looksLikeCloudflare } from "../src/sources/_shared/cloudflare.ts";

installEmulatorGlobals();

const fixture = (name: string): string =>
  readFileSync(
    resolve("src/sources/hentairead/fixtures", name),
    "utf8",
  );

const assert = (condition: unknown, message: string) => {
  if (!condition) throw new Error(message);
};

const listing = fixture("listing.html");
const details = fixture("details.html");
const reader = fixture("reader.html");
const readerDom = fixture("reader-dom.html");
const cf = fixture("cf.html");

const items = parseListing(listing);
assert(items.length === 3, `expected 3 listing items, got ${items.length}`);
for (const item of items) ItemSchema.parse(item);
assert(
  items[0]?.id === "mama-mama-o-mamoru-tame-ni-boku-ga-shita-koto-colorized",
  `bad first id: ${items[0]?.id}`,
);
assert(
  items[0]?.coverImage === "https://hencover.xyz/cover/2025/06/hr_260265.jpg",
  `cover should be original hencover src, got ${items[0]?.coverImage}`,
);
assert(
  !items[0]?.coverImage?.includes("-350x"),
  "must not pick the 350w srcset entry",
);
assert(items[0]?.title.includes("MAMA"), `bad title: ${items[0]?.title}`);
assert(
  items[1]?.id === "kyonyuu-jk-ga-honki-o-dashitara-papa-wa-mou-nigerarenai",
  items[1]?.id,
);
assert(
  items[1]?.coverImage?.includes("hr_260264"),
  items[1]?.coverImage,
);
assert(hasNextPage(listing), "listing should have next page");
console.log("listing OK", items.length, items[0]?.title);

const h3Only = parseListing(
  `<div><img src="https://hencover.xyz/cover/x.jpg"><h3><a href="https://hentairead.com/hentai/h3-only-slug/">H3 Only</a></h3></div>`,
);
assert(h3Only[0]?.id === "h3-only-slug", h3Only[0]?.id);
ItemSchema.parse(h3Only[0]);

assert(
  parseMangaId("https://hentairead.com/hentai/foo-bar/") === "foo-bar",
  "parseMangaId",
);

const scrapeListing = "/tmp/hr-scrape/listing.dec.html";
if (existsSync(scrapeListing)) {
  const scraped = parseListing(readFileSync(scrapeListing, "utf8"));
  assert(scraped.length === 30, `scrape listing expected 30, got ${scraped.length}`);
  for (const item of scraped) {
    ItemSchema.parse(item);
    assert(item.coverImage?.includes("hencover.xyz"), `scrape cover ${item.id}`);
    assert(!!item.title, `missing title ${item.id}`);
  }
  console.log("scrape listing OK", scraped.length, scraped[0]?.coverImage);
}

const content = parseDetails(
  details,
  "mama-mama-o-mamoru-tame-ni-boku-ga-shita-koto-colorized",
);
ContentSchema.parse(content);
assert(content.title.includes("MAMA"), content.title);
assert(content.coverImage.includes("hencover.xyz"), content.coverImage);
assert(
  content.credits?.some((c) => /Misaoka/i.test(c.name)),
  `artist missing: ${JSON.stringify(content.credits)}`,
);
assert(
  (content.genres?.length ?? 0) >= 3,
  `expected tags, got ${content.genres?.length}`,
);
assert(
  content.summary?.includes("Scanlators"),
  `scanlators missing from summary: ${content.summary}`,
);
assert(
  content.summary?.includes("Pages: 202"),
  `pages missing: ${content.summary}`,
);
assert(
  (content.context?.languages as string[] | undefined)?.includes("english"),
  "english language chip",
);
assert(parseGalleryId(details) === "260265", "gallery id");
assert(content.additionalDetails?.Pages === "202", "pages detail");
assert(content.additionalDetails?.Category === "Manga", "category");
const uploaded = parseUploadedDate(details);
assert(uploaded instanceof Date, "uploaded date");
assert(readerLanguageOrder(details)[0] === "english", "lang order");
console.log("details OK", content.title, "tags", content.genres?.length);

const scrapeGallery = "/tmp/hr-scrape/gallery.dec.html";
if (existsSync(scrapeGallery)) {
  const full = parseDetails(
    readFileSync(scrapeGallery, "utf8"),
    "mama-mama-o-mamoru-tame-ni-boku-ga-shita-koto-colorized",
  );
  ContentSchema.parse(full);
  assert(full.title.includes("MAMA"), full.title);
  assert(full.coverImage.includes("hencover.xyz"), full.coverImage);
  assert(full.credits?.some((c) => /Misaoka/i.test(c.name)), "scrape artist");
  assert((full.genres?.length ?? 0) >= 10, `scrape tags ${full.genres?.length}`);
  assert(full.summary?.includes("Pages: 202"), full.summary);
  assert(full.context?.galleryId === "260265", "scrape gallery id");
  console.log("scrape details OK", full.genres?.length, "tags");
}

const pages = parseChapterPages(reader);
assert(pages.length === 3, `expected 3 pages, got ${pages.length}`);
assert(
  pages[0]?.url === "https://henread.xyz/manga/260265/page-001.webp",
  pages[0]?.url,
);
assert(
  pages.every((p) => p.url?.includes("page-00")),
  "should prefer script payload over fallback img",
);
console.log("reader OK", pages.length, pages[0]?.url);

const domPages = parseChapterPages(readerDom);
assert(domPages.length === 2, `dom fallback expected 2, got ${domPages.length}`);
assert(domPages[0]?.url?.includes("page-a.webp"), domPages[0]?.url);
assert(
  !domPages.some((p) => p.url?.includes("hencover")),
  "dom fallback must skip covers",
);
console.log("reader DOM fallback OK", domPages.length);

const noParenReader = reader
  .replace("({", "{")
  .replace("});", "};")
  .replace("'(eyJ", "'eyJ")
  .replace("19)'", "19'");
const pages2 = parseChapterPages(noParenReader);
assert(pages2.length === 3, `unwrapped payload expected 3, got ${pages2.length}`);
assert(
  pages2[2]?.url === "https://henread.xyz/manga/260265/page-003.webp",
  pages2[2]?.url,
);

assert(listingUrl(1, "views") === "https://hentairead.com/hentai/?sortby=views");
assert(
  listingUrl(2, "new") === "https://hentairead.com/hentai/page/2/?sortby=new",
);
assert(
  searchUrl(1, [["s", "foo"]]).startsWith(
    "https://hentairead.com/page/1/?s=foo",
  ),
);
assert(
  readerUrl("slug") === "https://hentairead.com/hentai/slug/english/p/1/",
);
assert(
  readerUrl("slug", "") === "https://hentairead.com/hentai/slug/p/1/",
);
assert(parsePageRange(">20")[0] === 21, `>20 min ${parsePageRange(">20")}`);
assert(parsePageRange(">=20")[0] === 20);
assert(parsePageRange("<20")[1] === 19, `<20 max ${parsePageRange("<20")}`);
assert(parsePageRange("<=20")[1] === 20);

const source = new HentaiRead();
const cfg = source.getConfiguration() as {
  cloudflareResolutionURL?: string;
  useClientForImageRequests?: boolean;
  imageReferer?: string;
};
assert(cfg.cloudflareResolutionURL === CF_RESOLVE, JSON.stringify(cfg));
assert(cfg.useClientForImageRequests === true, "images must use HttpClient");
assert(cfg.imageReferer === "https://hentairead.com/");
assert(HentaiRead.info.version >= 2.1, `version ${HentaiRead.info.version}`);

const src = readFileSync("src/sources/hentairead/index.ts", "utf8");
assert(/^["']use httpclient["']/m.test(src), "must use httpclient directive");
assert(
  !/validateStatus\s*:\s*\(\s*\)\s*=>/.test(src),
  "validateStatus bypasses native CF",
);
assert(!!(source as { client?: unknown }).client, "owned HttpClient");

const main = async () => {
  ChapterSchema.parse({
    id: content.context?.galleryId as string,
    index: 0,
    number: 1,
    title: content.context?.scanlator as string,
    language: "en",
    date: new Date(content.context?.uploaded as string),
    webUrl: readerUrl(
      "mama-mama-o-mamoru-tame-ni-boku-ga-shita-koto-colorized",
    ),
  });

  assert(/assertCloudflareCleared/.test(src), "homepage must probe CF like NovelCrow");
  assert(!/rateLimit\s*:/.test(src), "rateLimit hung listing after Resolve");
  try {
    const home = await source.getHomePage();
    assert(home.feeds?.length === 2, "feeds");
  } catch (error) {
    assert(error instanceof CloudflareError, `homepage expected CF, got ${error}`);
    console.log("homepage CF probe OK");
  }

  assert(parseListing("<html><body>no cards</body></html>").length === 0, "empty html");

  assert(looksLikeCloudflare(cf), "cf fixture should detect");
  const liveCf = "/tmp/hr-live.html";
  if (existsSync(liveCf)) {
    assert(looksLikeCloudflare(readFileSync(liveCf, "utf8")), "live CF body");
  }

  try {
    throw new CloudflareError(CF_RESOLVE);
  } catch (error) {
    assert(error instanceof CloudflareError, "CloudflareError type");
    assert(
      (error as CloudflareError).resolutionURL === CF_RESOLVE,
      "resolution URL",
    );
  }

  try {
    await source.getItemList({ key: "latest" }, 1);
    console.log("WARNING: live listing succeeded (no CF from this IP?)");
  } catch (error) {
    assert(error instanceof CloudflareError, `expected CF, got ${error}`);
    assert(
      (error as CloudflareError).resolutionURL === CF_RESOLVE,
      `live CF url ${(error as CloudflareError).resolutionURL}`,
    );
    console.log("live listing CF OK", (error as CloudflareError).resolutionURL);
  }

  const stt = "dist/sources/hentairead.stt";
  if (existsSync(stt)) {
    const head = readFileSync(stt).subarray(0, 512).toString("utf8");
    assert(
      head.startsWith('"use httpclient"') || head.includes('"use httpclient"'),
      `stt missing directive in first 512 bytes: ${head.slice(0, 80)}`,
    );
  }

  console.log(
    "config OK",
    cfg.cloudflareResolutionURL,
    "v" + HentaiRead.info.version,
  );
  console.log("hentairead rewrite tests passed");
};

const verifySourceWithCapturedPages = async () => {
  const client = source.client as unknown as { get: (url: string, options?: unknown) => Promise<unknown> };
  const originalGet = client.get;
  const urls: string[] = [];
  client.get = async (url) => {
    urls.push(url);
    const body = url.includes("/p/1/") ? reader : url.includes("/hentai/?sortby=") ? listing : details;
    return { status: 200, ok: true, headers: { get: () => null }, text: async () => body };
  };
  try {
    const latest = await source.getItemList({ key: "latest" }, 1);
    assert(latest.items.length === 3, "latest should parse captured listing");
    assert(urls[0] === listingUrl(1, "new"), `latest route ${urls[0]}`);
    const popular = await source.getItemList({ key: "popular" }, 1);
    assert(popular.items[0]?.title.includes("MAMA"), "popular title missing");
    assert(urls[1] === listingUrl(1, "views"), `popular route ${urls[1]}`);
    const id = latest.items[0]!.id;
    ContentSchema.parse(await source.getContent(id));
    const chapters = await source.getChapters(id);
    ChapterSchema.parse(chapters[0]);
    const chapterPages = await source.getChapterPages(id, chapters[0]!.id);
    assert(chapterPages.length === 3, "reader should parse captured pages");
    console.log("HentaiRead source listing, title, and reader OK");
  } finally {
    client.get = originalGet;
  }
};

verifySourceWithCapturedPages().then(async () => {
  if (process.env.RUN_LIVE_HENTAIREAD === "1") await main();
  else console.log("HentaiRead captured-page checks passed");
}).catch((error) => {
    console.error(error);
    process.exit(1);
});
