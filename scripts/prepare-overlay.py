#!/usr/bin/env python3
"""Build what the package bundles besides Tools itself: the overlay companion and the Arcade helper (Tauri's
externalBin; the helper is Link's `arcade-helper`, built from this workspace so its lockfile covers it) and, on
Linux, libarcade-trim.so, which trims WebKit's helper processes (OPEN1; tauri.bundle.conf.json puts it in the
AppImage's usr/lib)."""
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
    # (package, program name): each becomes binaries/<program>-<target>, which Tauri puts beside Tools.
    companions = [("arcade-tools-overlay", "arcade-tools-overlay"), ("arcade-tools-helper", "arcade-helper")]
    for architecture in targets:
        packages = [arg for package, _ in companions for arg in ("-p", package)]
        subprocess.run(["cargo", "build", "--release", "--locked", *packages, "--target", architecture], cwd=root, env=environment, check=True)
    for _, program in companions:
        destination = root / "src-tauri/binaries" / f"{program}-{target}{suffix}"
        destination.parent.mkdir(parents=True, exist_ok=True)
        if destination.is_symlink():
            raise SystemExit("The generated companion destination is a symlink")
        sources = [root / "target" / architecture / "release" / f"{program}{suffix}" for architecture in targets]
        if len(sources) == 2:
            subprocess.run(["lipo", "-create", *map(str, sources), "-output", str(destination)], check=True)
            # Tauri's universal build compiles each architecture separately and
            # checks externalBin for that architecture's triple as well.
            for architecture, source in zip(targets, sources):
                single = destination.with_name(f"{program}-{architecture}")
                if single.is_symlink():
                    raise SystemExit("The generated companion destination is a symlink")
                shutil.copy2(source, single)
                print(f"Prepared {single.relative_to(root)}")
        else:
            shutil.copy2(sources[0], destination)
        print(f"Prepared {destination.relative_to(root)}")
    if "linux" in target:
        trimmer = root / "src-tauri/binaries/libarcade-trim.so"
        if trimmer.is_symlink():
            raise SystemExit("The generated trimmer destination is a symlink")
        linux = root / "packaging/linux"
        subprocess.run(["cc", "-O2", "-shared", "-fPIC", "-Wall", "-Werror", "-o", str(trimmer),
                        str(linux / "arcade-trim-preload.c"), f"-I{linux}", "-lpthread"], check=True)
        print(f"Prepared {trimmer.relative_to(root)}")


if __name__ == "__main__":
    main()
