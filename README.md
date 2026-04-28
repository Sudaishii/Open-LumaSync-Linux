# openLightsSync

Native Linux controller for USB LED light bars (SyncLight / iCUE compatible). No official Linux support exists for these devices — this app fills that gap with a full-featured Tauri v2 desktop application.

## Supported Devices

| Vendor ID | Product ID | Description |
|-----------|------------|-------------|
| `0x1a86`  | `0xfe07`   | SyncLight Bar (HID) |
| `0x1a86`  | `0xfe0c`   | SyncLight Bar (CDC variant) |

## Features

- **Full RGB color control** — Spectrum picker with real-time preview, hex/RGB input
- **Centralized color & brightness** — Global color shared across all modes, changes apply instantly
- **10 built-in modes** — Static, Rainbow, Pulse, Chase, Chase Bounce, Breathe, Fire, Wave, Sparkle, Heartbeat
- **Audio sync** — 3 visualization modes (Spectrum, Energy, Beat) with selectable PulseAudio source
- **Screen Sync** — Screen border color sampling for ambient lighting (experimental, requires `grim`)
- **System tray** — Minimize to tray, toggle power, quick access
- **Single instance** — Second launch reopens the existing window
- **GNOME Shell extension** — Quick Settings toggle (GNOME 45-48)
- **D-Bus interface** — `com.synclights.gui` for scripting and extension communication
- **Auto-reconnect** — Automatic device detection and connection recovery
- **State persistence** — Saves color, brightness, layout between sessions
- **Dark theme** — Adwaita-inspired dark UI that integrates with GNOME/GTK

## Quick Start

See **[INSTALL.md](INSTALL.md)** for installation instructions (pre-built .deb, runtime dependencies, udev rules, GNOME extension).

See **[BUILD.md](BUILD.md)** for building from source (build dependencies, Tauri CLI, build commands).

## Screenshots

*TODO: Add screenshots*

## Project Structure

```
openLightsSync/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs         # Tauri commands, tray, window management
│   │   ├── hid.rs          # USB HID protocol (RB/SC frames, per-LED control)
│   │   ├── effects.rs      # LED mode engine (10 modes)
│   │   ├── audio.rs        # Audio capture + visualization (PulseAudio)
│   │   ├── ambilight.rs    # Screen capture + border sampling
│   │   ├── dbus.rs         # D-Bus server for GNOME extension
│   │   └── state.rs        # State persistence (~/.config/synclights/)
│   ├── capabilities/       # Tauri v2 security permissions
│   ├── icons/              # App icons (SVG + PNG)
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── build.rs
├── ui/
│   ├── index.html
│   ├── style.css           # Adwaita-inspired dark theme
│   ├── main.js
│   └── color-picker.js     # Custom spectrum color picker
├── gnome-extension/
│   ├── metadata.json
│   ├── extension.js
│   └── install.sh
├── INSTALL.md
├── BUILD.md
├── LICENSE                  # CC BY-NC-SA 4.0
└── README.md
```

## HID Protocol Notes

The LED bar uses a custom USB HID protocol over vendor-specific endpoints:

- **RB frames** — 64-byte packets with XOR checksum, used for color/brightness/power commands
- **SC frames** — 64-byte packets with CRC16-CCITT, used for per-LED segment data
- **Per-LED segments** — Action `0x86`, data format: `[startIdx, R, G, B, endIdx]` (1-based), 55-byte chunks with 20ms inter-frame delay
- **Keepalive** — Action `0x97` every 3 seconds to prevent firmware idle timeout

## D-Bus Interface

Interface: `com.synclights.gui` at `/com/synclights/gui`

| Method | Description |
|--------|-------------|
| `TogglePower` | Toggle the LED bar on/off |
| `SetBrightness(u8)` | Set brightness (0-255) |
| `GetStatus` | Returns JSON with connection and brightness info |
| `ShowWindow` | Bring the app window to front |

## License

[CC BY-NC-SA 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/) — See [LICENSE](LICENSE).
