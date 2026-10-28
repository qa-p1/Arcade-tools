#!/usr/bin/env python3
"""Check the recorded hashes of everything vendored: Link sources, the design
kit (scripts/sync-design.py) and the adapted Tauri NSIS template."""
from pathlib import Path
import hashlib
import re

root = Path(__file__).resolve().parents[1]
text = (root / "VENDORED").read_text()
checked = 0
for line in text.splitlines():
    fields = line.split()
    if len(fields) >= 2 and fields[1].startswith("sha256:"):
        path, expected = fields[0], fields[1].removeprefix("sha256:")
        if not (root / path).is_file():
            raise SystemExit(f"Vendored file missing: {path}")
        actual = hashlib.sha256((root / path).read_bytes()).hexdigest()
        if actual != expected:
            raise SystemExit(f"Vendored file changed: {path} (refresh it with its sync step, never by hand)")
        checked += 1
# Every Link copy comes from the tag Cargo.toml pins.
pinned = re.search(r'arcade-link = \{[^}]*tag = "([^"]+)"', (root / "Cargo.toml").read_text()).group(1)
tags = set(re.findall(r"^Tag: (\S+)$", text, re.M))
if tags != {pinned}:
    raise SystemExit(f"VENDORED names Link tags {sorted(tags)}, but Cargo.toml pins {pinned}")
design = sorted(p.relative_to(root).as_posix() for p in (root / "src-ui/design").rglob("*") if p.is_file())
missing = [p for p in design if p not in text]
if missing:
    raise SystemExit(f"Not recorded in VENDORED: {', '.join(missing)}")
print(f"Verified {checked} vendored files: Link {pinned}, the design kit and the adapted Tauri NSIS template")
