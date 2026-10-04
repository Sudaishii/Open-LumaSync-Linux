# Install snzhy-OpenSycnlights

The tested route is a source installation on Arch Linux / Omarchy. This repository does not currently provide an AUR package or verified prebuilt release. Node.js and the Tauri CLI are not required to build or run the application.

## 1. Get the source

Download and extract the repository's source ZIP from GitHub, or clone your published repository:

```sh
git clone https://github.com/Sudaishii/Open-LumaSync-Linux.git
cd Open-LumaSync-Linux
```

Run the following commands inside that project folder.

## 2. Install dependencies

On **Omarchy**:

```sh
omarchy pkg add git rust base-devel pkgconf python glib2 gtk3 webkit2gtk-4.1 libsoup3 hidapi libpulse libayatana-appindicator grim xorg-xwayland
```

On **Arch Linux**:

```sh
sudo pacman -S --needed git rust base-devel pkgconf python glib2 gtk3 webkit2gtk-4.1 libsoup3 hidapi libpulse libayatana-appindicator grim xorg-xwayland
```

Audio needs a working PulseAudio-compatible server, normally PipeWire with `pipewire-pulse` on Omarchy. `glib2` supplies `gdbus` for the plugin bridge. Use a current stable Rust toolchain. Other distributions' build dependencies are listed in [BUILD.md](BUILD.md); their desktop integration is not validated here.

## USB access

Install the supplied udev rule once:

```sh
sudo install -m 0644 packaging/70-snzhy-opensycnlights.rules /etc/udev/rules.d/70-snzhy-opensycnlights.rules
sudo udevadm control --reload-rules
```

Unplug and reconnect the USB backlight. The rule grants access to the active local desktop session using `uaccess`. It does not grant access to every user. The rule covers the supported device IDs; the CDC variant is not independently validated. Run the app as your normal desktop user, without sudo.

## 3. Choose an installation

Quit any standalone app instance from its tray menu first. Updating an already-installed Omarchy service stops it before replacing its executable.

### Omarchy app + dedicated bar plugin

```sh
./install.sh --omarchy
```

This builds the release binary, installs an app-menu launcher, backs up your `shell.json`, places `snzhy.backlight` beside the tray and enables the user service at login. It uses your installed Omarchy theme and does not edit `/usr/share/omarchy`.

Once installed, click the **Backlight** icon to open its popup, then choose **Open controller**. Enable **Resume at login** to restore a mode you leave running. Login startup and mode restoration are separate: the service starts with the desktop, while resume is off by default until you enable it.

### Standalone application

```sh
./install.sh
~/.local/bin/snzhy-opensycnlights
```

This installs the app and menu entry without adding an Omarchy widget or service. Closing the app window leaves it in the tray; it will not automatically start at login in this mode.

### Build and run without installation

```sh
cargo build --release --locked --manifest-path src-tauri/Cargo.toml
./run.sh
```

### Offline builds

The first build downloads Rust dependencies. After dependencies are cached:

```sh
./install.sh --omarchy --offline
```

An offline build cannot fetch missing crates. Use the normal command for a new machine.

## Installed files

| Purpose | Location |
|---|---|
| App executable, license and attribution | `${XDG_DATA_HOME:-~/.local/share}/snzhy-opensycnlights/` |
| App launcher | `~/.local/bin/snzhy-opensycnlights` |
| Application-menu entry and icons | `${XDG_DATA_HOME:-~/.local/share}/applications/` and `icons/` |
| Omarchy widget | `${XDG_CONFIG_HOME:-~/.config}/omarchy/plugins/snzhy.backlight/` |
| Bar bridge | `~/.local/bin/snzhy-backlight` |
| Login service | `${XDG_CONFIG_HOME:-~/.config}/systemd/user/snzhy-backlight.service` |
| Hardware and controller settings | `${XDG_CONFIG_HOME:-~/.config}/snzhy-opensycnlights/` |

The installed binary contains the frontend; the service points to the installed launcher. You may move or remove the source checkout after installation. Keep it if you want to update from source.

If `~/.local/bin` is not on your PATH, use the full commands shown above or add that directory to your shell's PATH. Personal presets use the app's local WebView storage; export them from **Presets** before changing machines.

## Update

Get the latest source from your project's repository, then repeat the installation command:

```sh
git pull --ff-only
./install.sh --omarchy
```

Use `./install.sh` for a standalone installation, after quitting the old app. Existing settings and personal presets are retained. If the bar keeps an older widget after an update, try `omarchy-shell shell rescanPlugins`; if it still sticks, `omarchy restart shell` clears the QML cache. The separate backlight service continues running during a shell restart.

## Uninstall

Quit the app. For an Omarchy installation, first disable the widget and service:

```sh
omarchy plugin disable snzhy.backlight
systemctl --user disable --now snzhy-backlight.service
```

Then remove only this project's installed files:

```sh
rm -rf -- "${XDG_DATA_HOME:-$HOME/.local/share}/snzhy-opensycnlights" "${XDG_CONFIG_HOME:-$HOME/.config}/omarchy/plugins/snzhy.backlight"
rm -f -- "$HOME/.local/bin/snzhy-opensycnlights" "$HOME/.local/bin/snzhy-backlight" "${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/snzhy-backlight.service" "${XDG_DATA_HOME:-$HOME/.local/share}/applications/com.snzhy.opensycnlights.desktop"
rm -f -- "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/32x32/apps/snzhy-opensycnlights.png" "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/128x128/apps/snzhy-opensycnlights.png"
systemctl --user daemon-reload
```

Settings, exported scenes and shell-config backups are kept. The USB rule can remain for other controllers. To remove it too:

```sh
sudo rm -f /etc/udev/rules.d/70-snzhy-opensycnlights.rules
sudo udevadm control --reload-rules
```

If installation or first use fails, see [TROUBLESHOOTING.md](TROUBLESHOOTING.md).
