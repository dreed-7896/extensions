# MihonBare

On-device Keiyoushi/Mihon APK loader for iOS (LiveContainer).

Repo URL is hardcoded to the current Keiyoushi protobuf catalog:

`https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.pb`

(`index.json` is two “Outdated App” stubs. Mihon 0.20+ reads `index.pb`.)

Cloudflare: same desktop Chrome UA for WKWebView + every request (clearance is UA-bound). Challenge HTML on HTTP 200 is treated as 403 after the overlay, and `cf_clearance` cookies are merged onto the extension Cookie header.

Page images go through the extension `imageRequest` + OkHttp client (MD@Home tokens, decrypt interceptors). Covers are a plain `GET` with source headers — they are not `Page` objects.

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
