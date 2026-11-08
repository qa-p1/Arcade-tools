#!/usr/bin/env python3
"""Where Tools' window-open memory goes, measured the way the budget gate measures it.

    python3 scripts/open-memory.py ARCADE-LINK/tools/fleet APPIMAGE [--label NAME]

Starts the AppImage in the background in a private profile on the current DISPLAY (as fleet_budget.py does: 30 s to
settle, then `--settings`), and 6 s after the window opens prints one JSON line: PSS per process, and for each process
its largest mappings grouped as glibc thread arenas, [heap], other anonymous memory and files. Then it closes Tools.
Written to explain the gate's two levels of open-window PSS on CI (about 257 and about 300 MB, Tools D18); the memory
probe workflow runs it next to the gate's own command on several runners.
"""
import argparse
import importlib.util
import json
import os
import platform
import subprocess
import tempfile
import time
from pathlib import Path

HEAP_MAX = 0x4000000  # glibc's HEAP_MAX_SIZE on 64-bit: every thread arena's heap starts on a 64 MB boundary


def mappings(pid):
    """(kind, name, PSS kB) for each mapping of the process."""
    out, current = [], None
    try:
        lines = Path(f"/proc/{pid}/smaps").read_text().splitlines()
    except OSError:
        return out
    for line in lines:
        fields = line.split()
        if fields and "-" in fields[0] and not fields[0].endswith(":"):
            start = int(fields[0].split("-")[0], 16)
            name = fields[5] if len(fields) > 5 else ""
            if name == "[heap]":
                kind = "heap"
            elif name.startswith("/"):
                kind = "file"
            elif name:
                kind = "special"
            elif fields[1] == "rw-p" and start % HEAP_MAX == 0:
                kind = "arena"
            else:
                kind = "anon"
            current = [kind, name or fields[0], 0]
            out.append(current)
        elif current and fields[:1] == ["Pss:"]:
            current[2] = int(fields[1])
    return out


def describe(pid):
    rows = mappings(pid)
    kinds = {}
    for kind, _, kb in rows:
        kinds[kind] = kinds.get(kind, 0) + kb
    top = sorted(rows, key=lambda r: -r[2])[:8]
    return {
        "pss": round(sum(kinds.values()) / 1024, 1),
        "by_kind": {k: round(v / 1024, 1) for k, v in sorted(kinds.items(), key=lambda kv: -kv[1])},
        "arenas": sum(1 for kind, _, kb in rows if kind == "arena" and kb),
        "top": [[kind, Path(name).name if kind == "file" else name, round(kb / 1024, 1)] for kind, name, kb in top],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("fleet", type=Path, help="Arcade-Link's tools/fleet directory (for its process helpers)")
    parser.add_argument("appimage", type=Path)
    parser.add_argument("--label", default="")
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location("fleet_budget", args.fleet / "fleet_budget.py")
    fleet = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fleet)
    image = str(args.appimage.resolve())

    root = Path(tempfile.mkdtemp(prefix="arcade-open-memory-"))
    run = tempfile.mkdtemp(prefix="aom-")
    os.chmod(run, 0o700)
    for name in ("home", "config", "data", "cache", "state"):
        (root / name).mkdir()
    env = {k: v for k, v in os.environ.items() if not k.startswith("ARCADE_") and k not in (
        "APPIMAGE", "APPDIR", "LD_LIBRARY_PATH", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS")}
    env.update(HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"),
               XDG_CACHE_HOME=str(root / "cache"), XDG_STATE_HOME=str(root / "state"), XDG_RUNTIME_DIR=run,
               GDK_BACKEND="x11", ARCADE_NO_INSTALL_PROMPT="1")
    bus = subprocess.run(["dbus-daemon", "--session", "--fork", "--print-address=1", "--print-pid=1"], env=env,
                         capture_output=True, text=True, check=True).stdout.split("\n")
    env["DBUS_SESSION_BUS_ADDRESS"] = bus[0]
    launcher = subprocess.Popen([image, "--background"], env=env, cwd=env["HOME"], stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL, start_new_session=True)

    def processes():
        family = fleet.tree([launcher.pid] + fleet.unpacked("tools", env["HOME"], env["XDG_DATA_HOME"]))
        return [pid for pid in family if Path(f"/proc/{pid}").exists()]

    try:
        time.sleep(32)
        subprocess.run([image, "--settings"], env=env, timeout=30, capture_output=True)
        time.sleep(6)
        report = {}
        for pid in processes():
            try:
                name = Path(f"/proc/{pid}/comm").read_text().strip()
            except OSError:
                continue
            report[f"{name}[{pid}]"] = describe(pid)
        cpu = next((line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines()
                    if line.startswith("model name")), platform.processor())
        print(json.dumps({
            "label": args.label,
            "malloc_arena_max": os.environ.get("MALLOC_ARENA_MAX"),
            "cpus": os.cpu_count(),
            "cpu": cpu,
            "total_pss": round(sum(p["pss"] for p in report.values()), 1),
            "processes": report,
        }), flush=True)
    finally:
        for pid in processes():
            try:
                os.kill(pid, 15)
            except OSError:
                pass
        try:
            os.kill(int(bus[1]), 15)
        except (OSError, ValueError):
            pass


if __name__ == "__main__":
    main()
