# Installation

## Pre-built .deb (Debian/Ubuntu)

Download the latest `.deb` from the [Releases](../../releases) page, then:

```bash
sudo dpkg -i openLightsSync_0.1.0_amd64.deb
sudo apt install -f  # install any missing dependencies
```

## Runtime dependencies

The `.deb` package declares these automatically, but if running from a manual build:

```bash
# Debian/Ubuntu
sudo apt install libhidapi-hidraw0 libpulse0 libayatana-appindicator3-1

# Fedora
sudo dnf install hidapi pulseaudio-libs libayatana-appindicator-gtk3

# Arch
sudo pacman -S hidapi libpulse libayatana-appindicator
```

### Optional (Screen Sync)

Screen Sync requires a screen capture tool:

- **wlroots compositors** (Sway, Hyprland): install `grim`
- **GNOME on Wayland**: `gnome-screenshot` is used automatically (lower FPS due to screenshot overhead)

> Screen Sync is experimental and disabled by default. Enable it in Settings.

## USB Permissions (udev)

USB HID devices require a udev rule to allow non-root access:

```bash
sudo tee /etc/udev/rules.d/99-synclights.rules << 'EOF'
# SyncLight Bar (HID)
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="1a86", ATTRS{idProduct}=="fe07", MODE="0666"
# SyncLight Bar (CDC)
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="1a86", ATTRS{idProduct}=="fe0c", MODE="0666"
EOF

sudo udevadm control --reload-rules
sudo udevadm trigger
```

Unplug and replug the device after applying the rule.

## GNOME Shell Extension (optional)

Adds a Quick Settings toggle for power control in the GNOME panel. Supports GNOME 45-48.

```bash
cd gnome-extension
./install.sh
gnome-extensions enable synclights@synclights.local
```

**Important:** On Wayland, GNOME Shell does not detect newly installed extensions until you log out and log back in. After re-login the extension will appear in the Quick Settings panel and `gnome-extensions enable` will work.

On X11 you can restart the shell without logging out: Alt+F2, type `r`, press Enter.

## Running

After installation, launch from the application menu or run:

```bash
openLightsSync
```

The app minimizes to the system tray on close. A second launch will reopen the existing window (single-instance enforcement).
