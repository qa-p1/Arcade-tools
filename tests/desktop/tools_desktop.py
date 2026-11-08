#!/usr/bin/env python3
"""Tier 1 (headless Hyprland, Link's desktop harness) for Arcade Tools.

    tools_desktop.py <AppImage>

In a private profile: the tray item and its menu, the window and its identity (ARCADE_APP.md ID-1), Escape on a
top-level page (UI-2), a second start reusing the running copy, the shortcuts overlay surviving the pointer moving
into another window (UI-1, TL-2) and closing on Escape, and Quit from the tray. Exit 10 means a fleet rule is broken;
any other failure is the harness's or the app's.
"""
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, ".arcade-link/tools/desktop")
from arcade_desktop import Desktop  # noqa: E402

RULE_BROKEN = 10
app = sys.argv[1]
d = Desktop()
home = tempfile.mkdtemp(prefix="tools-desktop-")
env = {**os.environ, "HOME": home, "XDG_CONFIG_HOME": f"{home}/.config", "XDG_DATA_HOME": f"{home}/.local/share",
       "XDG_CACHE_HOME": f"{home}/.cache", "ARCADE_HOME": f"{home}/arcade-link"}
log = open(d.artifact("tools.log"), "wb")


def run(*args, timeout=60):
    return subprocess.run([app, *args], env=env, stdout=log, stderr=subprocess.STDOUT, timeout=timeout)


def broken(rule, message):
    d.screenshot(f"broken-{rule}")
    print(f"RULE BROKEN {rule}: {message}")
    sys.exit(RULE_BROKEN)


def check(ok, message):
    if not ok:
        d.screenshot("failed")
        raise SystemExit(f"FAILED: {message}")


run("--install", "--silent", "--no-launch", timeout=120)
resident = subprocess.Popen([app, "--background"], env=env, stdout=log, stderr=subprocess.STDOUT)

def tray_labels():
    try:
        return [label for _, label in d.tray_menu("Arcade Tools") if label]
    except Exception:  # not registered yet, or gone
        return []


# Tray (ARCADE_APP.md UI-3 for Tools' own hub menu): opens Tools first, quits it last.
d.wait(lambda: "Open Arcade Tools" in tray_labels(), 60, "the Tools tray menu")
labels = tray_labels()
print("tray menu:", labels)
check(labels[0] == "Open Arcade Tools" and labels[-1] == "Quit Arcade Tools", f"tray menu order: {labels}")

# Window and identity.
d.tray_click("Arcade Tools", "Open Arcade Tools")
window = d.wait_window(title="Arcade Tools", timeout=60)
time.sleep(4)  # first paint under software GL
d.screenshot("window")
if window.get("class") != "dev.arcade.Tools" and window.get("initialClass") != "dev.arcade.Tools":
    broken("ID-1", f"window class {window.get('class')!r}, expected dev.arcade.Tools")

# Escape on a top-level page keeps the window (UI-2: only an empty top view closes).
d.focus(window["address"])
d.escape(window["address"])
time.sleep(1.5)
if d.find_window(cls="dev.arcade.Tools") is None:
    broken("UI-2", "Escape on Home closed the window")

# A second start hands over to the running copy: still one window, and the second process exits.
second = run("--settings", timeout=30)
check(second.returncode == 0, f"second --settings exited {second.returncode}")
time.sleep(2)
windows = [c for c in d.clients() if c.get("class") == "dev.arcade.Tools"]
check(len(windows) == 1, f"{len(windows)} Tools windows after a second start")

# The shortcuts overlay is a popup: the pointer leaving it for another window must not close it (UI-1, TL-2).
other = subprocess.Popen(["foot", "--app-id", "harness.other", "--title", "other window"])
other_w = d.wait_window(cls="harness.other")
overlay_proc = subprocess.Popen([app, "--show-shortcuts"], env=env, stdout=log, stderr=subprocess.STDOUT)


def overlay():
    return d.find_window(title="Arcade Tools Shortcuts") or d.find_layer("arcade-tools-overlay")


shown = d.wait(overlay, 30, "the shortcuts overlay")
time.sleep(2)
d.screenshot("overlay")
if "address" in shown:
    d.pointer_move(*d.center_of(shown))
    time.sleep(0.5)
d.pointer_move(*d.center_of(other_w))
time.sleep(3)
if overlay() is None:
    broken("UI-1", "the shortcuts overlay closed when the pointer moved into another window")
d.escape(shown.get("address"))
d.wait(lambda: overlay() is None, 10, "Escape to close the overlay")
overlay_proc.wait(timeout=15)
other.terminate()

# Quit from the tray closes the window and removes the tray item. The process started above has already handed
# over to the unpacked copy (U1), so the tray item is what shows the resident copy is gone.
d.tray_click("Arcade Tools", "Quit Arcade Tools")
d.wait_gone(cls="dev.arcade.Tools", timeout=20)
d.wait(lambda: not tray_labels(), 30, "the tray item to go away")
resident.wait(timeout=30)
dumps = list(Path(home).rglob("core*"))
check(not dumps, f"core dumps: {dumps}")
print("PASS: tray, window identity, Escape, single instance, overlay popup rule, quit")
