"""Диагностика дублей в classpath: реплицирует merge_chain + build_classpath."""
import json
import os

base = os.path.expandvars(r"%LOCALAPPDATA%\mc-launcher-v2")
child = json.load(open(os.path.join(base, r"instances\3c6e9b3b-ad87-49ff-a8f5-b4bc35166afa\versions\neoforge-21.1.99.json"), encoding="utf-8"))
parent = json.load(open(os.path.join(base, r"cache\manifests\1.21.1.json"), encoding="utf-8"))

# merge_chain: ребёнок приоритетнее, дедуп по name (первый встреченный остаётся)
merged = []
seen = set()
for lib in child["libraries"] + parent["libraries"]:
    if lib["name"] in seen:
        continue
    seen.add(lib["name"])
    merged.append(lib)

paths = {}
for lib in merged:
    a = (lib.get("downloads") or {}).get("artifact") or {}
    p = a.get("path")
    if p:
        paths.setdefault(p, []).append(lib["name"])

dups = {p: ns for p, ns in paths.items() if len(ns) > 1}
print("библиотек после дедупа по name:", len(merged))
print("дублирующихся путей:", len(dups))
for p, ns in list(dups.items())[:10]:
    print(" DUP:", p)
    for n in ns:
        print("      -", n)
