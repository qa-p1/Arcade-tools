#!/usr/bin/env python3
"""Exercise CI-built packages inside a disposable profile; never touch a live install."""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--target', required=True)
args = parser.parse_args()
repo = Path(__file__).resolve().parents[1]
version = json.loads((repo / 'src-tauri' / 'tauri.conf.json').read_text())['version']
bundle = repo / 'target' / args.target / 'release' / 'bundle'
extension = '.exe' if sys.platform == 'win32' else '.dmg' if sys.platform == 'darwin' else '.AppImage'
packages = sorted(bundle.rglob('*' + extension))
if len(packages) != 1:
    raise SystemExit(f'Expected one installer: {packages}')
package = packages[0]


def lifecycle(executable, environment, root):
    def run(*args):
        return subprocess.run([str(executable), *args], env=environment, capture_output=True, text=True, encoding='utf-8', check=True, timeout=90)
    # README §Clean uninstall: whatever Tools adds to a fresh profile, --uninstall --remove-data takes away again
    # (the runtime folder only holds sockets of processes that have exited).
    def files():
        return {p for p in root.rglob('*') if not p.is_dir() and not p.is_relative_to(root / 'runtime')}
    fresh = files()  # the package; on Linux also the legacy self-update copy staged below
    # Old Tools stages self-updates in this legacy cache. Migration must not
    # remove the executing AppImage before the installer copies its bytes.
    if sys.platform == 'linux':
        import hashlib
        legacy = Path(environment['ARCADE_HOME']) / '.cache/arcade-tools/downloads'
        legacy.mkdir(parents=True, exist_ok=True)
        staged = legacy / 'self-update.AppImage'
        shutil.copy2(executable, staged)
        staged.chmod(0o755)
        fresh.add(staged)
        result = subprocess.run([str(staged), '--install', '--silent', '--no-launch'],
            env=environment, capture_output=True, text=True, check=True, timeout=90)
        upgraded = json.loads(result.stdout)
        digest = lambda path: hashlib.sha256(Path(path).read_bytes()).hexdigest()
        assert staged.is_file(), 'Installer moved its source out from under the old helper'
        assert digest(upgraded['path']) == digest(executable), 'Self-update installed different bytes'
    installed = json.loads(run('--install', '--silent', '--no-launch').stdout)
    receipt_path = root / 'arcade' / 'installs' / 'arcade.tools.json'
    assert installed['version'] == version
    assert receipt_path.is_file()
    assert Path(installed['path']).exists()
    assert json.loads(run('--integration-status').stdout)
    run('--repair')
    run('--uninstall', '--silent')
    assert not receipt_path.exists()
    run('--install', '--silent', '--no-launch')
    run('--uninstall', '--silent', '--remove-data')
    left = sorted(str(p.relative_to(root)) for p in files() - fresh)
    assert not left, f'--uninstall --remove-data left files in a fresh profile: {left}'


