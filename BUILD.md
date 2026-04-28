# Building from source

## Prerequisites

- **Rust toolchain** (stable, 1.70+): https://rustup.rs
- **Tauri CLI v2**:
  ```bash
  cargo install tauri-cli --version "^2"
  ```

## Build dependencies

```bash
# Debian/Ubuntu
sudo apt install \
  build-essential \
  libglib2.0-dev \
  libgtk-3-dev \
  libwebkit2gtk-4.1-dev \
  libsoup-3.0-dev \
  libhidapi-dev \
  libpulse-dev \
  libayatana-appindicator3-dev \
  libjavascriptcoregtk-4.1-dev

# Fedora
sudo dnf install \
  gcc gcc-c++ \
  glib2-devel \
  gtk3-devel \
  webkit2gtk4.1-devel \
  libsoup3-devel \
  hidapi-devel \
  pulseaudio-libs-devel \
  libayatana-appindicator-gtk3-devel \
  javascriptcoregtk4.1-devel

# Arch
sudo pacman -S \
  base-devel \
  glib2 \
  gtk3 \
  webkit2gtk-4.1 \
  libsoup3 \
  hidapi \
  libpulse \
  libayatana-appindicator
```

## Build commands

```bash
# Development (debug, with hot reload of the UI)
cargo tauri dev

# Release build (optimized binary + bundles)
cargo tauri build

# .deb package only
cargo tauri build --bundles deb
```

## Output

| Artifact | Path |
|----------|------|
| Binary | `src-tauri/target/release/openLightsSync` |
| .deb | `src-tauri/target/release/bundle/deb/openLightsSync_0.1.0_amd64.deb` |

Install the .deb with:
```bash
sudo dpkg -i src-tauri/target/release/bundle/deb/openLightsSync_0.1.0_amd64.deb
```

## Notes

- AppImage bundling may fail in headless environments; the .deb target is reliable.
- The frontend is plain HTML/CSS/JS in the `ui/` folder — no Node.js or npm required.
- `cargo tauri dev` serves the UI from `ui/` directly with hot reload.
