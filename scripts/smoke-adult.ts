import emulate from "@suwatte/toolchain/emulator";
import {
  ChapterSchema,
  ContentSchema,
  ItemSchema,
} from "@suwatte/toolchain/validate";

const sources = [
  { name: "nhentai", mod: () => import("../src/sources/nhentai/index.ts") },
  { name: "hitomi", mod: () => import("../src/sources/hitomi/index.ts") },
  { name: "ehentai", mod: () => import("../src/sources/ehentai/index.ts") },
  {
    name: "pandachaika",
    mod: () => import("../src/sources/pandachaika/index.ts"),
  },
  {
    name: "hentai2read",
    mod: () => import("../src/sources/hentai2read/index.ts"),
  },
  {
    name: "hentainexus",
    mod: () => import("../src/sources/hentainexus/index.ts"),
  },
  {
    name: "hentairead",
    mod: () => import("../src/sources/hentairead/index.ts"),
  },
  {
    name: "novelcrow",
    mod: () => import("../src/sources/novelcrow/index.ts"),
  },
] as const;

const assert = (condition: unknown, message: string) => {
  if (!condition) throw new Error(message);
};

const smokeOne = async (name: string, load: () => Promise<{ default: unknown }>) => {
  const mod = await load();
  const source = emulate(mod.default as never);
  console.log(`\n=== ${name} ===`);

  const home = await source.getHomePage!();
  assert(home.feeds.length >= 1, `${name}: expected homepage feeds`);
  console.log(
    "home feeds:",
    home.feeds.map((feed) => feed.title).join(", "),
  );

  const listKey = home.feeds[0]?.content.list?.key ?? "popular";
  const list = await source.getItemList({ key: listKey }, 1);
  assert(list.items.length > 0, `${name}: empty homepage list (${listKey})`);
  for (const item of list.items.slice(0, 5)) ItemSchema.parse(item);
  console.log(
    `list(${listKey}):`,
    list.items.length,
    "first:",
    list.items[0]?.title,
  );

  const search = await source.getSearchResults({ query: "" }, 1);
  assert(search.items.length > 0, `${name}: empty blank search`);
  for (const item of search.items.slice(0, 5)) ItemSchema.parse(item);

  const contentId = list.items[0]!.id;
  const content = await source.getContent(contentId);
  ContentSchema.parse(content);
  console.log("content:", content.title);

  const chapters = await source.getChapters!(contentId);
  assert(chapters.length > 0, `${name}: no chapters`);
  for (const chapter of chapters) ChapterSchema.parse(chapter);

  const pages = await source.getChapterPages!(contentId, chapters[0]!.id);
  assert(pages.length > 0, `${name}: no pages`);
  assert(
    pages.every((page) => !!(page.url || page.b64)),
    `${name}: pages missing url/b64`,
  );
  console.log("pages:", pages.length, pages[0]?.url ? "url" : "b64");
  console.log(`${name}: OK`);
};

const run = async () => {
  const failures: string[] = [];
  for (const entry of sources) {
    try {
      await smokeOne(entry.name, entry.mod);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      console.error(`${entry.name}: FAIL`, message);
      failures.push(`${entry.name}: ${message}`);
    }
  }
  if (failures.length) {
    console.error("\nFailures:\n" + failures.join("\n"));
    process.exit(1);
  }
  console.log("\nAll adult sources smoke OK");
};

run();
