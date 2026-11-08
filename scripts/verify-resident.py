#!/usr/bin/env python3
"""Actual AppImage close/reopen PID gate. Run under private D-Bus and Xvfb."""
import ctypes
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time


def close_window(identifier):
    # Real WM_DELETE_WINDOW, even without an Xvfb window manager.
    class Data(ctypes.Union):
        _fields_ = [('b', ctypes.c_char * 20), ('s', ctypes.c_short * 10), ('l', ctypes.c_long * 5)]
    class Message(ctypes.Structure):
        _fields_ = [('type', ctypes.c_int), ('serial', ctypes.c_ulong), ('send_event', ctypes.c_int),
                    ('display', ctypes.c_void_p), ('window', ctypes.c_ulong), ('message_type', ctypes.c_ulong),
                    ('format', ctypes.c_int), ('data', Data)]
    class Event(ctypes.Union):
        _fields_ = [('client', Message), ('padding', ctypes.c_long * 24)]
    x11 = ctypes.CDLL('libX11.so.6')
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XInternAtom.restype = ctypes.c_ulong
    x11.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
    x11.XSendEvent.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_long, ctypes.POINTER(Event)]
    x11.XFlush.argtypes = [ctypes.c_void_p]
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    display = x11.XOpenDisplay(None)
    assert display
    try:
        event = Event()
        event.client.type = 33
        event.client.display = display
        event.client.window = int(identifier)
        event.client.message_type = x11.XInternAtom(display, b'WM_PROTOCOLS', 0)
        event.client.format = 32
        event.client.data.l[0] = x11.XInternAtom(display, b'WM_DELETE_WINDOW', 0)
        x11.XSendEvent(display, int(identifier), 0, 0, ctypes.byref(event))
        x11.XFlush(display)
    finally:
        x11.XCloseDisplay(display)


def descendants(resident):
    processes = {}
    for entry in Path('/proc').iterdir():
        if not entry.name.isdigit():
            continue
        try:
            fields = (entry / 'stat').read_text().rpartition(') ')[2].split()
            processes[int(entry.name)] = int(fields[1])
        except (OSError, IndexError, ValueError):
            pass
    family = {resident}
    while True:
        children = {pid for pid, parent in processes.items() if parent in family}
        if children <= family:
            return family
        family |= children


def renderers(resident):
    result = []
    for pid in descendants(resident):
        try:
            executable = Path(f'/proc/{pid}/cmdline').read_bytes().split(b'\0')[0]
            if executable.endswith(b'WebKitWebProcess'):
                result.append(pid)
        except OSError:
            pass
    return result


def memory(resident):
    pss = 0
    for pid in descendants(resident):
        try:
            for line in Path(f'/proc/{pid}/smaps_rollup').read_text().splitlines():
                if line.startswith('Pss:'):
                    pss += int(line.split()[1])
        except OSError:
            pass
    return round(pss / 1024, 1)


