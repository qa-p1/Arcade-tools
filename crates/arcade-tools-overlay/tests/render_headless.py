#!/usr/bin/env python3
"""Render/lifecycle smoke test in a private Xvfb session, never the live desktop.

Mirrors Arcade-link/tools/e2e.py's process-scoped HOME/XDG/ARCADE isolation.
Requires Xvfb, xdotool, ImageMagick, libX11. No Python packages are needed.
"""

from __future__ import annotations

import argparse
import ctypes
import ctypes.util
import json
import math
import os
from pathlib import Path
import queue
import re
import shutil
import subprocess
import tempfile
import threading
import time

CRATE = Path(__file__).resolve().parents[1]
REPO = CRATE.parents[1]
FIXTURE = CRATE / "tests/fixtures/shortcuts-index.json"
STRIP = ("WAYLAND_DISPLAY", "WAYLAND_SOCKET", "HYPRLAND_INSTANCE_SIGNATURE", "SWAYSOCK", "DISPLAY",
         "DBUS_SESSION_BUS_ADDRESS", "XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP",
         "DESKTOP_SESSION", "APPIMAGE", "APPDIR", "GTK_THEME")


def isolated_env(root: Path) -> dict[str, str]:
    env = {key: value for key, value in os.environ.items() if key not in STRIP}
    for directory in ("home", "config", "cache", "data", "runtime", "arcade"):
        (root / directory).mkdir()
    (root / "runtime").chmod(0o700)
    env.update(HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "config"),
               XDG_CACHE_HOME=str(root / "cache"), XDG_DATA_HOME=str(root / "data"),
               XDG_RUNTIME_DIR=str(root / "runtime"), ARCADE_HOME=str(root / "arcade"),
               XDG_SESSION_TYPE="x11", GDK_BACKEND="x11", GTK_THEME="Adwaita:dark",
               ARCADE_OVERLAY_TIMING="1", NO_AT_BRIDGE="1")
    return env


class FocusTarget:
    """A real X11 window whose WM_CLASS and active-window property we control."""

    def __init__(self, display: str):
        self.x = ctypes.CDLL(ctypes.util.find_library("X11"))
        d = ctypes.c_void_p
        w = ctypes.c_ulong
        self.x.XOpenDisplay.argtypes = [ctypes.c_char_p]
        self.x.XOpenDisplay.restype = d
        self.x.XDefaultRootWindow.argtypes = [d]
        self.x.XDefaultRootWindow.restype = w
        self.x.XCreateSimpleWindow.argtypes = [d, w, ctypes.c_int, ctypes.c_int,
            ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, w, w]
        self.x.XCreateSimpleWindow.restype = w
        self.x.XInternAtom.argtypes = [d, ctypes.c_char_p, ctypes.c_int]
        self.x.XInternAtom.restype = w
        self.x.XChangeProperty.argtypes = [d, w, w, w, ctypes.c_int, ctypes.c_int,
                                          ctypes.c_void_p, ctypes.c_int]
        self.x.XStoreName.argtypes = [d, w, ctypes.c_char_p]
        self.x.XMapWindow.argtypes = [d, w]
        self.x.XSetInputFocus.argtypes = [d, w, ctypes.c_int, w]
        self.x.XGetInputFocus.argtypes = [d, ctypes.POINTER(w), ctypes.POINTER(ctypes.c_int)]
        self.x.XSync.argtypes = [d, ctypes.c_int]
        self.x.XCloseDisplay.argtypes = [d]
        self.display = self.x.XOpenDisplay(display.encode())
        assert self.display, "cannot open the isolated X display"
        self.root = self.x.XDefaultRootWindow(self.display)
        self.window = self.x.XCreateSimpleWindow(self.display, self.root, 80, 80, 800, 500, 0, 0, 0x282828)
        self.x.XStoreName(self.display, self.window, b"Isolated Firefox focus target")
        klass = b"firefox\0org.mozilla.firefox\0"
        self.x.XChangeProperty(self.display, self.window, self.atom(b"WM_CLASS"), self.atom(b"STRING"), 8, 0, klass, len(klass))
        self.x.XMapWindow(self.display, self.window)
        self.focus()

    def atom(self, name: bytes) -> int:
        return self.x.XInternAtom(self.display, name, 0)

    def focus(self) -> None:
        active = ctypes.c_ulong(self.window)
        self.x.XChangeProperty(self.display, self.root, self.atom(b"_NET_ACTIVE_WINDOW"),
                              self.atom(b"WINDOW"), 32, 0, ctypes.byref(active), 1)
        self.x.XSetInputFocus(self.display, self.window, 1, 0)
        self.x.XSync(self.display, 0)

    def focused(self) -> int:
        window, revert = ctypes.c_ulong(), ctypes.c_int()
        self.x.XGetInputFocus(self.display, ctypes.byref(window), ctypes.byref(revert))
        return window.value

    def unknown(self) -> None:
        self.set_class(b"unknown\0UnknownApp\0")

    def set_class(self, klass: bytes) -> None:
        self.x.XChangeProperty(self.display, self.window, self.atom(b"WM_CLASS"), self.atom(b"STRING"), 8, 0, klass, len(klass))
        self.focus()

    def close(self) -> None:
        self.x.XCloseDisplay(self.display)


