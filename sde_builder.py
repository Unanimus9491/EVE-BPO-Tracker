"""Builds the BPO list from the official EVE Online Static Data Export (JSONL zip).

Only four files from the SDE are read: types, groups, categories and blueprints.
The result is a compact list of dicts saved as bpos.json in the app data folder.
"""
from __future__ import annotations

import io
import json
import os
import tempfile
import urllib.request
import zipfile
from typing import Callable, Iterator

SDE_URL = "https://developers.eveonline.com/static-data/eve-online-static-data-latest-jsonl.zip"
USER_AGENT = "EVE-BPO-Collection-Tracker/1.0 (personal collection tool)"

META_GROUPS = {
    1: "Tech I", 2: "Tech II", 3: "Storyline", 4: "Faction",
    5: "Officer", 6: "Deadspace", 14: "Tech III",
}

# Product category name -> tab in the tracker
TAB_FOR_CATEGORY = {
    "Ship": "Ships", "Subsystem": "Ships",
    "Module": "Modules",
    "Charge": "Charges",
    "Drone": "Drones", "Fighter": "Drones",
    "Structure": "Structures", "Structure Module": "Structures", "Starbase": "Structures",
    "Deployable": "Structures", "Orbitals": "Structures",
    "Sovereignty Structures": "Structures", "Infrastructure Upgrades": "Structures",
    "Commodity": "Components", "Material": "Components",
}
TABS = ["All", "Ships", "Modules", "Charges", "Drones", "Structures",
        "Components", "Reactions", "Other"]

ProgressFn = Callable[[str, int, int], None]  # (message, done, total)


def download_sde(dest_path: str, progress: ProgressFn | None = None) -> str:
    req = urllib.request.Request(SDE_URL, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=60) as resp, open(dest_path, "wb") as out:
        total = int(resp.headers.get("Content-Length") or 0)
        done = 0
        while True:
            chunk = resp.read(1024 * 256)
            if not chunk:
                break
            out.write(chunk)
            done += len(chunk)
            if progress:
                progress("Downloading game data…", done, total)
    return dest_path


def _iter_jsonl(zf: zipfile.ZipFile, basename: str) -> Iterator[dict]:
    matches = [n for n in zf.namelist() if os.path.basename(n) == basename]
    if not matches:
        raise FileNotFoundError(f"{basename} not found in the SDE archive")
    with zf.open(matches[0]) as raw:
        for line in io.TextIOWrapper(raw, encoding="utf-8"):
            line = line.strip()
            if line:
                yield json.loads(line)


def _en(name) -> str:
    if isinstance(name, dict):
        return name.get("en") or next(iter(name.values()), "")
    return str(name or "")


def build_dataset(zip_path: str, progress: ProgressFn | None = None) -> list[dict]:
    def step(msg: str, n: int):
        if progress:
            progress(msg, n, 5)

    with zipfile.ZipFile(zip_path) as zf:
        step("Reading categories…", 0)
        categories = {c["_key"]: _en(c.get("name")) for c in _iter_jsonl(zf, "categories.jsonl")}
        step("Reading groups…", 1)
        groups = {g["_key"]: (_en(g.get("name")), g.get("categoryID"))
                  for g in _iter_jsonl(zf, "groups.jsonl")}
        step("Reading item types…", 2)
        types = {}
        for t in _iter_jsonl(zf, "types.jsonl"):
            types[t["_key"]] = {
                "name": _en(t.get("name")),
                "groupID": t.get("groupID"),
                "published": bool(t.get("published")),
                "marketGroupID": t.get("marketGroupID"),
                "metaGroupID": t.get("metaGroupID"),
                "techLevel": t.get("techLevel"),
            }
        step("Reading blueprints…", 3)
        out = []
        for bp in _iter_jsonl(zf, "blueprints.jsonl"):
            bp_id = bp["_key"]
            bpt = types.get(bp_id)
            if not bpt or not bpt["published"]:
                continue
            acts = bp.get("activities") or {}
            kind, act = None, None
            if acts.get("manufacturing", {}).get("products"):
                kind, act = "manufacturing", acts["manufacturing"]
            elif acts.get("reaction", {}).get("products"):
                kind, act = "reaction", acts["reaction"]
            if not act:
                continue
            prod_id = act["products"][0]["typeID"]
            prod = types.get(prod_id, {})
            g_name, cat_id = groups.get(prod.get("groupID"), ("Unknown", None))
            cat_name = categories.get(cat_id, "Unknown")
            if kind == "reaction":
                tab = "Reactions"
            else:
                tab = TAB_FOR_CATEGORY.get(cat_name, "Other")
            meta = META_GROUPS.get(prod.get("metaGroupID"))
            if not meta:
                tl = prod.get("techLevel") or 1
                meta = f"Tech {['I', 'II', 'III'][min(max(tl, 1), 3) - 1]}"
            out.append({
                "id": bp_id,
                "name": bpt["name"],
                "product_id": prod_id,
                "product_name": prod.get("name", ""),
                "tab": tab,
                "category": cat_name,
                "group": g_name,
                "tech": meta,
                "market": bpt["marketGroupID"] is not None,
                "kind": kind,
                "researchable": "research_material" in acts or "research_time" in acts,
            })
        step("Done", 5)
    out.sort(key=lambda e: e["name"].lower())
    return out


def download_and_build(progress: ProgressFn | None = None) -> list[dict]:
    fd, tmp = tempfile.mkstemp(suffix=".zip")
    os.close(fd)
    try:
        download_sde(tmp, progress)
        return build_dataset(tmp, progress)
    finally:
        try:
            os.remove(tmp)
        except OSError:
            pass