def main():
    image = Path(sys.argv[1]).resolve(strict=True)
    app = sys.argv[2]
    title, opening = {'arcade.tools': ('^Arcade Tools$', []),
                      'arcade.box': ('^Arcade Box$', ['--show'])}[app]
    with tempfile.TemporaryDirectory(prefix=app + '-resident-') as directory:
        root = Path(directory)
        env = {k: v for k, v in os.environ.items() if k not in (
            'APPIMAGE', 'APPDIR', 'LD_LIBRARY_PATH', 'ARCADE_DEV_BUILD', 'WAYLAND_DISPLAY',
            'HYPRLAND_INSTANCE_SIGNATURE', 'SWAYSOCK', 'ARCADE_TOOLS_NO_LEAN_RESTART',
            'ARCADE_BOX_NO_LEAN_RESTART', 'ARCADE_LOOK_NO_LEAN_RESTART')}
        env.update(ARCADE_HOME=str(root / 'profile'), HOME=str(root / 'home'),
                   XDG_CONFIG_HOME=str(root / 'config'), XDG_DATA_HOME=str(root / 'data'),
                   XDG_CACHE_HOME=str(root / 'cache'), XDG_RUNTIME_DIR=str(root / 'run'),
                   ARCADE_NO_INSTALL_PROMPT='1', GDK_BACKEND='x11')
        for name in ('home', 'config', 'data', 'cache', 'run'):
            (root / name).mkdir(mode=0o700)
        source = root / 'Download.AppImage'
        shutil.copy2(image, source)
        source.chmod(0o755)
        receipt = json.loads(subprocess.check_output(
            [str(source), '--install', '--silent', '--no-launch'], env=env, text=True, timeout=90))
        installed = Path(receipt['path'])
        endpoint = root / 'profile/run' / (app + '.endpoint')
        def run(*args):
            try:
                return subprocess.run([str(installed), *args], env=env, check=True,
                                      capture_output=True, text=True, timeout=25)
            except subprocess.TimeoutExpired as error:
                # What the hung launch said, and every process of this profile's app (a second instance that
                # failed to hand over would hold the pipes open).
                print(f'{" ".join(args)} did not return in 25 s; its output:\n{error.stdout or ""}{error.stderr or ""}',
                      file=sys.stderr)
                ps = subprocess.run(['ps', '-eo', 'pid,ppid,stat,etimes,args'], capture_output=True, text=True).stdout
                print('\n'.join(line for line in ps.splitlines() if 'PID' in line or 'arcade' in line.lower()),
                      file=sys.stderr)
                raise
        def pid():
            try:
                return json.loads(endpoint.read_text())['pid']
            except (FileNotFoundError, ValueError):
                return None
        def visible():
            r = subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', title],
                               env=env, capture_output=True, text=True, timeout=5)
            return r.stdout.strip().splitlines() if r.returncode == 0 else []
        def wait(probe, description, seconds=20):
            end = time.monotonic() + seconds
            while time.monotonic() < end:
                result = probe()
                if result:
                    return result
                time.sleep(.05)
            raise AssertionError('Timed out: ' + description)
        with tempfile.TemporaryFile() as log:
            child = subprocess.Popen([str(installed), '--background'], env=env, stdout=log,
                                     stderr=subprocess.STDOUT, start_new_session=True)
            try:
                original = wait(pid, 'initial resident endpoint')
                for cycle in range(3):
                    run(*opening)
                    identifier = wait(visible, 'native window')[0]
                    wait(lambda: renderers(original), 'native renderer starts')
                    assert pid() == original, 'Opening a window replaced the resident'
                    close_window(identifier)
                    wait(lambda: not visible(), 'window closes')
                    # Cross the old automatic-restart deadline on the first
                    # close, then exercise two quick create/destroy cycles.
                    deadline = time.monotonic() + (26 if cycle == 0 else 1)
                    while time.monotonic() < deadline:
                        # The installed AppImage hands over to its unpacked copy (Link U1): its launcher exits 0 and
                        # the resident named by the endpoint carries on, so follow that PID.
                        assert child.poll() in (None, 0), f'Tools launcher exited {child.returncode}'
                        assert Path(f'/proc/{original}').exists(), 'Closing a window exited the app'
                        assert pid() == original, 'Closing a window changed the resident PID'
                        time.sleep(.1)
                    wait(lambda: not renderers(original), 'closed WebView releases its renderer', seconds=10)
                    assert pid() == original
                    print(f'Cycle {cycle + 1}: PID {original} stable, 0 renderers, group PSS {memory(original)} MiB', flush=True)
                run('--restart')
                successor = wait(lambda: (p if (p := pid()) and p != original else None),
                                 'explicit Restart replaces the process')
                assert successor != original
                run('--quit')
                wait(lambda: not endpoint.exists(), 'explicit Quit stops resident')
            except Exception:
                log.seek(0)
                print(log.read().decode(errors='replace')[-6000:], file=sys.stderr)
                raise
            finally:
                subprocess.run([str(installed), '--quit'], env=env, capture_output=True, timeout=15)
                if child.poll() is None:
                    os.killpg(child.pid, signal.SIGKILL)
                child.wait(timeout=15)
            log.seek(0)
            output = log.read().decode(errors="replace")
            assert "panicked at" not in output, "Native app logged a panic:\n" + output
        print(f'PASS: {app} actual AppImage keeps its PID and releases renderers across 3 window close/reopen cycles; explicit Restart and Quit still work')


if __name__ == '__main__':
    main()
