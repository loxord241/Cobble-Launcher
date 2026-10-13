"""Диагностика: сканирует пути всех библиотек (родитель + ребёнок + классификаторы)
на незаконные для Windows имена (ошибка os error 123)."""
import json
import os
import re

base = os.path.expandvars(r"%LOCALAPPDATA%\mc-launcher-v2")
child = json.load(open(os.path.join(base, r"instances\3c6e9b3b-ad87-49ff-a8f5-b4bc35166afa\versions\neoforge-21.1.99.json"), encoding="utf-8"))
parent = json.load(open(os.path.join(base, r"cache\manifests\1.21.1.json"), encoding="utf-8"))

illegal = re.compile(r'[<>:"|?*\x00-\x1f]')
reserved = {"CON", "PRN", "AUX", "NUL", *(f"COM{i}" for i in range(1, 10)), *(f"LPT{i}" for i in range(1, 10))}


def check(path, label):
    problems = []
    if illegal.search(path):
        problems.append("illegal-char")
    for part in path.replace("\\", "/").split("/"):
        stem = part.split(".")[0].upper()
        if stem in reserved:
            problems.append("reserved:" + part)
        if part != part.strip() or part.endswith("."):
            problems.append("trailing:" + repr(part))
    if problems:
        print(label, "→", path, "→", problems)


for tag, d in (("child", child), ("parent", parent)):
    for l in d.get("libraries", []):
        dl = l.get("downloads") or {}
        a = dl.get("artifact") or {}
        if a.get("path"):
            check(a["path"], tag + "-artifact:" + l["name"])
        for cname, c in (dl.get("classifiers") or {}).items():
            if c.get("path"):
                check(c["path"], tag + f"-cls[{cname}]:" + l["name"])
        parts = l["name"].split(":")
        if len(parts) >= 3:
            g, art, v = parts[:3]
            rel = "/".join(g.split(".")) + f"/{art}/{v}/"
            check(rel, tag + "-maven-dir:" + l["name"])
    ai = (d.get("assetIndex") or {})
    print(tag, "assetIndex id:", ai.get("id"), "| logging file id:", ((d.get("logging") or {}).get("client") or {}).get("file", {}).get("id"))
    print(tag, "client sha1:", ((d.get("downloads") or {}).get("client") or {}).get("sha1", "НАСЛЕДУЕТ"))
print("scan done")
