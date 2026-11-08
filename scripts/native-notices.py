#!/usr/bin/env python3
"""Record the Ubuntu packages whose files the Linux AppImage bundles, for THIRD_PARTY_NOTICES.md.

  scripts/native-notices.py Arcade-Tools.AppImage           # rewrite packaging/linux/native/
  scripts/native-notices.py --check Arcade-Tools.AppImage   # fail if a bundled package is not recorded

Runs on the build host (Ubuntu 22.04 in CI and Release), where linuxdeploy copied the libraries from. Every file in
the image is mapped to the Debian package that installed it (`dpkg -S`); each package's `copyright` file is kept
under packaging/linux/native/copyright/, and packages.json names its source package. Versions are not recorded, so
an Ubuntu security update does not change the result. An executable or library that maps to no package and is not
Tools' own fails the run.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
NATIVE = ROOT / "packaging" / "linux" / "native"
OWN = ("usr/bin/arcade-tools", "usr/bin/arcade-tools-overlay", "usr/lib/libarcade-trim.so",
       # linuxdeploy's AppRun; it and the AppImage runtime are listed by scripts/notices.py (Packaging).
       "AppRun.wrapped")
LIB_DIRS = ("/usr/lib/x86_64-linux-gnu", "/lib/x86_64-linux-gnu", "/usr/lib", "/lib")


def is_elf(path):
    try:
        with open(path, "rb") as f:
            return f.read(4) == b"\x7fELF"
    except OSError:
        return False


def candidates(relative):
    """Host paths a bundled file may have come from: the same path, or the same path under a system library
    folder (linuxdeploy flattens libraries into usr/lib and drops the multiarch folder from GTK's modules)."""
    out = ["/" + relative]
    if relative.startswith("usr/lib/"):
        rest = relative[len("usr/lib/"):]
        out += [f"{d}/{rest}" for d in LIB_DIRS]
    seen, paths = set(), []
    for p in out:
        for q in (p, os.path.realpath(p)):
            if q not in seen and os.path.lexists(q):
                seen.add(q)
                paths.append(q)
    return paths


def owners(paths):
    """dpkg -S for many paths at once: {path: package}."""
    found = {}
    for i in range(0, len(paths), 200):
        batch = paths[i:i + 200]
        run = subprocess.run(["dpkg-query", "-S", *batch], capture_output=True, text=True)
        for line in run.stdout.splitlines():
            names, _, path = line.partition(": ")
            if path in batch and "diversion" not in names:
                found[path] = names.split(",")[0].strip()
    return found


def package_info(package):
    bare = package.split(":")[0]
    source = subprocess.run(["dpkg-query", "-W", "-f", "${source:Package}", package],
                            check=True, capture_output=True, text=True).stdout.strip() or bare
    copyright_file = Path("/usr/share/doc") / bare / "copyright"
    if not copyright_file.is_file():
        sys.exit(f"native-notices: {package} has no {copyright_file}")
    return bare, source, copyright_file.read_text(encoding="utf-8", errors="replace")


def scan(appimage):
    with tempfile.TemporaryDirectory() as work:
        image = Path(work) / "image.AppImage"
        shutil.copy2(appimage, image)
        image.chmod(0o755)
        subprocess.run([str(image), "--appimage-extract"], cwd=work, check=True, stdout=subprocess.DEVNULL,
                       env=dict(os.environ, APPIMAGE_EXTRACT_AND_RUN=""))
        root = Path(work) / "squashfs-root"
        files = {}
        for path in root.rglob("*"):
            if path.is_file() and not path.is_symlink():
                relative = path.relative_to(root).as_posix()
                files[relative] = is_elf(path)
        wanted = {rel: candidates(rel) for rel in files}
        mapped = owners(sorted({p for paths in wanted.values() for p in paths}))
        packages, unowned = set(), []
        for rel, elf in sorted(files.items()):
            owner = next((mapped[p] for p in wanted[rel] if p in mapped), None)
            if owner:
                packages.add(owner)
            elif elf and rel not in OWN:
                unowned.append(rel)
        if unowned:
            sys.exit("native-notices: bundled programs or libraries from no package:\n  " + "\n  ".join(unowned))
        return packages


def write(packages, into):
    (into / "copyright").mkdir(parents=True, exist_ok=True)
    record = {}
    for package in sorted(packages):
        bare, source, text = package_info(package)
        record[bare] = {"source": source}
        (into / "copyright" / f"{bare}.txt").write_text(text, encoding="utf-8")
    (into / "packages.json").write_text(json.dumps({"host": "Ubuntu 22.04", "packages": record}, indent=2) + "\n",
                                        encoding="utf-8")


def snapshot(folder):
    if not folder.is_dir():
        return {}
    return {p.relative_to(folder).as_posix(): p.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n")
            for p in sorted(folder.rglob("*")) if p.is_file()}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("appimage", type=Path)
    ap.add_argument("--check", action="store_true", help="fail when packaging/linux/native/ is out of date")
    args = ap.parse_args()
    packages = scan(args.appimage.resolve())
    if not args.check:
        shutil.rmtree(NATIVE, ignore_errors=True)
        write(packages, NATIVE)
        print(f"Recorded {len(packages)} bundled packages in {NATIVE.relative_to(ROOT)}; now run scripts/notices.py")
        return
    with tempfile.TemporaryDirectory() as fresh:
        write(packages, Path(fresh))
        now, committed = snapshot(Path(fresh)), snapshot(NATIVE)
        if now != committed:
            added = sorted(set(now) - set(committed))
            removed = sorted(set(committed) - set(now))
            changed = sorted(k for k in set(now) & set(committed) if now[k] != committed[k])
            shutil.rmtree(NATIVE, ignore_errors=True)
            shutil.copytree(fresh, NATIVE)
            sys.exit("packaging/linux/native/ is out of date (the regenerated copy is in the tree):\n"
                     f"  added {added}\n  removed {removed}\n  changed {changed}\n"
                     "Run scripts/native-notices.py on the AppImage, then scripts/notices.py, and commit.")
    print(f"packaging/linux/native/ is current: {len(packages)} bundled packages")


if __name__ == "__main__":
    main()
