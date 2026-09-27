import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { installEmulatorGlobals } from "@suwatte/toolchain/emulator";
import {
  ChapterSchema,
  ContentSchema,
  ItemSchema,
} from "@suwatte/toolchain/validate";
import {
  AUTH_HEADER,
  BASE,
  CF_RESOLVE,
  DEFAULT_AUTH,
  PAGE_LIMIT,
} from "../src/sources/hiperdex/constants.ts";
import {
  chapterFromDto,
  chapterIdOf,
  chaptersInput,
  cleanTitle,
  contentFromManga,
  detailsInput,
  isApiKeyError,
  itemFromManga,
  pagesFromDto,
  pagesInput,
  parseAuthFromBundle,
  parseChapterId,
  parseSlug,
  scriptSrcFromHome,
  searchInput,
  sessionCookieFromHeaders,
  trpcUrl,
  unwrapBatch,
  type ChapterDto,
  type MangaDto,
  type PageDto,
} from "../src/sources/hiperdex/parse.ts";
import Hiperdex from "../src/sources/hiperdex/index.ts";

installEmulatorGlobals();

const fixture = <T>(name: string): T =>
  JSON.parse(
    readFileSync(resolve("src/sources/hiperdex/fixtures", name), "utf8"),
  ) as T;

const assert = (condition: unknown, message: string) => {
  if (!condition) throw new Error(message);
};

const search = fixture<{ hits: MangaDto[] }>("search.json");
const details = fixture<MangaDto>("details.json");
const chapters = fixture<ChapterDto[]>("chapters.json");
const pages = fixture<PageDto[]>("pages.json");

assert(parseSlug("/manga/excuse-me-this-is-my-room-16524ba3/") === "excuse-me-this-is-my-room-16524ba3", "slug");
assert(parseChapterId("120:322960").id === "322960", "chapter id");
assert(parseChapterId("120:322960").number === 120, "chapter number");
assert(chapterIdOf(120, 322960) === "120:322960");

const items = search.hits.map((hit) => itemFromManga(hit, hit.title ?? ""));
assert(items.length === 2, `hits ${items.length}`);
for (const item of items) ItemSchema.parse(item);
assert(items[0]?.id === "excuse-me-this-is-my-room-16524ba3");
assert(items[0]?.coverImage?.includes("r2d2storage"), items[0]?.coverImage);
console.log("listing parse OK", items[0]?.title);

const content = contentFromManga(details, details.title ?? "");
ContentSchema.parse(content);
assert(content.credits?.some((c) => c.name === "LObeam"), JSON.stringify(content.credits));
assert(content.additionalTitles?.includes("The Ark Is Me"), "alt titles json string");
assert(content.context?.mangaId === 8810, "mangaId");
assert((content.genres?.length ?? 0) >= 3, "genres");
console.log("details parse OK", content.title, content.additionalTitles?.[0]);

const parsedChapters = chapters
  .map((dto) => chapterFromDto(dto, details.slug ?? ""))
  .filter(Boolean);
assert(parsedChapters.length === 3);
for (const ch of parsedChapters) ChapterSchema.parse(ch);
assert(parsedChapters[0]?.title === "Chapter 120 [END]", parsedChapters[0]?.title);
assert(parsedChapters[1]?.title === "Chapter 119", parsedChapters[1]?.title);
assert(parsedChapters[0]?.webUrl?.endsWith("/120"), parsedChapters[0]?.webUrl);
console.log("chapters parse OK", parsedChapters[0]?.id);

const mappedPages = pagesFromDto(pages);
assert(mappedPages.length === 3);
assert(mappedPages[0]?.url?.includes("1.jpg"), mappedPages[0]?.url);
console.log("pages parse OK", mappedPages[0]?.url);

assert(
  cleanTitle("(Official) Foo", { stripVersion: true, customRegex: "" }) === "Foo",
  `clean ${cleanTitle("(Official) Foo", { stripVersion: true, customRegex: "" })}`,
);

