#!/usr/bin/env python3
"""Prove click-free repair using the actual Tools AppImage, under private D-Bus/Xvfb."""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time


def main():
    image = Path(sys.argv[1]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix='tools-maintenance-') as directory:
        root = Path(directory)
        profile = root / 'profile'
        env = {key: value for key, value in os.environ.items() if key not in (
            'APPIMAGE', 'APPDIR', 'LD_LIBRARY_PATH', 'WAYLAND_DISPLAY', 'HYPRLAND_INSTANCE_SIGNATURE', 'SWAYSOCK')}
        env.update(ARCADE_HOME=str(profile), HOME=str(root / 'home'),
                   XDG_CONFIG_HOME=str(profile / '.config'), XDG_DATA_HOME=str(profile / '.local/share'),
                   XDG_CACHE_HOME=str(profile / '.cache'), ARCADE_NO_INSTALL_PROMPT='1', GDK_BACKEND='x11')
        (root / 'home').mkdir()
        source = root / image.name
        shutil.copy2(image, source)
        source.chmod(0o755)
        result = subprocess.run([str(source), '--install', '--silent', '--no-launch', '--start-at-login'],
                                env=env, capture_output=True, text=True, check=True, timeout=60)
        receipt = json.loads(result.stdout)
        installed = Path(receipt['path'])
        desktop = Path(receipt['integration']['desktopEntry'])
        cli = Path(receipt['integration']['cli'])
        login = Path(receipt['integration']['autostart'])
        # Preserve a disabled login choice while repairing other integration.
        disabled = login.read_text() + '\nHidden=true\n'
        login.write_text(disabled)
        endpoint = profile / 'run/arcade.tools.endpoint'
        with tempfile.TemporaryFile() as log:
            child = subprocess.Popen([str(installed), '--background'], env=env,
                                     stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                def wait(predicate, message):
                    deadline = time.monotonic() + 25
                    while time.monotonic() < deadline:
                        assert child.poll() is None, f'Tools exited while checking {message}'
                        if predicate():
                            return
                        time.sleep(0.05)
                    raise AssertionError(f'Tools did not automatically restore {message}')

                wait(endpoint.is_file, 'resident endpoint')
                # Two separate faults require two event-driven repairs. There
                # is no UI, API repair request, update check or network fixture.
                for cycle in range(2):
                    time.sleep(0.7)
                    desktop.unlink()
                    cli.unlink()
                    wait(lambda: desktop.is_file() and cli.is_symlink() and cli.resolve() == installed,
                         f'launcher and CLI, cycle {cycle + 1}')
                    assert login.read_text() == disabled, 'Automatic repair enabled login'
                registry = profile / 'apps/arcade.tools.json'
                registry.unlink()
                wait(registry.is_file, 'Arcade Link registry metadata')
                assert json.loads(registry.read_text())['executable'] == str(installed)
                assert login.read_text() == disabled
                report = json.loads((profile / '.local/share/arcade/tools/maintenance/latest.json').read_text())
                assert 'arcade.tools' in report['repaired'] and not report['pending'], report
                subprocess.run([str(installed), '--quit'], env=env, capture_output=True, check=True, timeout=20)
                child.wait(timeout=15)
            except Exception:
                log.seek(0)
                print(log.read().decode(errors='replace')[-8000:], file=sys.stderr)
                raise
            finally:
                if child.poll() is None:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait()
        print('PASS: real Tools AppImage repairs repeated launcher/CLI deletion without clicks; disabled login preserved')


if __name__ == '__main__':
    main()
