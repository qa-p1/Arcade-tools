#!/usr/bin/env python3
"""Measure eagerly loaded shell assets; report lazy docs/map bytes separately."""
import gzip
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1] / 'dist'
manifest_path = root / '.vite/manifest.json'
if not manifest_path.is_file():
    raise SystemExit('Run npm run build first.')
manifest = json.loads(manifest_path.read_text())
files = {root / 'index.html'}
seen = set()
def include(key):
    if key in seen:
        return
    seen.add(key)
    entry = manifest[key]
    files.add(root / entry['file'])
    files.update(root / path for path in entry.get('css', []))
    for imported in entry.get('imports', []):
        include(imported)
for key, entry in manifest.items():
    if entry.get('isEntry'):
        include(key)
if len(files) < 2:
    raise SystemExit('No built shell entry found.')
raw = sum(path.stat().st_size for path in files)
compressed = sum(len(gzip.compress(path.read_bytes(), mtime=0)) for path in files)
all_assets = {path for path in root.rglob('*') if path.suffix in ('.html','.js','.css')}
deferred = sum(path.stat().st_size for path in all_assets - files)
print(f'UI shell: {raw} bytes raw, {compressed} bytes gzip; budget 204800 bytes; deferred guides/map: {deferred} bytes')
if raw > 204800:
    raise SystemExit('UI shell exceeds the 200 KB budget.')
