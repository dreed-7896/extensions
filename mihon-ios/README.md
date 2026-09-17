# MihonBare

On-device Keiyoushi/Mihon APK loader for iOS (LiveContainer). Catalog/install
matches the public TachiManga model: empty until you install an APK; each
repo is listed separately so duplicate packages (same `pkg` in Keiyoushi and
Cursed) are not auto-merged.

Hardcoded repos (Mihon 0.20 `index.pb`; `index.min.json` is accepted too):

- Keiyoushi `https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.pb`
- Cursed `https://github.com/yuzono/cursed-manga-repo/raw/repo/index.pb`

(`index.min.json` on Keiyoushi is two “Outdated App” stubs.)

APKs are Dex-loaded the Mihon way: `tachiyomi.extension.class` → `Source` or
`SourceFactory`. Cloudflare: same desktop Chrome UA for WKWebView + HTTP.
After a challenge, fetches retry through that WebView (`fetch()`, same TLS as
`cf_clearance`) instead of copying cookies onto URLSession.

Page images go through the extension `imageRequest` + OkHttp client (MD@Home
tokens, decrypt interceptors). Covers are a plain `GET` with source headers.

## IPA (LiveContainer)

This Linux environment cannot compile iOS. GitHub Action `build-ios-ipa` on macOS produces **`MihonBare.ipa`** (ad-hoc signed).

LiveContainer → + → import the IPA.

## Prove the engine (Linux)

```sh
cd mihon-ios/runtime
cargo run --release --bin mihon-host -- popular /path/to/some.apk
```

MangaPill APK (Keiyoushi) already returns real titles through this host.

## Mac

```sh
bash mihon-ios/scripts/build-ipa.sh
# mihon-ios/dist/MihonBare.ipa
```
