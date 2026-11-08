#!/usr/bin/env python3
"""Upgrade from a published Tools release to a new one, keeping the user's data (PLAN exit gate "Upgrade test").

    dbus-run-session -- xvfb-run -a python3 scripts/verify-upgrade.py OLD.AppImage NEW.AppImage

OLD is the previous release's published file (check it against its SHA256SUMS.txt first), NEW the rc's. In a
private profile: OLD is installed from a download with Start at login, and it runs with the settings, update state
and history a user of it has (written in its own file shapes, then read and rewritten by OLD itself). NEW is then
downloaded and installed over it while OLD runs, the way a user updates by hand. NEW must replace the installed
copy in place, keep every setting, the update state and the history, keep Start at login, move the launcher, command
and login entry to itself, and stop on --quit. Tools' own updater path from OLD is checked after promotion, when
OLD can see the release (PLAN Release §4).
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

PREFERENCES = {"linkEnabled": True, "channels": {"arcade.box": "nightly"},
               "updates": {"enabled": False, "autoUpdate": {"arcade.look": True}, "rolledBack": {"arcade.look": "0.3.1"}},
               "notificationsEnabled": False, "theme": "dark", "onboardingComplete": True}
TRAY = {"hostIcons": False, "excluded": ["arcade.find"], "showUpdateBadge": False, "startAtLogin": True,
        "shortcutsEnabled": True, "shortcut": "Ctrl+Alt+Slash"}
UPDATES = {"schema": 1, "lastCheck": 1760000000, "lastSuccess": 1760000000, "lastFailure": None,
           "apps": {"arcade.look": {"installedVersion": "0.3.1", "channel": "stable",
                                    "latest": {"stable": {"latestVersion": "0.4.0", "updateAvailable": True,
                                                          "heldBack": False, "reason": None, "error": None,
                                                          "cache": None}},
                                    "updateAvailable": True, "heldBack": False, "reason": None,
                                    "notified": ["0.4.0"]}}}
HISTORY = [{"time": 1760000000, "app": "arcade.look", "from": "0.3.0", "to": "0.3.1", "result": "success"},
           {"time": 1760000100, "app": "arcade.look", "from": "0.3.1", "to": "0.4.0", "result": "rolled_back",
            "reason": "failed to start"}]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def alive(pid):
    """Running, not a zombie (OLD without U1 is this script's own child until reaped)."""
    try:
        return Path(f"/proc/{pid}/stat").read_text().rpartition(") ")[2].split()[0] != "Z"
    except OSError:
        return False


def subset(expected, actual, where):
    """Every key and value of `expected` is in `actual` (a newer Tools may add fields)."""
    if isinstance(expected, dict):
        assert isinstance(actual, dict), f"{where}: {actual!r} is not an object"
        for key, value in expected.items():
            assert key in actual, f"{where}.{key} is missing: {actual}"
            subset(value, actual[key], f"{where}.{key}")
    else:
        assert expected == actual, f"{where}: {actual!r}, expected {expected!r}"


def main():
    old_image, new_image = (Path(arg).resolve(strict=True) for arg in sys.argv[1:3])
    with tempfile.TemporaryDirectory(prefix="tools-upgrade-") as directory:
        root = Path(directory)
        profile = root / "profile"
        env = {k: v for k, v in os.environ.items() if not k.startswith("ARCADE_") and k not in (
            "APPIMAGE", "APPDIR", "LD_LIBRARY_PATH", "WAYLAND_DISPLAY", "HYPRLAND_INSTANCE_SIGNATURE", "SWAYSOCK")}
        env.update(ARCADE_HOME=str(profile), HOME=str(root / "home"), XDG_CONFIG_HOME=str(profile / ".config"),
                   XDG_DATA_HOME=str(profile / ".local/share"), XDG_CACHE_HOME=str(profile / ".cache"),
                   ARCADE_NO_INSTALL_PROMPT="1", GDK_BACKEND="x11")
        (root / "home").mkdir()
        downloads = root / "home/Downloads"
        downloads.mkdir()
        state = profile / ".local/share/arcade/tools"
        endpoint = profile / "run/arcade.tools.endpoint"
        log = (root / "tools.log").open("w+b")
        launched = []

        def install(image):
            download = downloads / image.name
            shutil.copy2(image, download)
            download.chmod(0o755)
            result = subprocess.run([str(download), "--install", "--silent", "--no-launch", "--start-at-login"],
                                    env=env, capture_output=True, text=True, timeout=120)
            assert result.returncode == 0, f"{image.name} --install exited {result.returncode}: {result.stderr[-2000:]}"
            return json.loads(result.stdout)

        def resident():
            try:
                pid = json.loads(endpoint.read_text())["pid"]
            except (OSError, ValueError, KeyError):
                return None
            return pid if alive(pid) else None

        def wait(probe, what, seconds=30):
            end = time.monotonic() + seconds
            while time.monotonic() < end:
                if result := probe():
                    return result
                time.sleep(0.1)
            raise AssertionError(f"Timed out waiting for {what}")

        def start(executable):
            launched.append(subprocess.Popen([executable, "--background"], env=env, stdout=log,
                                             stderr=subprocess.STDOUT, start_new_session=True))
            return wait(resident, f"{executable} to start")

        def quit(executable):
            pid = resident()
            subprocess.run([executable, "--quit"], env=env, capture_output=True, timeout=30)
            if pid:
                wait(lambda: [process.poll() for process in launched] and not alive(pid), "--quit to stop the resident")

        try:
            old = install(old_image)
            installed = Path(old["path"])
            assert installed.is_file() and digest(installed) == digest(old_image), old
            # The user's data, in the shapes OLD writes; OLD reads it and rewrites what it owns.
            state.mkdir(parents=True, exist_ok=True)
            (state / "preferences.json").write_text(json.dumps(PREFERENCES))
            (state / "tray-preferences.json").write_text(json.dumps(TRAY))
            (state / "updates.json").write_text(json.dumps(UPDATES))
            (state / "update-history.jsonl").write_text("".join(json.dumps(line) + "\n" for line in HISTORY))
            old_pid = start(str(installed))
            time.sleep(5)
            subset(PREFERENCES, json.loads((state / "preferences.json").read_text()), f"{old['version']} preferences")
            subset(TRAY, json.loads((state / "tray-preferences.json").read_text()), f"{old['version']} tray")
            print(f"{old['version']} installed at {installed} and running (PID {old_pid}) with the user's data", flush=True)

            # Update by hand while OLD runs.
            new = install(new_image)
            # A dry run of this script against a local build of the same version sets VERIFY_UPGRADE_SAME_VERSION=1.
            assert new["version"] != old["version"] or os.environ.get("VERIFY_UPGRADE_SAME_VERSION") == "1", new
            assert Path(new["path"]) == installed, f"NEW installed at {new['path']}, not over {installed}"
            assert digest(installed) == digest(new_image), "The installed copy is not NEW's bytes"
            if resident():
                quit(str(installed))
            new_pid = start(str(installed))
            time.sleep(5)

            subset(PREFERENCES, json.loads((state / "preferences.json").read_text()), f"{new['version']} preferences")
            subset(TRAY, json.loads((state / "tray-preferences.json").read_text()), f"{new['version']} tray")
            updates = json.loads((state / "updates.json").read_text())
            look = updates["apps"]["arcade.look"]
            assert look["installedVersion"] == "0.3.1" and "0.4.0" in look["notified"], look
            history = [json.loads(line) for line in (state / "update-history.jsonl").read_text().splitlines() if line]
            assert history[:len(HISTORY)] == HISTORY, history

            status = json.loads(subprocess.run([str(installed), "--integration-status"], env=env, capture_output=True,
                                               text=True, check=True, timeout=30).stdout)
            receipt = status["receipt"]
            assert receipt["version"] == new["version"] and Path(receipt["path"]) == installed, receipt
            missing = [entry["path"] for entry in status["entries"] if not entry["exists"]]
            assert not missing, f"Integration entries missing after the update: {missing}"
            integration = receipt["integration"]
            login = Path(integration["autostart"])
            assert login.is_file() and "Hidden=true" not in login.read_text(), "Start at login was not kept"
            assert str(installed) in login.read_text(), login.read_text()
            cli = Path(integration["cli"])
            assert cli.resolve() == installed, f"{cli} → {cli.resolve()}"
            desktop = Path(integration["desktopEntry"])
            assert desktop.is_file() and str(installed) in desktop.read_text(), desktop
            stale = [str(entry) for entry in Path(old["integration"]["desktopEntry"]).parent.glob("*.desktop")
                     if entry != desktop and "Arcade Tools" in entry.read_text()]
            assert not stale, f"The old launcher is still listed: {stale}"
            print(f"{new['version']} replaced it in place (PID {new_pid}): settings, tray choices, update state, "
                  f"history and Start at login kept; launcher, command and login entry are NEW's", flush=True)
            quit(str(installed))
        except Exception:
            log.seek(0)
            print(log.read().decode(errors="replace")[-6000:], file=sys.stderr)
            raise
        finally:
            if pid := resident():
                os.kill(pid, 9)
            for process in launched:
                if process.poll() is None:
                    process.kill()
        log.seek(0)
        assert b"panicked at" not in log.read(), "Tools logged a panic"
    print(f"PASS: {old['version']} → {new['version']} keeps the user's data and integration")


if __name__ == "__main__":
    main()
