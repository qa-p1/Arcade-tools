#!/usr/bin/env python3
"""Build the native companion for Tauri's externalBin bundle configuration."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    target = args.target
    allowed = {"x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc", "aarch64-apple-darwin", "x86_64-apple-darwin", "universal-apple-darwin"}
    if target not in allowed:
        parser.error("Unsupported package target")
    environment = dict(os.environ, CARGO_BUILD_JOBS="3")
    targets = ["aarch64-apple-darwin", "x86_64-apple-darwin"] if target == "universal-apple-darwin" else [target]
    suffix = ".exe" if "windows" in target else ""
    for architecture in targets:
        subprocess.run(["cargo", "build", "--release", "--locked", "-p", "arcade-tools-overlay", "--target", architecture], cwd=root, env=environment, check=True)
    destination = root / "src-tauri/binaries" / f"arcade-tools-overlay-{target}{suffix}"
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.is_symlink():
        raise SystemExit("The generated companion destination is a symlink")
    sources = [root / "target" / architecture / "release" / f"arcade-tools-overlay{suffix}" for architecture in targets]
    if len(sources) == 2:
        subprocess.run(["lipo", "-create", *map(str, sources), "-output", str(destination)], check=True)
    else:
        shutil.copy2(sources[0], destination)
    print(f"Prepared {destination.relative_to(root)}")


if __name__ == "__main__":
    main()
