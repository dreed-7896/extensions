# Suwatte v7 sources

Sources for NHentai, NovelCrow and HentaiRead. Built with the official `@suwatte/toolchain`.

Add this source list in Suwatte **Settings → Sources → Add Source List**:

https://raw.githubusercontent.com/dreed-7896/extensions/suwatte-v7-sources/dist

Enter the catalog **folder address**, without `/sources.json`. Suwatte appends `/sources.json` and resolves each bundle from that folder. Then install each source from the list. The catalog and its `.stt` bundles are committed together on the `suwatte-v7-sources` branch.

## Updating

Use Node.js 22 or newer. Run `npm ci`, `npm run check`, `npm run build`, then commit `src/`, `package-lock.json`, and `dist/` on this branch. Increase a source's numeric `version` before releasing an update. The source list URL stays the same.

NovelCrow and HentaiRead use their current site HTML and may require changes when their websites update. On a Cloudflare challenge, Suwatte can open the corresponding site to establish a session. NovelCrow also supports its salted chapter payload when the ordinary chapter images are absent.
