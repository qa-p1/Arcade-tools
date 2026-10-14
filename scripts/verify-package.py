#!/usr/bin/env python3
"""Exercise CI-built packages inside a disposable profile; never touch a live install."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--target', required=True)
args = parser.parse_args()
repo = Path(__file__).resolve().parents[1]
bundle = repo / 'target' / args.target / 'release' / 'bundle'
extension = '.exe' if sys.platform == 'win32' else '.dmg' if sys.platform == 'darwin' else '.AppImage'
packages = sorted(bundle.rglob('*' + extension))
if len(packages) != 1:
    raise SystemExit(f'Expected one installer: {packages}')
package = packages[0]


def lifecycle(executable, environment, root):
    def run(*args):
        return subprocess.run([str(executable), *args], env=environment, capture_output=True, text=True, encoding='utf-8', check=True, timeout=90)
    installed = json.loads(run('--install', '--silent', '--no-launch').stdout)
    receipt_path = root / 'arcade' / 'installs' / 'arcade.tools.json'
    assert installed['version'] == '0.2.0'
    assert receipt_path.is_file()
    assert Path(installed['path']).exists()
    assert json.loads(run('--integration-status').stdout)
    run('--repair')
    run('--uninstall')
    assert not receipt_path.exists()


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
            assert receipt['method'] == 'windows-installer' and receipt['version'] == '0.2.0'
            assert same(receipt['path'], executable), receipt['path']
            integration = receipt['integration']
            assert Path(integration['cli']).is_file()
            assert launcher.is_file() and same(integration['desktopEntry'], launcher)
            assert startup.is_file() == login == bool(integration.get('autostart')), integration
            assert desktop.is_file() == desktop_shortcut == bool(integration.get('desktopShortcut')), integration
            assert registry('DisplayVersion') == '0.2.0'
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
            assert receipt['method'] == 'macos-bundle' and receipt['version'] == '0.2.0'
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
            run('--uninstall')
            assert not receipt_path.exists() and not target.exists() and not os.path.lexists(cli)
        finally:
            subprocess.run(['hdiutil','detach',str(mount)],check=True)
    else:
        environment.pop('WAYLAND_DISPLAY',None)
        environment.pop('HYPRLAND_INSTANCE_SIGNATURE',None)
        subprocess.run([str(staged_package),'--appimage-extract'],cwd=root,env=environment,stdout=subprocess.DEVNULL,check=True,timeout=90)
        binaries = root / 'squashfs-root' / 'usr' / 'bin'
        assert (binaries / 'arcade-tools-overlay').is_file()
        subprocess.run([str(binaries / 'arcade-tools'),'--version'],env=environment,check=True,timeout=10)
        environment['APPIMAGE'] = str(staged_package)
        lifecycle(binaries / 'arcade-tools', environment, root)
print(f'PASS: {sys.platform} package contains Tools and the native overlay; isolated package smoke')
