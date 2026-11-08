#!/usr/bin/env python3
"""Tools updates itself from one published release to the next, with a delta (PLAN exit gate "a delta update ...
works through Tools"; the Gates workflow runs it on published rc files).

    dbus-run-session -- xvfb-run -a python3 scripts/verify-self-update.py OLD.AppImage NEW.AppImage [--seconds 420]

OLD is an earlier published rc's AppImage, NEW the newest published rc's (both checked against their manifests
first). In a private profile OLD is installed with Start at login, automatic updates on for Tools and the test
channel forced (ARCADE_TOOLS_CHANNEL_OVERRIDE=test, the one way to see rc releases), then started the way login
starts it (--background). Nobody touches it: its first update check must find NEW on GitHub, download it with a
delta, replace itself in place and come back in the background. Passes when the installed copy is NEW's bytes, the
receipt and history record the update, Tools logged the delta, and nothing panicked.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time

PREFERENCES = {"updates": {"enabled": True, "autoUpdate": {"arcade.tools": True}},
               "notificationsEnabled": False, "onboardingComplete": True}
DELTA = re.compile(rb"Downloaded (\S+) with a delta: (\d+) of (\d+) bytes")


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def alive(pid):
    try:
        return Path(f"/proc/{pid}/stat").read_text().rpartition(") ")[2].split()[0] != "Z"
    except OSError:
        return False


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("old", type=Path)
    parser.add_argument("new", type=Path)
    parser.add_argument("--seconds", type=int, default=420, help="how long the update may take (first check: 60 s)")
    args = parser.parse_args()
    old_image, new_image = args.old.resolve(strict=True), args.new.resolve(strict=True)
    new_digest = digest(new_image)
    assert digest(old_image) != new_digest, "OLD and NEW are the same file"
    with tempfile.TemporaryDirectory(prefix="tools-self-update-") as directory:
        root = Path(directory)
        profile = root / "profile"
        env = {k: v for k, v in os.environ.items() if not k.startswith("ARCADE_") and k not in (
            "APPIMAGE", "APPDIR", "LD_LIBRARY_PATH", "WAYLAND_DISPLAY", "HYPRLAND_INSTANCE_SIGNATURE", "SWAYSOCK")}
        env.update(ARCADE_HOME=str(profile), HOME=str(root / "home"), XDG_CONFIG_HOME=str(profile / ".config"),
                   XDG_DATA_HOME=str(profile / ".local/share"), XDG_CACHE_HOME=str(profile / ".cache"),
                   ARCADE_NO_INSTALL_PROMPT="1", ARCADE_TOOLS_CHANNEL_OVERRIDE="test", GDK_BACKEND="x11")
        downloads = root / "home/Downloads"
        downloads.mkdir(parents=True)
        state = profile / ".local/share/arcade/tools"
        endpoint = profile / "run/arcade.tools.endpoint"
        log_path = root / "tools.log"
        log = log_path.open("w+b")
        launched = []

        def resident():
            try:
                pid = json.loads(endpoint.read_text())["pid"]
            except (OSError, ValueError, KeyError):
                return None
            return pid if alive(pid) else None

        def logged():
            """What Tools wrote: this script's capture of the first process, and its log files (the restarted copy
            has no terminal, so it writes to <state>/logs)."""
            log.flush()
            text = log_path.read_bytes()
            for file in sorted((state / "logs").glob("*")) if (state / "logs").is_dir() else []:
                if file.is_file():
                    text += b"\n" + file.read_bytes()
            return text

        installed = None
        try:
            download = downloads / old_image.name
            shutil.copy2(old_image, download)
            download.chmod(0o755)
            result = subprocess.run([str(download), "--install", "--silent", "--no-launch", "--start-at-login"],
                                    env=env, capture_output=True, text=True, timeout=120)
            assert result.returncode == 0, f"--install exited {result.returncode}: {result.stderr[-2000:]}"
            old = json.loads(result.stdout)
            installed = Path(old["path"])
            assert digest(installed) == digest(old_image), old
            state.mkdir(parents=True, exist_ok=True)
            (state / "preferences.json").write_text(json.dumps(PREFERENCES))
            launched.append(subprocess.Popen([str(installed), "--background"], env=env, stdout=log,
                                             stderr=subprocess.STDOUT, start_new_session=True))
            start = time.monotonic()
            first = None
            while time.monotonic() - start < 30 and not first:
                first = resident()
                time.sleep(0.2)
            assert first, "OLD did not start in the background"
            print(f"{old['version']} ({digest(old_image)[:12]}) installed at {installed}, running in the background "
                  f"(PID {first}); waiting for it to update itself", flush=True)

            # Untouched: the first check (60 s after start) updates Tools, which restarts in the background.
            restarted = None
            while time.monotonic() - start < args.seconds:
                pid = resident()
                if pid and pid != first and digest(installed) == new_digest:
                    restarted = pid
                    break
                time.sleep(2)
            assert restarted, (f"Tools did not update itself within {args.seconds} s: installed "
                               f"{digest(installed)[:12]}, NEW {new_digest[:12]}, resident {resident()}")
            took = time.monotonic() - start
            time.sleep(5)
            assert alive(restarted), "the updated Tools stopped within 5 s"

            receipt = json.loads(subprocess.run([str(installed), "--integration-status"], env=env, check=True,
                                                capture_output=True, text=True, timeout=30).stdout)["receipt"]
            assert Path(receipt["path"]) == installed, receipt
            assert receipt.get("sha256") in (None, new_digest), receipt
            history = [json.loads(line) for line in (state / "update-history.jsonl").read_text().splitlines() if line]
            ours = [h for h in history if h.get("app") == "arcade.tools"]
            assert ours and ours[-1].get("result") == "success", f"history: {history}"
            login = Path(receipt["integration"]["autostart"])
            assert login.is_file() and str(installed) in login.read_text(), "Start at login was not kept"
            text = logged()
            delta = DELTA.search(text)
            assert delta, "Tools did not log a delta download:\n" + text.decode(errors="replace")[-4000:]
            fetched, size = int(delta.group(2)), int(delta.group(3))
            assert size == new_image.stat().st_size and fetched < size, delta.group(0)
            assert b"panicked at" not in text, "Tools logged a panic"
            print(f"updated in {took:.0f} s with a delta: {fetched} of {size} bytes ({100 * fetched / size:.1f} %); "
                  f"restarted in the background (PID {restarted}); receipt, history and Start at login agree",
                  flush=True)
            subprocess.run([str(installed), "--quit"], env=env, capture_output=True, timeout=30)
        except Exception:
            print(logged().decode(errors="replace")[-6000:])
            raise
        finally:
            if pid := resident():
                os.kill(pid, 9)
            for process in launched:
                if process.poll() is None:
                    process.kill()
    print(f"PASS: Tools updated itself from {old_image.name} to {new_image.name} with a delta")


if __name__ == "__main__":
    main()