def command(env: dict[str, str], *args: str) -> str:
    result = subprocess.run(args, env=env, capture_output=True, text=True, timeout=10)
    assert result.returncode == 0, f"{args!r} exited {result.returncode}: {result.stderr}"
    return result.stdout.strip()


class Render:
    def __init__(self, binary: Path, env: dict[str, str], index: Path | None = FIXTURE):
        args = [str(binary), "--show"]
        if index is not None:
            args += ["--index", str(index)]
        self.started = time.perf_counter()
        self.process = subprocess.Popen(args, env=env, stderr=subprocess.PIPE, stdout=subprocess.DEVNULL, text=True)
        self.lines: list[str] = []
        self.events: queue.Queue = queue.Queue()

        def read() -> None:
            for line in self.process.stderr:
                self.lines.append(line)
                if "first_frame backend=" in line:
                    self.events.put((time.perf_counter(), line))
            self.events.put(None)

        threading.Thread(target=read, daemon=True).start()
        try:
            event = self.events.get(timeout=15)
            assert event, "overlay exited before presenting a frame: " + "".join(self.lines)
            now, line = event
            self.internal = float(re.search(r"spawn_to_first_frame_ms=([\d.]+)", line)[1])
            self.external = (now - self.started) * 1000
            self.window = command(env, "xdotool", "search", "--onlyvisible", "--name", "^Arcade Tools Shortcuts$").splitlines()[0]
            command(env, "xdotool", "windowfocus", "--sync", self.window)
        except BaseException:
            self.stop()
            raise

    def stop(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=3)

    def closed(self) -> None:
        assert self.process.wait(timeout=5) == 0, "".join(self.lines)


def screenshot(env: dict[str, str], path: Path, window: str) -> None:
    # Xvfb has no compositor; allow the queued X presentation to reach the server.
    time.sleep(0.08)
    command(env, "import", "-window", window, str(path))
    assert path.read_bytes().startswith(b"\x89PNG\r\n\x1a\n")


def pixels(path: Path, crop: str = "1100x720+0+0") -> bytes:
    return subprocess.run(["convert", str(path), "-crop", crop, "-depth", "8", "RGBA:-"], check=True, capture_output=True, timeout=10).stdout


