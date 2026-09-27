# Suwatte v7 sources

Suwatte v7 ports for all 11 sources in the `aidoku-extensions` branch: AllPornComic, Doujins, E-Hentai, Hentai2Read, HentaiNexus, HentaiRead, Hiperdex, IMHentai, NovelCrow, OmegaScans and PandaChaika. The list also includes Hitomi, NHentai and R2 Library from the existing Suwatte branch. Built with the official `@suwatte/toolchain`.

Add this source list in Suwatte **Settings → Sources → Add Source List**:

https://raw.githubusercontent.com/dreed-7896/extensions/suwatte-v7-sources/dist

Enter the catalog **folder address**, without `/sources.json`. Suwatte appends `/sources.json` and resolves each bundle from that folder. Then install each source from the list. The catalog and its `.stt` bundles are committed together on the `suwatte-v7-sources` branch.

## Updating

Use Node.js 22 or newer. Run `npm ci`, `npm run check`, `npm run build`, then commit `src/`, `package-lock.json`, and `dist/` on this branch. Increase a source's numeric `version` before releasing an update. The source list URL stays the same.

NovelCrow and HentaiRead use site HTML and may need updates as the websites change. Cloudflare challenge responses invoke Suwatte's built-in resolution view, including challenge pages served with HTTP 200. A solved challenge does not guarantee the site will accept Suwatte's subsequent native request. HentaiRead resolves the site's root and parses listing titles using the Aidoku selectors with the site's `sortby=new` route. NovelCrow also supports its salted chapter payload when ordinary chapter images are absent.