const fakeHeaders = { get: (name: string) => (name.toLowerCase() === "set-cookie" ? "__st=abc; Path=/" : null) };
assert(sessionCookieFromHeaders(fakeHeaders) === "__st=abc", "cookie parse");
assert(isApiKeyError(401, "{}"));
assert(isApiKeyError(200, '{"error":"Unauthorized: Invalid or missing API Key"}'));

const bundle = `const rC=[120,45,99,102,103,45,97,117,116,104].map(n=>String.fromCharCode(n)).join(""),aC=atob("eWNlcXQ3cWd1MA==")+"04"`;
assert(parseAuthFromBundle(bundle) === DEFAULT_AUTH, parseAuthFromBundle(bundle));
assert(scriptSrcFromHome('<script src="/assets/index-DlNdCiUq.js"></script>')?.includes("index-"));

const src = readFileSync("src/sources/hiperdex/index.ts", "utf8");
assert(/^["']use httpclient["']/m.test(src), "httpclient directive");
assert(!/validateStatus\s*:\s*\(\s*\)\s*=>/.test(src), "no status swallow");

const source = new Hiperdex();
const cfg = source.getConfiguration() as {
  cloudflareResolutionURL?: string;
  useClientForImageRequests?: boolean;
};
assert(cfg.cloudflareResolutionURL === CF_RESOLVE);
assert(cfg.useClientForImageRequests === true);
assert(!!(source as { client?: unknown }).client);
assert(Hiperdex.info.version >= 1);

const searchUrl = trpcUrl(
  "search.query",
  searchInput({
    query: "",
    sort: "popular",
    genres: [],
    maxRating: "pornographic",
    page: 1,
    limit: PAGE_LIMIT,
  }),
);
assert(searchUrl.includes("/api/trpc/search.query"), searchUrl);
assert(trpcUrl("auth.me,series.bySlugWithGenres", detailsInput("foo")).includes("series.bySlugWithGenres"));
assert(trpcUrl("auth.me,series.chapters", chaptersInput(8810)).includes("series.chapters"));
assert(
  trpcUrl("auth.me,series.bySlug,reader.chapterPages", pagesInput("foo", 1, "9")).includes(
    "reader.chapterPages",
  ),
);

const main = async () => {
  const home = await source.getHomePage();
  assert(home.feeds?.length === 2, "feeds");

  const popular = await source.getItemList({ key: "popular" }, 1);
  assert(popular.items.length >= 1, `live popular ${popular.items.length}`);
  for (const item of popular.items) ItemSchema.parse(item);
  assert(popular.items[0]?.coverImage, "live cover");
  console.log("live popular OK", popular.items.length, popular.items[0]?.title);

  const latest = await source.getItemList({ key: "latest" }, 1);
  assert(latest.items.length >= 1, "live latest");
  console.log("live latest OK", latest.items.length);

  const q = await source.getSearchResults({ query: "room" }, 1);
  assert(q.items.length >= 1, "live search");
  console.log("live search OK", q.items.length, q.items[0]?.title);

  const id = popular.items[0]!.id;
  const liveContent = await source.getContent(id);
  ContentSchema.parse(liveContent);
  assert(liveContent.coverImage, "live details cover");
  console.log("live details OK", liveContent.title);

  const liveChapters = await source.getChapters(id);
  assert(liveChapters.length >= 1, `live chapters ${liveChapters.length}`);
  ChapterSchema.parse(liveChapters[0]);
  console.log("live chapters OK", liveChapters.length, liveChapters[0]?.title);

  const livePages = await source.getChapterPages(id, liveChapters[0]!.id);
  assert(livePages.length >= 1, `live pages ${livePages.length}`);
  assert(livePages[0]?.url?.startsWith("http"), livePages[0]?.url);
  console.log("live pages OK", livePages.length, livePages[0]?.url);

  const stt = "dist/sources/hiperdex.stt";
  try {
    const head = readFileSync(stt).subarray(0, 512).toString("utf8");
    assert(head.includes('"use httpclient"'), `stt directive ${head.slice(0, 60)}`);
  } catch {
    // dist not built yet
  }

  console.log("hiperdex tests passed", AUTH_HEADER, BASE);
};

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
