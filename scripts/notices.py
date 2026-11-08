#!/usr/bin/env python3
"""Write THIRD_PARTY_NOTICES.md: every third-party part that ships inside Arcade Tools, with its licence text.

  scripts/notices.py           # rewrite THIRD_PARTY_NOTICES.md
  scripts/notices.py --check   # fail if it is out of date (scripts/check runs this)

Rust: the normal (non-dev, non-build) dependency graph of the shipped binaries, for every target platform, from
`cargo metadata --locked`; each crate's own licence files from its source. JavaScript: the packages the built
interface bundles (svelte's runtime and @tauri-apps/api; `vite build` puts nothing else in). Fonts: Inter.
Identical licence texts are printed once, with every component that uses them.
"""
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "THIRD_PARTY_NOTICES.md"
SHIPPED = ("arcade-tools", "arcade-tools-overlay")
BUNDLED_JS = ("svelte", "@tauri-apps/api")
STANDARD = ROOT / "scripts" / "licences"
PREFERRED = ("MIT", "Apache-2.0", "Zlib", "BSD-3-Clause", "MPL-2.0", "Unlicense")
LICENCE_FILE = re.compile(r"^(licen[cs]e|copying|notice|copyright)([-._].*)?$", re.I)


def normalized(text):
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").replace("\r", "\n").split("\n")]
    while lines and not lines[0]:
        lines.pop(0)
    while lines and not lines[-1]:
        lines.pop()
    return "\n".join(lines)


def licence_texts(folder, declared=None):
    files = []
    if declared:
        files.append(folder / declared)
    if folder.is_dir():
        files += sorted(p for p in folder.iterdir() if p.is_file() and LICENCE_FILE.match(p.name))
        sub = folder / "LICENSES"
        if sub.is_dir():
            files += sorted(p for p in sub.iterdir() if p.is_file())
    seen, texts = set(), []
    for path in files:
        if path.resolve() in seen or not path.is_file():
            continue
        seen.add(path.resolve())
        text = normalized(path.read_text(encoding="utf-8", errors="replace"))
        if text:
            texts.append(text)
    return texts


def checkout_root(folder):
    """The repository root of a git dependency (licence files usually live there, not in the crate's folder)."""
    for parent in folder.parents:
        if (parent / ".git").exists() or parent.parent.name == "checkouts":
            return parent
    return None


def standard_text(expression, authors, name):
    """The SPDX text of the first preferred licence the crate offers, with its authors as copyright holders."""
    offered = re.split(r"\s+OR\s+|/", expression or "")
    for spdx in PREFERRED:
        if spdx in offered:
            holders = ", ".join(re.sub(r"\s*<[^>]*>", "", a) for a in authors) or f"the {name} developers"
            text = (STANDARD / f"{spdx}.txt").read_text(encoding="utf-8")
            text = re.sub(r"Copyright \(c\) <year> <(copyright holders|owner)>\.?", f"Copyright (c) {holders}", text)
            return [normalized(f"{name} is offered under {expression}; it publishes no licence file, so the standard "
                               f"{spdx} text follows.\n\n{text}")]
    return []


def rust_crates():
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1", "--all-features"],
        cwd=ROOT, check=True, capture_output=True, text=True).stdout)
    packages = {p["id"]: p for p in meta["packages"]}
    members = set(meta["workspace_members"])
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    todo = [i for i in members if packages[i]["name"] in SHIPPED]
    if len(todo) != len(SHIPPED):
        sys.exit(f"notices: expected workspace crates {SHIPPED}")
    reached = set()
    while todo:
        node = nodes[todo.pop()]
        for dep in node["deps"]:
            if any(k["kind"] is None for k in dep["dep_kinds"]) and dep["pkg"] not in reached:
                reached.add(dep["pkg"])
                todo.append(dep["pkg"])
    crates = []
    for pid in reached - members:
        p = packages[pid]
        folder = Path(p["manifest_path"]).parent
        texts = licence_texts(folder, p.get("license_file"))
        if not texts and p["source"] and p["source"].startswith("git+"):
            root = checkout_root(folder)
            texts = licence_texts(root) if root else []
        if not texts:
            texts = standard_text(p.get("license"), p.get("authors") or [], p["name"])
        crates.append({
            "label": f'{p["name"]} {p["version"]}',
            "licence": p.get("license") or ("see licence file" if p.get("license_file") else "unspecified"),
            "home": p.get("repository") or p.get("homepage") or "",
            "texts": texts,
        })
    return crates


def js_packages():
    out = []
    for name in BUNDLED_JS:
        folder = ROOT / "node_modules" / name
        info = json.loads((folder / "package.json").read_text(encoding="utf-8"))
        repo = info.get("repository")
        repo = repo.get("url", "") if isinstance(repo, dict) else (repo or "")
        out.append({
            "label": f'{name} {info["version"]}',
            "licence": info.get("license", "unspecified"),
            "home": re.sub(r"^git\+|\.git$", "", repo),
            "texts": licence_texts(folder),
        })
    return out


def render():
    fonts = [{"label": "Inter (typeface)", "licence": "OFL-1.1", "home": "https://rsms.me/inter/",
              "texts": [normalized((ROOT / "src-ui/design/fonts/OFL.txt").read_text(encoding="utf-8"))]}]
    sections = [("Fonts", fonts), ("JavaScript", js_packages()), ("Rust crates", rust_crates())]
    missing = [c["label"] for _, group in sections for c in group if not c["texts"]]
    if missing:
        sys.exit("notices: no licence text found for " + ", ".join(sorted(missing)))

    lines = [
        "# Third-party notices",
        "",
        "Arcade Tools is MIT licensed (see `LICENSE`). It includes the open-source parts below, each under its own",
        "licence. This file is generated by `scripts/notices.py`; do not edit it by hand.",
        "",
    ]
    by_text = {}
    for title, group in sections:
        lines += [f"## {title}", ""]
        for c in sorted(group, key=lambda c: c["label"].lower()):
            home = f" — {c['home']}" if c["home"] else ""
            lines.append(f"- {c['label']} ({c['licence']}){home}")
            for text in c["texts"]:
                by_text.setdefault(text, []).append(c["label"])
        lines.append("")
    lines += ["## Licence texts", ""]
    for n, (text, users) in enumerate(sorted(by_text.items(), key=lambda kv: (sorted(kv[1])[0].lower(), kv[0])), 1):
        users = sorted(set(users), key=str.lower)
        lines += [f"### {n}. Used by {', '.join(users)}", "", "```text", text.replace("```", "'''"), "```", ""]
    return "\n".join(lines)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="fail if THIRD_PARTY_NOTICES.md is out of date")
    args = ap.parse_args()
    text = render()
    if args.check:
        current = normalized(OUT.read_text(encoding="utf-8")) if OUT.exists() else ""
        if current != normalized(text):
            sys.exit("THIRD_PARTY_NOTICES.md is out of date: run scripts/notices.py")
        print("THIRD_PARTY_NOTICES.md is current")
        return
    OUT.write_text(text + "\n", encoding="utf-8", newline="\n")
    print(f"Wrote {OUT.name}: {len(text) // 1024} KiB")


if __name__ == "__main__":
    main()
