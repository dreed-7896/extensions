#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/runtime"

TARGET="${1:-aarch64-apple-ios}"
echo "building mihon-host for $TARGET"
rustup target add "$TARGET"
cargo build --release --target "$TARGET" --lib

LIB="$ROOT/runtime/target/$TARGET/release/libmihon_host.a"
mkdir -p "$ROOT/app/libs"
cp "$LIB" "$ROOT/app/libs/libmihon_host.a"
echo "copied $LIB -> app/libs/libmihon_host.a"

if ! command -v xcodebuild >/dev/null; then
  echo "xcodebuild not found; rust lib is ready, IPA needs a Mac"
  exit 0
fi

cd "$ROOT/app"
xcodebuild -project MihonBare.xcodeproj -scheme MihonBare \
  -configuration Release -sdk iphoneos \
  -destination 'generic/platform=iOS' \
  -derivedDataPath "$ROOT/dist/derived" \
  CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO \
  CODE_SIGN_IDENTITY="" build

APP="$ROOT/dist/derived/Build/Products/Release-iphoneos/MihonBare.app"
if [[ ! -d "$APP" ]]; then
  echo "MihonBare.app not found at $APP"
  exit 1
fi

# LiveContainer accepts ad-hoc signed IPAs.
codesign --force --deep --sign - "$APP" 2>/dev/null || true

STAGE="$ROOT/dist/Payload"
rm -rf "$STAGE" "$ROOT/dist/MihonBare.ipa"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
(cd "$ROOT/dist" && zip -qry MihonBare.ipa Payload)
echo "IPA: $ROOT/dist/MihonBare.ipa"
