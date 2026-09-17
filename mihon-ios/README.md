# MihonBare

On-device Keiyoushi/Mihon APK loader for iOS (LiveContainer).

Repo URL is hardcoded:

`https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.json`

Cloudflare: if a source returns a CF challenge, a WKWebView overlay appears (Safari UA + `cf_clearance` cookie), then the request is retried.

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
