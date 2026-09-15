#!/usr/bin/env python3
"""Emit TachiManga/Mihon repo indexes (index.pb + legacy index.min.json)."""

from __future__ import annotations

import gzip
import json
import subprocess
import sys
from pathlib import Path

from google.protobuf import json_format

SCRIPTS = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS / "repo-index"))
import index_pb2  # noqa: E402

ROOT = SCRIPTS.parent
REPO = ROOT / "repo"
PKG = "eu.kanade.tachiyomi.extension.all.r2merge"
SOURCE_ID = 8210462026091001
DEFAULT_SIGNING_KEY = "3a6e16769f64921128875f1967c28905262477dd8201bdb11877ea8b6e8116de"
RAW_BASE = (
    "https://raw.githubusercontent.com/raahat-hossain/extensions/"
    "cursor/r2-merge-extension-8f4a/repo"
)


def signing_key(apk: Path) -> str:
    apksigner = Path.home() / "android-sdk/build-tools/36.1.0/apksigner"
    if not apksigner.exists():
        return DEFAULT_SIGNING_KEY
    out = subprocess.check_output(
        [str(apksigner), "verify", "--print-certs", str(apk)],
        text=True,
    )
    for line in out.splitlines():
        if "SHA-256 digest:" in line:
            return line.split(":", 1)[1].strip()
    return DEFAULT_SIGNING_KEY


def main() -> None:
    apks = sorted((REPO / "apk").glob("tachiyomi-all.r2merge-*.apk"))
    if not apks:
        raise SystemExit("no apk in repo/apk")
    apk = apks[-1]
    jar = REPO / "jar" / apk.name.replace(".apk", ".jar")
    icon = REPO / "icon" / f"{PKG}.png"
    version = apk.name.removeprefix("tachiyomi-all.r2merge-v").removesuffix(".apk")
    code = int(version.split(".")[-1])

    index = index_pb2.Index(
        name="R2 Library",
        badgeLabel="R2",
        signingKey=signing_key(apk),
        contact=index_pb2.Contact(website="https://github.com/raahat-hossain/extensions"),
        extensionList=index_pb2.ExtensionList(
            extensions=[
                index_pb2.Extension(
                    name="R2 Library",
                    packageName=PKG,
                    resources=index_pb2.Resources(
                        apkUrl=f"{RAW_BASE}/apk/{apk.name}",
                        iconUrl=f"{RAW_BASE}/icon/{PKG}.png" if icon.exists() else "",
                        jarUrl=f"{RAW_BASE}/jar/{jar.name}" if jar.exists() else "",
                    ),
                    extensionLib="1.4",
                    versionCode=code,
                    versionName=version,
                    contentWarning=index_pb2.CONTENT_WARNING_NSFW,
                    sources=[
                        index_pb2.Source(
                            id=SOURCE_ID,
                            name="R2 Library",
                            language="all",
                            homeUrl="https://developers.cloudflare.com",
                        ),
                    ],
                ),
            ],
        ),
    )

    (REPO / "index.pb").write_bytes(gzip.compress(index.SerializeToString(deterministic=True)))
    (REPO / "index.json").write_text(
        json_format.MessageToJson(
            index,
            always_print_fields_with_no_presence=False,
            preserving_proto_field_name=True,
        )
        + "\n",
        encoding="utf-8",
    )

    legacy = [
        {
            "name": "Tachiyomi: R2 Library",
            "pkg": PKG,
            "apk": apk.name,
            "lang": "all",
            "code": code,
            "version": version,
            "nsfw": 1,
            "sources": [
                {
                    "name": "R2 Library",
                    "lang": "all",
                    "id": str(SOURCE_ID),
                    "baseUrl": "https://developers.cloudflare.com",
                    "versionId": 1,
                },
            ],
        },
    ]
    (REPO / "index.min.json").write_text(json.dumps(legacy, separators=(",", ":")), encoding="utf-8")
    print("wrote", apk.name, "index.pb", (REPO / "index.pb").stat().st_size, "bytes")


if __name__ == "__main__":
    main()
