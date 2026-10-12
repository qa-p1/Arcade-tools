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
        if not os.environ.get('GITHUB_ACTIONS'):
            raise SystemExit('Windows installer smoke is restricted to disposable CI runners')
        destination = root / 'Programs' / 'Arcade Tools'
        subprocess.run(f'"{staged_package}" /S /STARTATLOGIN=0 /D={destination}', env=environment, check=True, timeout=120)
        assert (destination / 'arcade-tools.exe').is_file()
        assert (destination / 'arcade-tools-overlay.exe').is_file()
        receipt = json.loads((root / 'arcade' / 'installs' / 'arcade.tools.json').read_text(encoding='utf-8'))
        assert receipt['method'] == 'windows-installer'
        assert receipt['version'] == '0.2.0'
        subprocess.run([str(destination / 'arcade-tools.exe'), '--version'], env=environment, check=True, timeout=10)
        subprocess.run([str(destination / 'uninstall.exe'), '/S'], env=environment, check=True, timeout=120)
        assert not (root / 'arcade' / 'installs' / 'arcade.tools.json').exists()
    elif sys.platform == 'darwin':
        mount = root / 'mounted'
        mount.mkdir()
        subprocess.run(['hdiutil','attach','-readonly','-nobrowse','-mountpoint',str(mount),str(staged_package)],check=True)
        try:
            apps = list(mount.glob('*.app'))
            assert len(apps) == 1
            destination = root / apps[0].name
            subprocess.run(['ditto',str(apps[0]),str(destination)],check=True)
            binaries = destination / 'Contents' / 'MacOS'
            assert (binaries / 'arcade-tools-overlay').is_file()
            subprocess.run([str(binaries / 'arcade-tools'),'--version'], env=environment,check=True,timeout=10)
            lifecycle(binaries / 'arcade-tools', environment, root)
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
