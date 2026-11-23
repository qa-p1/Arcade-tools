#!/usr/bin/env python3
"""The published Tools starts the Arcade helper it ships, the helper writes its file-manager menus pointing at a copy
that outlives the AppImage's mount, and uninstalling Tools takes the menus, the helper and its copy away again.

Runs the real Tools AppImage under a private D-Bus/Xvfb (verify-package.py already checks the helper is in the
package and answers --version)."""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time


def wait(predicate, message, seconds=30):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.1)
    raise AssertionError(f'timed out waiting for {message}')


def endpoint_pid(path):
    try:
        return json.loads(path.read_text())['pid']
    except (OSError, ValueError, KeyError):
        return None


def mentions_helper(folder, own):
    """Text files under `folder` that call the helper (its menu entries, scripts and actions), outside `own` (Arcade's
    and Tools' own folders: the helper's copy, logs and state)."""
    if not folder.is_dir():
        return []
    found = []
    for path in folder.rglob('*'):
        if any(path.is_relative_to(o) for o in own):
            continue
        try:
            raw = path.read_bytes() if path.is_file() else b''
        except OSError:
            continue
        if b'\0' not in raw and b'arcade-helper' in raw:
            found.append(path)
    return found


def main():
    image = Path(sys.argv[1]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix='tools-helper-') as directory:
        root = Path(directory)
        profile = root / 'profile'
        home = root / 'home'
        data = profile / '.local/share'
        env = {key: value for key, value in os.environ.items() if key not in (
            'APPIMAGE', 'APPDIR', 'LD_LIBRARY_PATH', 'WAYLAND_DISPLAY', 'HYPRLAND_INSTANCE_SIGNATURE', 'SWAYSOCK')}
        env.update(ARCADE_HOME=str(profile), HOME=str(home), XDG_CONFIG_HOME=str(profile / '.config'),
                   XDG_DATA_HOME=str(data), XDG_CACHE_HOME=str(profile / '.cache'),
                   ARCADE_NO_INSTALL_PROMPT='1', GDK_BACKEND='x11')
        home.mkdir()
        source = root / image.name
        shutil.copy2(image, source)
        source.chmod(0o755)
        result = subprocess.run([str(source), '--install', '--silent', '--no-launch'],
                                env=env, capture_output=True, text=True, check=True, timeout=60)
        installed = Path(json.loads(result.stdout)['path'])
        tools_endpoint = profile / 'run/arcade.tools.endpoint'
        helper_endpoint = profile / 'run/arcade.helper.endpoint'
        with tempfile.TemporaryFile() as log:
            child = subprocess.Popen([str(installed), '--background'], env=env,
                                     stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            helper_pid = None
            try:
                # Tools starts the helper once it is up.
                wait(lambda: endpoint_pid(tools_endpoint), 'Tools to start')
                helper_pid = wait(lambda: endpoint_pid(helper_endpoint), 'Tools to start the Arcade helper')
                running = Path(f'/proc/{helper_pid}/exe').resolve()
                copies = [p for p in profile.rglob('arcade-helper') if p.is_file() and p.parent.name == 'helper']
                assert len(copies) == 1, f'expected one installed copy of the helper, found {copies}'
                copy = copies[0]
                assert os.access(copy, os.X_OK), f'{copy} is not executable'
                # The resident runs from the copy, not from the AppImage's temporary mount.
                assert running == copy.resolve(), f'the helper runs from {running}, not {copy}'

                def status():
                    out = subprocess.run([str(copy), 'status'], env=env, capture_output=True, text=True, timeout=20)
                    value = json.loads(out.stdout)
                    return value if 'Open With (every file manager)' in value['fileManagers'] else None
                state = wait(status, 'the helper to write its menus')
                assert state['running'] == state['version'], state

                # Every entry it wrote calls the copy; none calls a path that goes away with the AppImage's mount.
                own = [data / 'arcade', profile / 'run', profile / 'registry']
                entries = mentions_helper(data, own) + mentions_helper(home, own)
                assert entries, 'the helper wrote no menu entries'
                for entry in entries:
                    text = entry.read_text(errors='replace')
                    assert '/tmp/.mount_' not in text and '/.mount_' not in text, f'{entry} calls the AppImage mount'
                    assert str(copy) in text or str(copy).replace(' ', '\\s') in text, f'{entry} does not call {copy}'
                validator = shutil.which('desktop-file-validate')
                for entry in entries:
                    if entry.suffix == '.desktop' and entry.parent.name == 'applications' and validator:
                        subprocess.run([validator, str(entry)], check=True, capture_output=True, text=True)

                subprocess.run([str(installed), '--quit'], env=env, capture_output=True, check=True, timeout=20)
                child.wait(timeout=15)
                # The helper serves the menus without Tools.
                assert Path(f'/proc/{helper_pid}').exists(), 'quitting Tools stopped the helper'

                # Uninstalling Tools takes the menus away, stops the helper and deletes its copy.
                subprocess.run([str(installed), '--uninstall', '--silent', '--remove-data'], env=env,
                               capture_output=True, text=True, check=True, timeout=60)
                wait(lambda: not Path(f'/proc/{helper_pid}').exists(), 'the helper to stop on uninstall', 15)
                left = mentions_helper(data, own) + mentions_helper(home, own)
                assert not left, f'menu entries left after uninstalling Tools: {left}'
                assert not copy.exists(), f'{copy} left after uninstalling Tools'
                assert not helper_endpoint.exists(), 'the helper endpoint is left behind'
            except Exception:
                log.seek(0)
                print(log.read().decode(errors='replace')[-8000:], file=sys.stderr)
                raise
            finally:
                if child.poll() is None:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait()
                for pid in (endpoint_pid(tools_endpoint), helper_pid):
                    if pid and Path(f'/proc/{pid}').exists():
                        os.kill(pid, signal.SIGKILL)
    print('PASS: published Tools starts its Arcade helper; menus call a copy that outlives the AppImage; '
          'uninstalling Tools removes the menus, the helper and its copy')


if __name__ == '__main__':
    main()
