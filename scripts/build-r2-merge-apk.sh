#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
YUZONO="${YUZONO_DIR:-$ROOT/.ref/yuzono}"
ANDROID_SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/android-sdk}}"

if [[ ! -d "$YUZONO/gradle/build-logic" ]]; then
  echo "Yuzono host missing at $YUZONO" >&2
  echo "git clone --depth 1 https://github.com/yuzono/cursed-manga-extensions.git $YUZONO" >&2
  exit 1
fi

if [[ ! -d "$ANDROID_SDK/platforms" ]]; then
  echo "Android SDK not found at $ANDROID_SDK" >&2
  exit 1
fi

export ANDROID_HOME="$ANDROID_SDK"
export ANDROID_SDK_ROOT="$ANDROID_SDK"

rm -rf "$YUZONO/src/all/r2merge"
cp -a "$ROOT/mihon/src/all/r2merge" "$YUZONO/src/all/r2merge"

if [[ -d "$YUZONO/.git" ]]; then
  git -C "$YUZONO" checkout -- settings.gradle.kts >/dev/null 2>&1 || true
fi

python3 - "$YUZONO/settings.gradle.kts" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
text = path.read_text()
text = text.replace(
    'loadAllIndividualExtensions()\n// loadIndividualExtension("all", "mangadex")',
    '// loadAllIndividualExtensions()\nloadIndividualExtension("all", "r2merge")',
    1,
)
# Skip unused lib modules so Gradle only configures core + this extension.
text = text.replace(
    'File(rootDir, "lib").eachDir { include("lib:${it.name}") }',
    '// File(rootDir, "lib").eachDir { include("lib:${it.name}") }',
)
text = text.replace(
    'File(rootDir, "lib-multisrc").eachDir { include("lib-multisrc:${it.name}") }',
    '// File(rootDir, "lib-multisrc").eachDir { include("lib-multisrc:${it.name}") }',
)
path.write_text(text)
PY

printf 'sdk.dir=%s\n' "$ANDROID_SDK" > "$YUZONO/local.properties"

KEYSTORE="$ROOT/scripts/r2-signing.jks"
if [[ ! -f "$KEYSTORE" ]]; then
  echo "Missing $KEYSTORE — generate it with keytool before building." >&2
  exit 1
fi
cp -f "$KEYSTORE" "$YUZONO/signingkey.jks"
cp -f "$KEYSTORE" "$YUZONO/src/all/r2merge/signingkey.jks"
export KEY_STORE_PASSWORD="${KEY_STORE_PASSWORD:-r2library}"
export ALIAS="${ALIAS:-r2library}"
export KEY_ALIAS="${KEY_ALIAS:-r2library}"
export KEY_PASSWORD="${KEY_PASSWORD:-r2library}"

(
  cd "$YUZONO"
  ./gradlew :src:all:r2merge:assembleRelease --no-daemon --stacktrace
)

mkdir -p "$ROOT/repo/apk" "$ROOT/repo/jar" "$ROOT/repo/icon"
rm -f "$ROOT/repo/apk/"tachiyomi-all.r2merge-*.apk
rm -f "$ROOT/repo/jar/"tachiyomi-all.r2merge-*.jar
find "$YUZONO/src/all/r2merge/build/outputs/apk" -name 'tachiyomi-all.r2merge-*.apk' -exec cp {} "$ROOT/repo/apk/" \;
find "$YUZONO/src/all/r2merge/build/outputs/jar" -name 'tachiyomi-all.r2merge-*.jar' -exec cp {} "$ROOT/repo/jar/" \;
cp -f "$ROOT/mihon/src/all/r2merge/res/mipmap-xhdpi/ic_launcher.png" \
  "$ROOT/repo/icon/eu.kanade.tachiyomi.extension.all.r2merge.png"
# Spotless formats the Yūzōnō copy; bring that back so the repo stays in sync.
cp -a "$YUZONO/src/all/r2merge/src/." "$ROOT/mihon/src/all/r2merge/src/"
cp -f "$YUZONO/src/all/r2merge/build.gradle.kts" "$ROOT/mihon/src/all/r2merge/build.gradle.kts"
python3 -c 'import google.protobuf' 2>/dev/null || python3 -m pip install --user protobuf
python3 "$ROOT/scripts/emit-repo-index.py"