def run(binary: Path, samples: int, output: Path) -> None:
    for executable in ("Xvfb", "xdotool", "import", "convert"):
        assert shutil.which(executable), f"missing {executable}"
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="overlay-xvfb-", dir=output) as temporary:
        root = Path(temporary)
        env = isolated_env(root)
        rfd, wfd = os.pipe()
        xvfb = subprocess.Popen(["Xvfb", "-displayfd", str(wfd), "-screen", "0", "1920x1080x24", "-nolisten", "tcp"],
                                pass_fds=(wfd,), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        os.close(wfd)
        with os.fdopen(rfd) as reader:
            env["DISPLAY"] = ":" + reader.readline().strip()
        target = None
        active = None
        results = []
        try:
            target = FocusTarget(env["DISPLAY"])
            for sample in range(samples):
                target.focus()
                active = Render(binary, env)
                results.append({"internalMs": active.internal, "externalMs": active.external})
                if sample == 0:
                    screenshot(env, output / "firefox-dark.png", active.window)
                    (output / "phases.log").write_text("".join(active.lines))
                    command(env, "xdotool", "type", "--clearmodifiers", "--delay", "0", "ctrl+alt+v")
                    screenshot(env, output / "conflict-search.png", active.window)
                    assert pixels(output / "firefox-dark.png") != pixels(output / "conflict-search.png"), "typing did not filter the rendered sheet"
                    command(env, "xdotool", "key", "--clearmodifiers", "ctrl+u", "Tab", "Tab", "Tab", "Next", "Prior", "Down", "Up", "End")
                    screenshot(env, output / "wm-scrolled.png", active.window)
                    command(env, "xdotool", "key", "--clearmodifiers", "shift+Tab")
                command(env, "xdotool", "key", "--clearmodifiers", "Escape")
                active.closed()
                assert target.focused() == target.window, f"Escape did not restore the previous X11 window: actual={target.focused()} expected={target.window}; log={active.lines}"
                active = None
                assert not (root / "arcade/run/tools-overlay/instance.json").exists(), "idle instance metadata left behind"

            # --show on an existing instance is idempotent; --toggle closes it.
            active = Render(binary, env)
            command(env, str(binary), "--show", "--index", str(FIXTURE))
            assert active.process.poll() is None
            command(env, str(binary), "--toggle")
            active.closed()
            active = None
            command(env, str(binary), "--hide")

            # Unidentified windows still retain a focus-restoration target.
            target.set_class(b"")
            active = Render(binary, env)
            command(env, "xdotool", "key", "Escape")
            active.closed()
            assert target.focused() == target.window
            active = None

            # Unknown apps can be picked immediately using only the keyboard.
            target.unknown()
            active = Render(binary, env)
            screenshot(env, output / "app-picker.png", active.window)
            command(env, "xdotool", "type", "--delay", "0", "firefox")
            command(env, "xdotool", "key", "Return")
            screenshot(env, output / "picked-firefox.png", active.window)
            assert pixels(output / "firefox-dark.png", "250x40+20+20") == pixels(output / "picked-firefox.png", "250x40+20+20"), "keyboard picker did not select Firefox"
            command(env, str(binary), "--hide")
            active.closed()
            active = None

            for name, index in [("missing-index", root / "absent.json"), ("corrupt-index", root / "corrupt.json")]:
                if name == "corrupt-index":
                    index.write_text("{invalid")
                active = Render(binary, env, index)
                screenshot(env, output / f"{name}.png", active.window)
                command(env, str(binary), "--hide")
                active.closed()
                active = None

            cache = root / "arcade/cache/arcade/tools/shortcuts-index.json"
            cache.parent.mkdir(parents=True)
            shutil.copyfile(FIXTURE, cache)
            env["GTK_THEME"] = "Adwaita:light"
            active = Render(binary, env, None)
            screenshot(env, output / "all-light.png", active.window)
            # Clicking the thin outside margin closes the desktop window.
            command(env, "xdotool", "mousemove", "--window", active.window, "4", "4", "click", "1")
            active.closed()
            active = None
            summary = {"samples": samples, "binaryBytes": binary.stat().st_size,
                       "internalMedianMs": sorted(row["internalMs"] for row in results)[len(results) // 2],
                       "internalP95Ms": sorted(row["internalMs"] for row in results)[math.ceil(len(results) * .95) - 1],
                       "externalP95Ms": sorted(row["externalMs"] for row in results)[math.ceil(len(results) * .95) - 1],
                       "measurements": results}
            (output / "timings.json").write_text(json.dumps(summary, indent=2) + "\n")
            print(json.dumps({key: value for key, value in summary.items() if key != "measurements"}))
            print(f"Screenshots: {output}")
        finally:
            if active:
                active.stop()
            if target:
                target.close()
            xvfb.terminate()
            xvfb.wait(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=REPO / "target/release/arcade-tools-overlay")
    parser.add_argument("--samples", type=int, default=20)
    parser.add_argument("--output", type=Path, default=REPO / "target/overlay-smoke")
    args = parser.parse_args()
    assert args.samples > 0
    run(args.binary.resolve(), args.samples, args.output.resolve())
