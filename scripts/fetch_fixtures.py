#!/usr/bin/env python3
"""Генератор фикстур: скачивает РЕАЛЬНЫЕ version JSON Mojang (с проверкой
sha1 из манифеста) и Fabric profile JSON, замораживает их в
src-tauri/tests/fixtures/ (спека §9: «замороженные JSON»).

Запуск: python scripts/fetch_fixtures.py
"""
import hashlib
import json
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "src-tauri" / "tests" / "fixtures"
UA = "mc-launcher-v2/0.2.0 (github:loxord241/Cobble-Launcher)"
MANIFEST_URL = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"


def get_json(url: str) -> dict:
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)


def get_bytes(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def main() -> int:
    FIXTURES.mkdir(parents=True, exist_ok=True)
    manifest = get_json(MANIFEST_URL)
    by_id = {v["id"]: v for v in manifest["versions"]}

    for vid, fname in [
        ("1.20.1", "version_1_20_1.json"),
        ("1.12.2", "version_1_12_2.json"),
    ]:
        entry = by_id[vid]
        expected = entry["sha1"]
        data = get_bytes(entry["url"])
        actual = hashlib.sha1(data).hexdigest()
        if actual != expected:
            print(f"ХЭШ НЕ СОВПАЛ для {vid}: {actual} != {expected}")
            return 1
        (FIXTURES / fname).write_bytes(data)
        print(f"OK {vid}: {len(data)} байт, sha1 проверен")

    # Fabric profile: реальный JSON профиля loader+1.20.1
    versions = get_json("https://meta.fabricmc.net/v2/versions/loader")
    loader = versions[0]["version"]
    profile = get_json(
        f"https://meta.fabricmc.net/v2/versions/loader/1.20.1/{loader}/profile/json"
    )
    (FIXTURES / "version_fabric_like.json").write_text(
        json.dumps(profile, ensure_ascii=False, indent=1), encoding="utf-8"
    )
    print(f"OK fabric profile: loader {loader}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