with tempfile.TemporaryDirectory(prefix='arcade-tools-package-') as temporary:
    root = Path(temporary)
    environment = dict(os.environ, ARCADE_HOME=str(root / 'arcade'), ARCADE_NO_INSTALL_PROMPT='1', HOME=str(root / 'home'))
    (root / 'home').mkdir()
    staged_package = root / package.name
    shutil.copy2(package, staged_package)
    for key, name in [('XDG_CONFIG_HOME','config'), ('XDG_DATA_HOME','data'), ('XDG_CACHE_HOME','cache'), ('XDG_RUNTIME_DIR','runtime')]:
        environment[key] = str(root / name)
        (root / name).mkdir()
    if sys.platform == 'win32':
        # CI only, not a real desktop: shortcuts and Apps & features use the
        # disposable runner account; receipts, the CLI shim and PATH stay under
        # the isolated ARCADE_HOME.
        import winreg
        if not os.environ.get('GITHUB_ACTIONS'):
            raise SystemExit('Windows installer smoke is restricted to disposable CI runners')
        receipt_path = root / 'arcade' / 'installs' / 'arcade.tools.json'
        programs = Path(os.environ['APPDATA']) / 'Microsoft' / 'Windows' / 'Start Menu' / 'Programs'
        startup = programs / 'Startup' / 'Arcade Tools.lnk'
        launcher = programs / 'Arcade Tools' / 'Arcade Tools.lnk'
        desktop = Path(os.environ['USERPROFILE']) / 'Desktop' / 'Arcade Tools.lnk'
        uninstall_key = r'Software\Microsoft\Windows\CurrentVersion\Uninstall\arcade.tools'
        for path in (startup, launcher, desktop):
            assert not path.exists(), f'Unexpected existing Tools shortcut: {path}'

        def registry(name):
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, uninstall_key) as key:
                return winreg.QueryValueEx(key, name)[0]

        def registered():
            try:
                winreg.OpenKey(winreg.HKEY_CURRENT_USER, uninstall_key).Close()
                return True
            except FileNotFoundError:
                return False

        def same(a, b):
            return os.path.normcase(str(Path(a).resolve())) == os.path.normcase(str(Path(b).resolve()))

        def installed(destination, login, desktop_shortcut):
            executable = destination / 'arcade-tools.exe'
            assert executable.is_file() and (destination / 'arcade-tools-overlay.exe').is_file()
            receipt = json.loads(receipt_path.read_text(encoding='utf-8'))
            assert receipt['method'] == 'windows-installer' and receipt['version'] == version
            assert same(receipt['path'], executable), receipt['path']
            integration = receipt['integration']
            assert Path(integration['cli']).is_file()
            assert launcher.is_file() and same(integration['desktopEntry'], launcher)
            assert startup.is_file() == login == bool(integration.get('autostart')), integration
            assert desktop.is_file() == desktop_shortcut == bool(integration.get('desktopShortcut')), integration
            assert registry('DisplayVersion') == version
            assert registry('URLInfoAbout') == 'https://github.com/qa-p1/Arcade-catalog'
            quiet = registry('QuietUninstallString')
            assert quiet.endswith('uninstall.exe" /S') and same(quiet[1:-len('" /S')], destination / 'uninstall.exe'), quiet
            subprocess.run([str(executable), '--version'], env=environment, check=True, timeout=10)
            return receipt

        def uninstalled(destination, receipt):
            # _?= keeps NSIS from detaching a temporary copy, so the receipt
            # check below sees the finished uninstall.
            uninstaller = destination / 'uninstall.exe'
            subprocess.run(f'"{uninstaller}" /S _?={destination}', env=environment, check=True, timeout=180)
            assert not (destination / 'arcade-tools.exe').exists()
            assert not receipt_path.exists()
            for key in ('cli', 'desktopEntry', 'autostart', 'desktopShortcut'):
                if receipt['integration'].get(key):
                    assert not Path(receipt['integration'][key]).exists(), key
            assert not registered()

        # A plain /S uses the default per-user folder and task defaults: start
        # at login on (Tools hosts tray icons), no desktop shortcut.
        default = Path(os.environ['LOCALAPPDATA']) / 'Programs' / 'Arcade Tools'
        subprocess.run(f'"{staged_package}" /S', env=environment, check=True, timeout=180)
        uninstalled(default, installed(default, True, False))
        # Explicit tasks and a custom folder, as Tools and the wizard pass them.
        destination = root / 'Programs' / 'Arcade Tools'
        subprocess.run(f'"{staged_package}" /S /STARTATLOGIN=0 /DESKTOPSHORTCUT=1 /D={destination}', env=environment, check=True, timeout=180)
        uninstalled(destination, installed(destination, False, True))
    elif sys.platform == 'darwin':
        # CI only, not a real desktop: the same flow as an app's dmg check.
        import filecmp
        mount = root / 'mounted'
        mount.mkdir()
        subprocess.run(['hdiutil','attach','-readonly','-nobrowse','-mountpoint',str(mount),str(staged_package)],check=True)
        try:
            apps = list(mount.glob('*.app'))
            assert len(apps) == 1
            assert (mount / 'Applications').is_symlink()
            assert os.readlink(mount / 'Applications') == '/Applications'
            binaries = apps[0] / 'Contents' / 'MacOS'
            assert (binaries / 'arcade-tools-overlay').is_file()
            for name in ['arcade-tools', 'arcade-tools-overlay']:
                executable = binaries / name
                subprocess.run([str(executable),'--version'], env=environment,check=True,timeout=10)
                if args.target == 'universal-apple-darwin':
                    architectures = subprocess.run(['lipo','-archs',str(executable)],capture_output=True,text=True,check=True).stdout.split()
                    assert set(architectures) == {'arm64','x86_64'}, (name, architectures)
            # Link uses ARCADE_HOME as the home folder, so Applications is under it.
            subprocess.run([str(binaries / 'arcade-tools'), '--install', '--silent', '--no-launch'], env=environment, check=True, timeout=90)
            target = root / 'arcade' / 'Applications' / 'Arcade Tools.app'
            installed = target / 'Contents' / 'MacOS' / 'arcade-tools'
            assert filecmp.cmp(binaries / 'arcade-tools', installed, shallow=False)
            assert (target / 'Contents' / 'MacOS' / 'arcade-tools-overlay').is_file()
            receipt_path = root / 'arcade' / 'installs' / 'arcade.tools.json'
            receipt = json.loads(receipt_path.read_text(encoding='utf-8'))
            assert receipt['method'] == 'macos-bundle' and receipt['version'] == version
            assert Path(receipt['path']) == target and receipt['managedBy'] == 'self'
            cli = Path(receipt['integration']['cli'])
            assert cli.is_symlink() and cli.resolve(strict=True) == installed.resolve()
            def run(*arguments):
                return subprocess.run([str(installed), *arguments], env=environment, capture_output=True, text=True, encoding='utf-8', check=True, timeout=30).stdout
            status = json.loads(run('--integration-status'))
            assert status['receipt']['path'] == str(target)
            assert json.loads(run('--repair'))['integration']['cli'] == str(cli)
            for profile in (root / 'home', root / 'arcade'):
                for name in ('.zprofile', '.bash_profile', '.profile', '.zshrc'):
                    assert not (profile / name).exists(), f'Shell profile written: {name}'
            run('--uninstall', '--silent')
            assert not receipt_path.exists() and not target.exists() and not os.path.lexists(cli)
        finally:
            subprocess.run(['hdiutil','detach',str(mount)],check=True)
    else:
        environment.pop('WAYLAND_DISPLAY',None)
        environment.pop('HYPRLAND_INSTANCE_SIGNATURE',None)
        subprocess.run([str(staged_package),'--appimage-extract'],cwd=root,env=environment,stdout=subprocess.DEVNULL,check=True,timeout=90)
        binaries = root / 'squashfs-root' / 'usr' / 'bin'
        assert (binaries / 'arcade-tools-overlay').is_file()
        # A bundled Ubuntu libwayland-client breaks the host Mesa EGL on newer
        # distributions (WebKit aborts with EGL_BAD_PARAMETER, blank window).
        bundled = sorted(p.name for p in (root / 'squashfs-root').rglob('libwayland-client.so*'))
        assert not bundled, f'AppImage must use the host libwayland-client: {bundled}'
        # The overlay's X11 path dlopens the host libxkbcommon-x11; a bundled libxkbcommon from another release
        # beside it crashed on the first key press (TL-5). Both must come from the host.
        bundled = sorted(p.name for p in (root / 'squashfs-root').rglob('libxkbcommon*.so*'))
        assert not bundled, f'AppImage must use the host libxkbcommon: {bundled}'
        # OPEN1: Tools preloads this into WebKit's helper processes (main.rs).
        assert (root / 'squashfs-root' / 'usr' / 'lib' / 'libarcade-trim.so').is_file(), 'AppImage lacks libarcade-trim.so'
        # The fleet identity (ARCADE_APP.md ID-1/ID-3): one dev.arcade.Tools entry, its icon and the symbolic icon.
        share = root / 'squashfs-root' / 'usr' / 'share'
        for identity in ('applications/dev.arcade.Tools.desktop', 'icons/hicolor/scalable/apps/dev.arcade.Tools.svg',
                         'icons/hicolor/symbolic/apps/dev.arcade.Tools-symbolic.svg'):
            assert (share / identity).is_file(), f'AppImage lacks {identity}'
        # Settings › About › "Show licences" reads the bundled notices (REPO-1).
        notices = list((root / 'squashfs-root' / 'usr' / 'lib').rglob('THIRD_PARTY_NOTICES.md'))
        assert len(notices) == 1 and notices[0].stat().st_size > 100_000, f'AppImage lacks the notices: {notices}'
        subprocess.run([str(binaries / 'arcade-tools'),'--version'],env=environment,check=True,timeout=10)
        environment['APPIMAGE'] = str(staged_package)
        # Exercise the mounting runtime and full AppImage bytes, not only
        # the extracted ELF (which cannot establish installer hash identity).
        lifecycle(staged_package, environment, root)
        # The package itself must survive its first webview, the main window.
        # AppRun changes into $APPDIR/usr and the mounting runtime sets $OWD
        # (extract-and-run doesn't, so it is set here); WebKit finds its
        # helpers relative to $APPDIR/usr and aborts the whole app otherwise.
        # The lifecycle above moved the staged copy; this one is a fresh download.
        work = root / 'work'
        work.mkdir()
        download = work / package.name
        shutil.copy2(package, download)
        window = dict(environment, APPIMAGE_EXTRACT_AND_RUN='1', OWD=str(work))
        window.pop('APPIMAGE')
        # U1: the launcher copies the image to <data>/arcade/runtime/ once, starts Tools from there and exits 0.
        # The shell keeps the display and bus alive after it exits; the copy must still run with its window open.
        def running_unpacked():
            found = {}
            for proc in Path('/proc').iterdir():
                try:
                    exe = os.readlink(proc / 'exe')
                except OSError:
                    continue
                if exe.startswith(str(root) + '/') and '/arcade/runtime/tools-' in exe and exe.endswith('/arcade-tools'):
                    found[int(proc.name)] = exe
            return found
        with tempfile.TemporaryFile() as log:
            child = subprocess.Popen(['dbus-run-session', '--', 'xvfb-run', '-a', 'sh', '-c', '"$1" || exit $?; exec sleep 600',
                                      'sh', str(download)], cwd=work, env=window,
                                     stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            unpacked = {}
            try:
                code = child.wait(timeout=20)
            except subprocess.TimeoutExpired:
                code = None
                unpacked = running_unpacked()
            finally:
                for pid in list(unpacked) + list(running_unpacked()):
                    try:
                        os.kill(pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                child.wait()
                # The bus may have started the document portal for the window;
                # killed, it leaves a dead FUSE mount in the profile.
                fusermount = shutil.which('fusermount3') or shutil.which('fusermount')
                for line in Path('/proc/self/mounts').read_text().splitlines():
                    mountpoint = line.split()[1]
                    if fusermount and mountpoint.startswith(str(root) + '/'):
                        subprocess.run([fusermount, '-uz', mountpoint], check=False)
            log.seek(0)
            output = log.read().decode(errors="replace")[-4000:]
            assert code is None, f'Tools exited {code} with its window open:\n{output}'
            assert unpacked, f'Tools is not running from its unpacked copy (U1) with its window open:\n{output}'
print(f'PASS: {sys.platform} package contains Tools and the native overlay; isolated package smoke')
