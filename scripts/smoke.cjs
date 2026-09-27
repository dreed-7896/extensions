const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const { buildSync } = require('esbuild');
const CryptoJS = require('crypto-js');
const { ContentSchema, ChapterSchema, PagedItemListSchema, ChapterPageSchema, HomePageSchema } = require('@suwatte/toolchain/validate');
global.CloudflareError = require('@suwatte/toolchain/emulator').CloudflareError;

function load(entry) {
  const code = buildSync({ entryPoints: [entry], bundle: true, platform: 'node', format: 'cjs', write: false }).outputFiles[0].text;
  const module = { exports: {} };
  Function('require', 'module', 'exports', code)(require, module, module.exports);
  return module.exports.default;
}

const pages = new Map();
global.HttpClient = class {
  constructor(config) { this.base = config.baseUrl; this.cloudflareResolutionURL = config.cloudflareResolutionURL; }
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
pages.set(`${BASE_H}/?s=&title-type=contains&sortby=latest`, `<div class="manga-item"><h3><a href="/hentai/sample/">Sample</a></h3><img src="/cover.jpg"></div>`);
pages.set(`${BASE_H}/hentai/?sortby=latest`, `<div class="manga-grid"><div class="manga-item"><h3><a href="/hentai/gallery/" title="Gallery"><svg></svg></a></h3><img alt="Gallery" src="/gallery-cover.jpg"></div></div>`);
pages.set(`${BASE_H}/hentai/sample/`, `<div class="manga-titles"><h1>Sample</h1></div><meta property="og:image" content="/cover.jpg"><li class="wp-manga-chapter"><a href="/hentai/sample/chapter-1/">Chapter 1</a></li>`);
pages.set(`${BASE_H}/hentai/sample/chapter-1/`, `<div class="reading-content"><img data-src="/page-1.jpg"></div>`);
pages.set(`${BASE_H}/hentai/gallery/p/1/`, `<div class="chapter-image-item"><img data-src="https://hencover.xyz/preview/gallery/01.jpg"></div>`);
pages.set(`${BASE_H}/hentai/gallery/`, `<meta property="og:title" content="Gallery"><meta name="description" content="Gallery description"><meta property="og:image" content="/gallery-cover.jpg">`);
pages.set(`${BASE_H}/hentai/json/p/1/`, `<script id="single-chapter-js-extra">var pagesData={"data":{"chapter":{"images":[{"src":"/json-page.jpg"}]}}};</script>`);
pages.set(`${BASE_H}/hentai/json/`, `<h1>JSON Gallery</h1>`);

(async () => {
  for (const [entry, id] of [
    ['src/sources/nhentai/index.ts', '123'],
    ['src/sources/novelcrow/index.ts', 'comic/sample'],
    ['src/sources/hentairead/index.ts', 'hentai/sample'],
  ]) {
    const Source = load(entry);
    const source = new Source();
    assert.equal(source.client.cloudflareResolutionURL, `${source.client.base}/`);
    assert.equal(source.getConfiguration().cloudflareResolutionURL, `${source.client.base}/`);
    assert.equal(source.getConfiguration().useClientForImageRequests, true);
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
  // Evaluate the actual published bundles with the JavaScriptCore-like globals.
  // Node's normal context supplies atob and URL, hiding missing device globals.
  for (const [name, id] of [['nhentai', '123'], ['novelcrow', 'comic/sample'], ['hentairead', 'hentai/sample']]) {
    const sandbox = { console, HttpClient: class {
      constructor(config) { this.base = config.baseUrl; }
      async get(path) {
        const url = new URL(path, this.base).href;
        const data = pages.get(url);
        assert.ok(data !== undefined, `Missing fixture: ${url}`);
        return { status: 200, text: async () => typeof data === 'string' ? data : JSON.stringify(data), json: async () => data };
      }
    } };
    const bundle = fs.readFileSync(`dist/sources/${name}.stt`, 'utf8');
    const pkg = vm.runInNewContext(`${bundle}\nSourcePackage;`, sandbox, { timeout: 2000 });
    const source = pkg.bootstrap();
    assert.equal((await source.getSearchResults({}, 1)).items.length, 1);
    assert.equal((await source.getContent(id)).title, 'Sample');
    const chapters = await source.getChapters(id);
    assert.equal((await source.getChapterPages(id, chapters[0].id)).length, 1);
    console.log(`PASS bundled ${name}`);
  }
  const HentaiRead = load('src/sources/hentairead/index.ts');
  const hentaiRead = new HentaiRead();
  assert.equal((await hentaiRead.getItemList({ key: 'recent' }, 1)).items[0].title, 'Gallery');
  assert.equal((await hentaiRead.getContent('hentai/gallery')).title, 'Gallery');
  assert.equal((await hentaiRead.getContent('hentai/gallery')).summary, 'Gallery description');
  assert.equal((await hentaiRead.getContent('hentai/gallery')).coverImage, `${BASE_H}/gallery-cover.jpg`);
  const galleryChapter = (await hentaiRead.getChapters('hentai/gallery'))[0];
  assert.equal(galleryChapter.id, '/hentai/gallery/p/1/');
  assert.equal((await hentaiRead.getChapterPages('hentai/gallery', galleryChapter.id))[0].url, 'https://henread.xyz/gallery/01.jpg');
  assert.equal((await hentaiRead.getChapterPages('hentai/json', '/hentai/json/p/1/'))[0].url, `${BASE_H}/json-page.jpg`);
  console.log('PASS HentaiRead gallery reader and metadata');
  const challenge = '<html><title>Just a moment...</title><script src="/cdn-cgi/challenge-platform/main.js"></script></html>';
  pages.set(`${BASE_C}/?s=&post_type=wp-manga`, challenge);
  pages.set(`${BASE_H}/?s=&title-type=contains&sortby=latest`, challenge);
  for (const [Source, url] of [[load('src/sources/novelcrow/index.ts'), BASE_C], [HentaiRead, BASE_H]]) {
    await assert.rejects(new Source().getSearchResults({}, 1), (error) =>
      error.name === 'CloudflareError' && error.resolutionURL === `${url}/`);
  }
  console.log('PASS HTTP 200 Cloudflare challenge handling');
  const catalog = JSON.parse(fs.readFileSync('dist/sources.json', 'utf8'));
  const readme = fs.readFileSync('README.md', 'utf8');
  const baseUrl = readme.match(/https:\/\/raw\.githubusercontent\.com\/[^\s]+\/dist\b/)?.[0];
  assert.ok(baseUrl, 'README must give the catalog directory, not sources.json');
  for (const item of catalog.sources) {
    const bundle = new URL(`sources/${item.path}.stt`, `${baseUrl}/`);
    assert.ok(fs.existsSync(`dist/sources/${item.path}.stt`));
    assert.ok(bundle.pathname.endsWith(`/dist/sources/${item.path}.stt`));
  }
  const http = require('node:http');
  const { HttpClient } = require('@suwatte/toolchain/emulator');
  const server = http.createServer((_req, response) => {
    response.writeHead(403, { 'cf-mitigated': 'challenge' });
    response.end('Checking your browser');
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  try {
    const client = new HttpClient({
      baseUrl: `http://127.0.0.1:${server.address().port}`,
      cloudflareResolutionURL: BASE_H,
    });
    await assert.rejects(client.get('/'), (error) =>
      error.name === 'CloudflareError' && error.resolutionURL === BASE_H);
    console.log('PASS Cloudflare challenge URL');
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
