#!/usr/bin/env python3
"""Check the recorded hashes of Link's unchanged shared sources."""
from pathlib import Path
import hashlib

root = Path(__file__).resolve().parents[1]
checked = 0
for line in (root / "VENDORED").read_text().splitlines():
    fields = line.split()
    if len(fields) >= 2 and fields[1].startswith("sha256:"):
        path, expected = fields[0], fields[1].removeprefix("sha256:")
        actual = hashlib.sha256((root / path).read_bytes()).hexdigest()
        if actual != expected:
            raise SystemExit(f"Vendored source changed: {path}")
        checked += 1
if checked != 5:
    raise SystemExit("Expected five checksum-pinned Link sources")
print(f"Verified {checked} Link v0.3.0 sources")
