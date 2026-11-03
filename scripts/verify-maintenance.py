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
        report_path = profile / '.local/share/arcade/tools/maintenance/latest.json'
        desktop_cache = desktop.parent / 'mimeinfo.cache'
        desktop_cache.unlink(missing_ok=True)
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
                wait(desktop_cache.is_file, 'desktop application cache at startup')
                assert login.read_text() == disabled, f'Startup changed login choice:\n{login.read_text()}'
                def report_stamp():
                    return report_path.stat().st_mtime_ns if report_path.is_file() else None

                def completed(previous):
                    if report_stamp() == previous:
                        return False
                    report = json.loads(report_path.read_text())
                    return 'arcade.tools' in report['repaired'] and not report['pending']

                # Two separate faults require two event-driven repairs. There
                # is no UI, API repair request, update check or network fixture.
                for cycle in range(2):
                    time.sleep(0.7)
                    previous = report_stamp()
                    desktop_cache.unlink(missing_ok=True)
                    desktop.unlink()
                    cli.unlink()
                    # The child creates links before the worker restores any
                    # preserved custom entries. Wait for the transaction's
                    # completion report before asserting its final state.
                    wait(lambda: completed(previous) and desktop.is_file() and desktop_cache.is_file()
                         and cli.is_symlink() and cli.resolve() == installed,
                         f'launcher and CLI, cycle {cycle + 1}')
                    assert login.read_text() == disabled, f'Login choice changed:\n{login.read_text()}'
                registry = profile / 'apps/arcade.tools.json'
                previous = report_stamp()
                registry.unlink()
                wait(lambda: completed(previous) and registry.is_file(), 'Arcade Link registry metadata')
                assert json.loads(registry.read_text())['executable'] == str(installed)
                assert login.read_text() == disabled
                previous = report_stamp()
                cached = json.loads(registry.read_text())
                cached['version'] = '0.0.1'
                registry.write_text(json.dumps(cached))
                wait(lambda: completed(previous)
                     and json.loads(registry.read_text())['version'] == receipt['version'],
                     'stale registry version with a correct executable path')
                assert login.read_text() == disabled
                report = json.loads(report_path.read_text())
                assert 'arcade.tools' in report['repaired'] and not report['pending'], report
                # Healthy custom launchers also need their discovery cache
                # refreshed, without rewriting the user's entry.
                customized = desktop.read_text().replace('[Desktop Entry]\n',
                    '[Desktop Entry]\nMimeType=application/x-arcade-maintenance;\n', 1)
                desktop_cache.unlink()
                desktop.write_text(customized)
                wait(lambda: desktop_cache.is_file()
                     and 'application/x-arcade-maintenance=' in desktop_cache.read_text(),
                     'cache for a healthy customized launcher')
                assert desktop.read_text() == customized
                assert login.read_text() == disabled
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
        print('PASS: real Tools AppImage repairs repeated launcher/CLI deletion and desktop caches without clicks; disabled login preserved')


if __name__ == '__main__':
    main()
