#!/usr/bin/env python3
"""Measure built HTML/JS/CSS, with Look's 17 KB shell as the budget."""
import gzip
from pathlib import Path

root = Path(__file__).resolve().parents[1] / "dist"
files = sorted(p for p in root.rglob("*") if p.suffix in (".html", ".js", ".css"))
if not files:
    raise SystemExit("Run npm run build first.")
raw = sum(p.stat().st_size for p in files)
compressed = sum(len(gzip.compress(p.read_bytes(), mtime=0)) for p in files)
print(f"UI shell: {raw} bytes raw, {compressed} bytes gzip; {len(files)} files; budget 17000 bytes")
if raw > 17000:
    raise SystemExit("UI shell exceeds the 17 KB budget.")
