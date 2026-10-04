#!/usr/bin/env python3
"""Check source installation in a temporary home without desktop side effects."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

PROJECT = Path(__file__).resolve().parent.parent

with tempfile.TemporaryDirectory(prefix='snzhy-install-check-') as temporary:
    sandbox = Path(temporary)
    checkout = sandbox / 'source checkout'
    home = sandbox / 'desktop user'
    commands = sandbox / 'commands'
    commands.mkdir()
    home.mkdir()
    checkout.mkdir()
    for name in ('install.sh', 'packaging/install.py', 'LICENSE', 'ATTRIBUTION.md',
                 'packaging/omarchy/install.sh', 'packaging/omarchy/snzhy-backlight.service',
                 'packaging/omarchy/manifest.json', 'packaging/omarchy/Backlight.qml',
                 'packaging/omarchy/snzhy-backlight', 'src-tauri/icons/32x32.png',
                 'src-tauri/icons/128x128.png', 'src-tauri/Cargo.toml'):
        destination = checkout / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(PROJECT / name, destination)
    binary = checkout / 'src-tauri/target/release/snzhy-OpenSycnlights'
    binary.parent.mkdir(parents=True)
    binary.write_text('#!/bin/sh\nprintf "installed-v1\\n"\n')
    binary.chmod(0o755)
    log = sandbox / 'calls.jsonl'
    for name in ('cargo', 'omarchy', 'omarchy-shell', 'systemctl', 'gdbus'):
        command = commands / name
        command.write_text('''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
with open(os.environ['INSTALL_CHECK_LOG'], 'a') as log:
    log.write(json.dumps([Path(sys.argv[0]).name, *sys.argv[1:]]) + '\\n')
if Path(sys.argv[0]).name == 'systemctl' and 'is-active' in sys.argv:
    sys.exit(0 if os.environ.get('INSTALL_CHECK_ACTIVE') == '1' else 3)
''')
        command.chmod(0o755)
    data = home / 'app data'
    config = home / 'desktop config'
    env = {**os.environ, 'HOME': str(home), 'XDG_DATA_HOME': str(data),
           'XDG_CONFIG_HOME': str(config), 'INSTALL_CHECK_LOG': str(log),
           'PATH': str(commands) + os.pathsep + os.environ['PATH']}

    def run(*arguments, success=True):
        result = subprocess.run(['bash', str(checkout / 'install.sh'), *arguments],
                                env=env, capture_output=True, text=True)
        assert (result.returncode == 0) == success, result.stderr + result.stdout
        return result

    run('--help')
    assert not log.exists(), 'help must not build or change the desktop'
    run('--unknown', success=False)
    run()
    calls = [json.loads(line) for line in log.read_text().splitlines()]
    build = next(call for call in calls if call[0] == 'cargo')
    assert '--locked' in build and '--offline' not in build, 'new users need a network-capable locked build'
    assert not any(call[0] in ('systemctl', 'omarchy') for call in calls), 'standalone install touched the desktop service/bar'
    installed = data / 'snzhy-opensycnlights/bin/snzhy-OpenSycnlights'
    launcher = home / '.local/bin/snzhy-opensycnlights'
    desktop = data / 'applications/com.snzhy.opensycnlights.desktop'
    assert installed.is_file() and os.access(launcher, os.X_OK) and desktop.is_file()
    assert str(checkout) not in launcher.read_text(), 'installed launcher depends on checkout'
    assert subprocess.check_output([str(launcher)], env=env, text=True).strip() == 'installed-v1'
    assert 'snzhy-opensycnlights' in desktop.read_text()
    if shutil.which('desktop-file-validate'):
        subprocess.run(['desktop-file-validate', str(desktop)], check=True)

    shell = config / 'omarchy/shell.json'
    shell.parent.mkdir(parents=True)
    original_shell = '{"bar":{"position":"top"},"custom":"preserve me"}\n'
    shell.write_text(original_shell)
    log.write_text('')
    run('--omarchy', '--offline')
    calls = [json.loads(line) for line in log.read_text().splitlines()]
    assert '--offline' in next(call for call in calls if call[0] == 'cargo')
    assert ['omarchy', 'plugin', 'enable', 'snzhy.backlight', '--after', 'omarchy.tray'] in calls
    assert ['systemctl', '--user', 'enable', '--now', 'snzhy-backlight.service'] in calls
    assert shell.read_text() == original_shell
    backups = list(shell.parent.glob('shell.json.backup-backlight-*'))
    assert backups and backups[0].read_text() == original_shell
    service = config / 'systemd/user/snzhy-backlight.service'
    unit = service.read_text()
    assert str(launcher) in unit and '" --background' in unit
    assert '@CONTROLLER_LAUNCHER@' not in unit and str(checkout) not in unit
    assert '/home/snezhy' not in unit
    plugin = config / 'omarchy/plugins/snzhy.backlight'
    manifest = json.loads((plugin / 'manifest.json').read_text())
    assert (plugin / manifest['entryPoints']['barWidget']).is_file()
    assert os.access(home / '.local/bin/snzhy-backlight', os.X_OK)

    binary.write_text('#!/bin/sh\nprintf "installed-v2\\n"\n')
    env['INSTALL_CHECK_ACTIVE'] = '1'
    log.write_text('')
    # Test the legacy plugin-install entry point as well as the public root command.
    result = subprocess.run(['bash', str(checkout / 'packaging/omarchy/install.sh'), '--offline'],
                            env=env, capture_output=True, text=True)
    assert result.returncode == 0, result.stderr
    calls = [json.loads(line) for line in log.read_text().splitlines()]
    stop = calls.index(['systemctl', '--user', 'stop', 'snzhy-backlight.service'])
    start = calls.index(['systemctl', '--user', 'enable', '--now', 'snzhy-backlight.service'])
    assert stop < start, 'update did not stop the previous service before starting the installed build'
    assert subprocess.check_output([str(launcher)], env=env, text=True).strip() == 'installed-v2'
    checkout.rename(sandbox / 'moved source checkout')
    assert subprocess.check_output([str(launcher)], env=env, text=True).strip() == 'installed-v2', 'moving the checkout broke the installed app'
    print('PASS: fresh/locked and offline builds, standalone isolation, Omarchy service/widget, paths with spaces, XDG directories, config backup, updates and checkout-independent launch.')
