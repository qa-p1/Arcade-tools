#!/usr/bin/env python3
"""Optional layer-shell smoke test using an isolated headless Sway compositor.

Requires sway + grim. No requests are sent to the user's compositor. Runtime
socket paths stay short inside target/w; generated config dies with the session.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import queue
import shutil
import subprocess
import tempfile
import threading
import time

import render_headless as smoke


def run(binary: Path, output: Path, samples: int, pointer: Path | None) -> None:
    for executable in ("sway", "grim", "timeout"):
        assert shutil.which(executable), f"missing {executable}"
    output.mkdir(parents=True, exist_ok=True)
    short = smoke.REPO / "target/w"
    short.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="s-", dir=short) as temporary:
        root = Path(temporary)
        env = smoke.isolated_env(root)
        env["XDG_RUNTIME_DIR"] = str(root)
        env.update(WLR_BACKENDS="headless", WLR_LIBINPUT_NO_DEVICES="1", WLR_RENDERER="pixman", SWAYSOCK=str(root / "s.sock"))
        config = root / "sway.conf"
        config.write_text("output HEADLESS-1 resolution 1920x1080\nseat seat0 fallback true\nxwayland disable\n")
        active = None
        with (output / "sway.log").open("w") as log:
            compositor = subprocess.Popen(["sway", "--unsupported-gpu", "--config", str(config)], env=env, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 8
                sockets, ipc = [], []
                while time.monotonic() < deadline:
                    sockets = [p for p in root.glob("wayland-*") if p.suffix != ".lock"]
                    ipc = [root / "s.sock"] if (root / "s.sock").exists() else []
                    if sockets and ipc:
                        break
                    assert compositor.poll() is None, (output / "sway.log").read_text()
                    time.sleep(.05)
                assert sockets and ipc, "headless compositor not ready"
                env.update(WAYLAND_DISPLAY=sockets[0].name, SWAYSOCK=str(ipc[0]), XDG_SESSION_TYPE="wayland")
                results = []
                for sample in range(samples):
                    started = time.perf_counter()
                    active = subprocess.Popen([str(binary), "--show", "--index", str(smoke.FIXTURE)], env=env, stderr=subprocess.PIPE, text=True)
                    events: queue.Queue = queue.Queue()
                    lines: list[str] = []

                    def read(process=active, events=events, lines=lines) -> None:
                        for line in process.stderr:
                            lines.append(line)
                            if "first_frame" in line:
                                events.put((time.perf_counter(), line))
                        events.put(None)

                    threading.Thread(target=read, daemon=True).start()
                    event = events.get(timeout=10)
                    assert event, "".join(lines)
                    now, line = event
                    assert "backend=wayland-layer" in line, line
                    results.append({"internalMs": float(line.split("spawn_to_first_frame_ms=")[1]), "externalMs": (now - started) * 1000})
                    if sample == 0:
                        (output / "phases.log").write_text("".join(lines))
                        smoke.command(env, "timeout", "5", "grim", str(output / "layer.png"))
                    smoke.command(env, str(binary), "--hide")
                    assert active.wait(timeout=5) == 0
                    active = None
                # Fractional scaling exercises the viewport path as well.
                smoke.command(env, "swaymsg", "-s", str(ipc[0]), "output", "HEADLESS-1", "scale", "1.5")
                active = subprocess.Popen([str(binary), "--show", "--index", str(smoke.FIXTURE)], env=env, stderr=subprocess.PIPE, text=True)
                ready: queue.Queue = queue.Queue()

                def read_scaled() -> None:
                    for line in active.stderr:
                        if "first_frame" in line:
                            ready.put(line)
                            return
                    ready.put(None)

                threading.Thread(target=read_scaled, daemon=True).start()
                assert ready.get(timeout=10)
                smoke.command(env, "timeout", "5", "grim", str(output / "layer-150-percent.png"))
                if pointer:
                    smoke.command(env, str(pointer))
                else:
                    smoke.command(env, str(binary), "--toggle")
                assert active.wait(timeout=5) == 0
                active = None
                (output / "timings.json").write_text(json.dumps(results, indent=2) + "\n")
                print(json.dumps({"samples": samples, "internalMaxMs": max(r["internalMs"] for r in results), "externalMaxMs": max(r["externalMs"] for r in results)}))
                print(f"Screenshots: {output}")
            finally:
                if active and active.poll() is None:
                    active.terminate()
                    active.wait(timeout=5)
                compositor.terminate()
                compositor.wait(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=smoke.REPO / "target/release/arcade-tools-overlay")
    parser.add_argument("--output", type=Path, default=smoke.REPO / "target/overlay-wayland")
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--pointer", type=Path, help="Path to the overlay-pointer example; checks outside-click close")
    args = parser.parse_args()
    assert args.samples > 0
    run(args.binary.resolve(), args.output.resolve(), args.samples, args.pointer.resolve() if args.pointer else None)
