#!/usr/bin/env python3
"""Install a built app for the current user; system USB rules stay explicit."""
import datetime
import os
from pathlib import Path
import shutil
import subprocess
import sys

PROJECT = Path(__file__).resolve().parent.parent
HOME = Path.home()
DATA = Path(os.environ.get('XDG_DATA_HOME', HOME / '.local/share'))
CONFIG = Path(os.environ.get('XDG_CONFIG_HOME', HOME / '.config'))
BIN = HOME / '.local/bin'
APP = DATA / 'snzhy-opensycnlights'
LAUNCHER = BIN / 'snzhy-opensycnlights'
SERVICE_NAME = 'snzhy-backlight.service'


def atomic_copy(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_name(destination.name + '.installing')
    shutil.copy2(source, temporary)
    temporary.replace(destination)


def write(path, text, mode=0o644):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + '.installing')
    temporary.write_text(text)
    temporary.chmod(mode)
    temporary.replace(path)


def systemd_quote(value):
    return '"' + str(value).replace('\\', '\\\\').replace('"', '\\"').replace('%', '%%').replace('$', '$$') + '"'


def desktop_quote(value):
    value = str(value).replace('\\', '\\\\\\\\').replace('"', '\\\\"').replace('`', '\\\\`').replace('$', '\\\\$').replace('%', '%%')
    return '"' + value + '"'


def main():
    if os.geteuid() == 0:
        raise RuntimeError('Run as your desktop user, without sudo.')
    mode = sys.argv[1] if len(sys.argv) == 2 else ''
    if mode not in ('standalone', 'omarchy'):
        raise RuntimeError('Expected standalone or omarchy.')
    binary = PROJECT / 'src-tauri/target/release/snzhy-OpenSycnlights'
    if not binary.is_file():
        raise RuntimeError('Release binary missing. Run ./install.sh first.')
    was_running = mode == 'omarchy' and subprocess.run(
        ['systemctl', '--user', 'is-active', '--quiet', SERVICE_NAME], check=False).returncode == 0
    if was_running:
        subprocess.run(['systemctl', '--user', 'stop', SERVICE_NAME], check=True)
    atomic_copy(binary, APP / 'bin/snzhy-OpenSycnlights')
    atomic_copy(PROJECT / 'LICENSE', APP / 'LICENSE')
    atomic_copy(PROJECT / 'ATTRIBUTION.md', APP / 'ATTRIBUTION.md')
    for size in (32, 128):
        atomic_copy(PROJECT / f'src-tauri/icons/{size}x{size}.png',
                    DATA / f'icons/hicolor/{size}x{size}/apps/snzhy-opensycnlights.png')
    # The installed launcher does not depend on the source checkout.
    write(LAUNCHER, '''#!/usr/bin/env bash
set -euo pipefail
controller_data="${XDG_DATA_HOME:-$HOME/.local/share}"
export WEBKIT_DISABLE_DMABUF_RENDERER="${WEBKIT_DISABLE_DMABUF_RENDERER:-1}"
export GDK_BACKEND="${GDK_BACKEND:-x11}"
exec "$controller_data/snzhy-opensycnlights/bin/snzhy-OpenSycnlights" "$@"
''', 0o755)
    write(DATA / 'applications/com.snzhy.opensycnlights.desktop', f'''[Desktop Entry]
Type=Application
Name=snzhy-OpenSycnlights
Comment=Robobloq USB backlight controller with screen and audio sync
Exec={desktop_quote(LAUNCHER)}
Icon=snzhy-opensycnlights
Terminal=false
Categories=Utility;
StartupWMClass=snzhy-OpenSycnlights
''')
    if mode == 'omarchy':
        source = PROJECT / 'packaging/omarchy'
        plugin = CONFIG / 'omarchy/plugins/snzhy.backlight'
        shell_config = CONFIG / 'omarchy/shell.json'
        if shell_config.exists():
            backup = shell_config.with_name('shell.json.backup-backlight-' + datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
            shutil.copy2(shell_config, backup)
        for name in ('manifest.json', 'Backlight.qml'):
            atomic_copy(source / name, plugin / name)
        atomic_copy(source / 'snzhy-backlight', BIN / 'snzhy-backlight')
        (BIN / 'snzhy-backlight').chmod(0o755)
        service = (source / SERVICE_NAME).read_text().replace('@CONTROLLER_LAUNCHER@', systemd_quote(LAUNCHER))
        write(CONFIG / 'systemd/user' / SERVICE_NAME, service)
        for command in (
            ['systemctl', '--user', 'daemon-reload'],
            ['systemctl', '--user', 'enable', '--now', SERVICE_NAME],
            ['omarchy', 'plugin', 'enable', 'snzhy.backlight', '--after', 'omarchy.tray'],
            ['omarchy-shell', 'shell', 'rescanPlugins'],
        ):
            subprocess.run(command, check=True)
        print('Omarchy Backlight widget installed beside the tray; login startup enabled.')
    print(f'Installed app: {APP}')
    print(f'Open from the application menu or run: {LAUNCHER}')
    print('USB access: install packaging/70-snzhy-opensycnlights.rules as shown in INSTALL.md, then reconnect the light.')


if __name__ == '__main__':
    try:
        main()
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f'Installation failed: {error}', file=sys.stderr)
        sys.exit(1)
