const assert = require('node:assert/strict');
const { buildSync } = require('esbuild');
const CryptoJS = require('crypto-js');
const { ContentSchema, ChapterSchema, PagedItemListSchema, ChapterPageSchema, HomePageSchema } = require('@suwatte/toolchain/validate');

function load(entry) {
  const code = buildSync({ entryPoints: [entry], bundle: true, platform: 'node', format: 'cjs', write: false }).outputFiles[0].text;
  const module = { exports: {} };
  Function('require', 'module', 'exports', code)(require, module, module.exports);
  return module.exports.default;
}

const pages = new Map();
global.HttpClient = class {
  constructor(config) { this.base = config.baseUrl; }
  async get(path) {
    const url = new URL(path, this.base).href;
    const data = pages.get(url);
    assert.ok(data !== undefined, `Missing fixture: ${url}`);
    return { status: 200, text: async () => typeof data === 'string' ? data : JSON.stringify(data), json: async () => data };
  }
};
const BASE_N = 'https://nhentai.net';
pages.set(`${BASE_N}/api/v2/search?query=%20&page=1&sort=date`, {
  result: [{ id: 123, english_title: 'Sample', thumbnail: '/galleries/a.jpg' }], num_pages: 1,
});
pages.set(`${BASE_N}/api/v2/galleries/123`, {
  id: 123, title: { english: 'Sample' }, cover: { path: '/galleries/a.jpg' },
  pages: [{ path: '/galleries/1.jpg' }], tags: [], num_pages: 1,
  num_favorites: 1, upload_date: 1700000000,
});
const BASE_C = 'https://novelcrow.com';
pages.set(`${BASE_C}/?s=&post_type=wp-manga`, `<div class="page-item-detail"><div class="post-title"><a href="/comic/sample/">Sample</a></div><img data-src="/cover.jpg"></div>`);
pages.set(`${BASE_C}/comic/sample/`, `<h1>Sample</h1><div class="summary_image"><img src="/cover.jpg"></div><li class="wp-manga-chapter"><a href="/comic/sample/chapter-1/">Chapter 1</a></li>`);
const cipher = CryptoJS.AES.encrypt(JSON.stringify(['/page-1.jpg']), 'nonce-example').toString();
pages.set(`${BASE_C}/comic/sample/chapter-1/`, `<script>var nonce='nonce-example'; var chapter_data='${cipher}';</script>`);
const BASE_H = 'https://hentairead.com';
pages.set(`${BASE_H}/?s=&post_type=wp-manga`, `<div class="manga-item"><h3><a href="/hentai/sample/">Sample</a></h3><img src="/cover.jpg"></div>`);
pages.set(`${BASE_H}/hentai/sample/`, `<div class="manga-titles"><h1>Sample</h1></div><meta property="og:image" content="/cover.jpg"><li class="wp-manga-chapter"><a href="/hentai/sample/chapter-1/">Chapter 1</a></li>`);
pages.set(`${BASE_H}/hentai/sample/chapter-1/`, `<div class="reading-content"><img data-src="/page-1.jpg"></div>`);

(async () => {
  for (const [entry, id] of [
    ['src/sources/nhentai/index.ts', '123'],
    ['src/sources/novelcrow/index.ts', 'comic/sample'],
    ['src/sources/hentairead/index.ts', 'hentai/sample'],
  ]) {
    const Source = load(entry);
    const source = new Source();
    const list = await source.getSearchResults({}, 1);
    PagedItemListSchema.parse(list);
    assert.equal(list.items.length, 1);
    const content = await source.getContent(id);
    ContentSchema.parse(content);
    const chapters = await source.getChapters(id);
    assert.equal(chapters.length, 1);
    ChapterSchema.parse(chapters[0]);
    const images = await source.getChapterPages(id, chapters[0].id);
    assert.equal(images.length, 1);
    ChapterPageSchema.parse(images[0]);
    HomePageSchema.parse(await source.getHomePage());
    console.log(`PASS ${Source.info.id}`);
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
