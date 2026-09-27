const assert = require('node:assert/strict');
const { buildSync } = require('esbuild');
const { ItemSchema, ContentSchema, ChapterSchema, ChapterPageSchema, HomePageSchema } = require('@suwatte/toolchain/validate');
global.CloudflareError = require('@suwatte/toolchain/emulator').CloudflareError;

function load(name) {
  const code = buildSync({ entryPoints: [`src/sources/${name}/index.ts`], bundle: true,
    platform: 'node', format: 'cjs', write: false }).outputFiles[0].text;
  const module = { exports: {} };
  Function('require', 'module', 'exports', code)(require, module, module.exports);
  return module.exports.default;
}

const requests = [];
global.HttpClient = class {
  constructor(config) { this.base = config.baseUrl; this.cloudflareResolutionURL = config.cloudflareResolutionURL; }
  async get(path) {
    const url = new URL(path, this.base).href;
    requests.push(url);
    let data;
    if (url.startsWith('https://allporncomic.com/')) {
      if (url.includes('/chapter-1/')) data = '<div class="reading-content"><img data-src="/image-1.jpg"></div>';
      else if (url.endsWith('/porncomic/one/')) data = '<h1>One</h1><div class="summary_image"><img src="/cover.jpg"></div><li class="wp-manga-chapter"><a href="/porncomic/one/chapter-1/">Chapter 1</a></li>';
      else data = '<div class="page-item-detail"><div class="post-title"><a href="/porncomic/one/">One</a></div><img src="/cover.jpg"></div>';
    } else if (url.startsWith('https://doujins.com/')) {
      if (url.includes('/folders?')) data = { folders: [{ link: '/gallery/example/', name: 'Example', thumbnail2: '/cover.jpg' }] };
      else if (url.endsWith('/gallery/example/')) data = '<div class="folder-title"><a>Example</a></div><div class="gallery-artist"><a>A</a></div><div class="doujin" data-file="/image-1.jpg"></div>';
      else data = '<div class="thumbnail-doujin"><a class="gallery-visited-from-favorites" href="/gallery/example/"><div class="title"><span class="text">Example</span></div><img src="/cover.jpg"></a></div>';
    } else if (url.startsWith('https://api.omegascans.org/')) {
      const series = { id: 123, series_slug: 'example', title: 'Example', thumbnail: '/cover.jpg', description: '<p>Summary</p>', tags: [{ name: 'Action' }] };
      if (url.includes('/chapter/query?')) data = { data: [{ chapter_name: 'Chapter 1', chapter_slug: 'one', price: 0 }, { chapter_name: 'Chapter 2', chapter_slug: 'paid', price: 5 }] };
      else if (url.includes('/query?')) data = { data: [series], meta: { current_page: 1, last_page: 1 } };
      else if (url.includes('/chapter/example/one')) data = { chapter: { chapter_data: { images: ['/one.jpg'] } } };
      else if (url.endsWith('/series/example')) data = series;
    } else if (url.startsWith('https://imhentai.xxx/')) {
      if (url.endsWith('/gallery/42/')) data = `<div class="gallery_first"><h1>Gallery</h1><div class="left_cover"><img src="/cover.jpg"></div></div><input id="load_dir" value="dir"><input id="load_id" value="42"><input id="gallery_id" value="42"><input id="load_server" value="7"><script>var pages = $.parseJSON('{"2":"w","1":"p"}');</script>`;
      else data = '<div class="thumb"><div class="inner_thumb"><a href="/gallery/42/" title="Gallery"><img src="/cover.jpg"></a></div><div class="caption">Gallery</div></div>';
    }
    assert.notEqual(data, undefined, `Unmatched fixture: ${url}`);
    return { status: 200, text: async () => typeof data === 'string' ? data : JSON.stringify(data) };
  }
};

(async () => {
  const cases = [
    ['allporncomic', 'porncomic/one', 'https://allporncomic.com/image-1.jpg'],
    ['doujins', 'gallery/example', 'https://doujins.com/image-1.jpg'],
    ['omegascans', 'example#123', 'https://api.omegascans.org/one.jpg'],
    ['imhentai', 'gallery/42', 'https://m7.imhentai.xxx/dir/42/1.png'],
  ];
  for (const [name, expectedId, firstPage] of cases) {
    const Source = load(name);
    const source = new Source();
    HomePageSchema.parse(await source.getHomePage());
    const list = await source.getItemList({ key: 'popular' }, 1);
    assert.equal(list.items.length, 1, `${name}: listing`);
    ItemSchema.parse(list.items[0]);
    assert.equal(list.items[0].id, expectedId, `${name}: ID`);
    const content = await source.getContent(expectedId);
    ContentSchema.parse(content);
    assert.ok(content.title, `${name}: title`);
    const chapters = await source.getChapters(expectedId);
    assert.equal(chapters.length, 1, `${name}: chapters`);
    ChapterSchema.parse(chapters[0]);
    const pages = await source.getChapterPages(expectedId, chapters[0].id);
    assert.equal(pages[0].url, firstPage, `${name}: reader`);
    ChapterPageSchema.parse(pages[0]);
    console.log(`PASS Aidoku port ${name}`);
  }
  const OmegaScans = load('omegascans');
  const omega = await new OmegaScans().getSearchResults({ query: 'Example' }, 1);
  assert.equal(omega.items[0].title, 'Example');
  const Doujins = load('doujins');
  assert.equal((await new Doujins().getItemList({ key: 'latest' }, 1)).items[0].title, 'Example');
  assert.ok(requests.some((url) => url.includes('/folders?start=')), 'Doujins latest route missing');
})().catch((error) => { console.error(error); process.exitCode = 1; });
