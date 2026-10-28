#!/usr/bin/env python3
"""One version for Arcade Tools.

The source is `[workspace.package] version` in the root Cargo.toml; both
Cargo packages inherit it. package.json, package-lock.json and
src-tauri/tauri.conf.json repeat it for npm and Tauri, and this check fails
when any of them drifts.

  python3 scripts/check-versions.py            # fail on drift
  python3 scripts/check-versions.py --print    # print the version
  python3 scripts/check-versions.py --set 1.2.3
"""
import argparse
import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CARGO = ROOT / 'Cargo.toml'
PACKAGES = [ROOT / 'Cargo.toml', ROOT / 'src-tauri' / 'Cargo.toml']
SEMVER = re.compile(r'^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$')


def source() -> str:
    return tomllib.loads(CARGO.read_text(encoding='utf-8'))['workspace']['package']['version']


def copies() -> dict[str, str]:
    package = json.loads((ROOT / 'package.json').read_text(encoding='utf-8'))
    lock = json.loads((ROOT / 'package-lock.json').read_text(encoding='utf-8'))
    tauri = json.loads((ROOT / 'src-tauri' / 'tauri.conf.json').read_text(encoding='utf-8'))
    return {
        'package.json': package.get('version'),
        'package-lock.json': lock.get('version'),
        'package-lock.json packages[""]': lock.get('packages', {}).get('', {}).get('version'),
        'src-tauri/tauri.conf.json': tauri.get('version'),
    }


def problems() -> list[str]:
    version = source()
    found = []
    if not SEMVER.match(version):
        found.append(f'Cargo.toml [workspace.package] version {version!r} is not a semver version')
    for path in PACKAGES:
        inherited = tomllib.loads(path.read_text(encoding='utf-8'))['package'].get('version')
        if inherited != {'workspace': True}:
            found.append(f'{path.relative_to(ROOT).as_posix()}: use version.workspace = true, not {inherited!r}')
    for name, value in copies().items():
        if value != version:
            found.append(f'{name} says {value!r}; Cargo.toml [workspace.package] says {version!r}')
    return found


def replace_json_version(path: Path, version: str, count: int) -> None:
    text = path.read_text(encoding='utf-8')
    # Only the first top-level occurrences, keeping each file's own layout.
    text, done = re.subn(r'("version":\s*")[^"]*(")', rf'\g<1>{version}\g<2>', text, count=count)
    if done != count:
        sys.exit(f'{path.relative_to(ROOT)}: expected {count} version field(s), found {done}')
    path.write_text(text, encoding='utf-8')


def set_version(version: str) -> None:
    if not SEMVER.match(version):
        sys.exit(f'{version!r} is not a semver version')
    text = CARGO.read_text(encoding='utf-8')
    text, done = re.subn(r'(?m)(^\[workspace\.package\]\n(?:[^\[\n].*\n)*?version\s*=\s*")[^"]*(")',
                         rf'\g<1>{version}\g<2>', text, count=1)
    if done != 1:
        sys.exit('Cargo.toml: no [workspace.package] version')
    CARGO.write_text(text, encoding='utf-8')
    replace_json_version(ROOT / 'package.json', version, 1)
    replace_json_version(ROOT / 'package-lock.json', version, 2)
    replace_json_version(ROOT / 'src-tauri' / 'tauri.conf.json', version, 1)
    print(f'Set {version}. Run cargo update -w to refresh Cargo.lock.')


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    group = parser.add_mutually_exclusive_group()
    group.add_argument('--print', action='store_true', help='print the version')
    group.add_argument('--set', metavar='VERSION', help='write VERSION to every copy')
    args = parser.parse_args()
    if args.set:
        set_version(args.set)
    found = problems()
    if found:
        print('Version drift:\n  ' + '\n  '.join(found), file=sys.stderr)
        return 1
    if args.print:
        print(source())
    elif not args.set:
        print(f'Version {source()} everywhere.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
