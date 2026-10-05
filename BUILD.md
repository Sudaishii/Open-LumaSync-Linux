# Build and test

Use a current stable Rust toolchain and the locked dependencies in `src-tauri/Cargo.lock`. The frontend is plain HTML/CSS/JavaScript, embedded in the executable. Node.js is needed only for browser tests. The Tauri CLI is optional for development and packaging.

## Dependencies

The tested Arch / Omarchy dependencies are listed in [INSTALL.md](INSTALL.md). For a standalone build on Debian/Ubuntu:

```sh
sudo apt install build-essential pkg-config python3 libglib2.0-dev libgtk-3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev libhidapi-dev libudev-dev libpulse-dev libayatana-appindicator3-dev
```

These are build prerequisites, not a claim of tested GNOME or other desktop capture support. Use the capture tools and audio server appropriate to your desktop. Prebuilt `.deb` or AppImage releases are not currently provided.

## Build and run

```sh
cargo build --release --locked --manifest-path src-tauri/Cargo.toml
./run.sh
```

Output: `src-tauri/target/release/snzhy-OpenSycnlights`. `run.sh` builds only if the executable is absent; rebuild explicitly after changing source.

Install the resulting app with `./install.sh`, or add Omarchy integration with `./install.sh --omarchy`. Add `--offline` only when dependencies are cached.

## Checks

```sh
cargo test --release --locked --manifest-path src-tauri/Cargo.toml
node --check ui/main.js
python3 tests/install-check.py
```

The installer check uses a temporary home and mocked desktop/build commands; it does not change the live bar, service or USB rules. It verifies portable paths, independent installed files, standalone/Omarchy modes, fresh-build behavior and updates.

Browser regressions require Playwright and Chromium:

```sh
npm install --no-save --package-lock=false playwright
npx playwright install chromium
node tests/ui-check.cjs
```

They cover controller state, resume, native persistence, scenes, responsive layouts and mocked command wiring. They do not prove physical hardware behavior.

`tests/audio-live-check.cjs`, `tests/sync-live-check.cjs` and `tests/shell-live-check.py` drive real hardware or desktop capture. Read each script before running: they change lighting, may show fullscreen colors/play tones, and require the corresponding desktop environment. The shell test enables resume and selects `DP-1`; adjust it for your own monitor. Do not run isolated USB diagnostics alongside another controller process.

The audio check accepts `SYNC_AUDIO_MODE`, `SYNC_AUDIO_PALETTE`, and `SYNC_AUDIO_FREQUENCY` (20–20000 Hz). To check volume changes during continuous background audio, run it with `SYNC_AUDIO_CONTINUOUS=1 SYNC_AUDIO_SENSITIVITY=5 SYNC_AUDIO_MODE=energy SYNC_AUDIO_PALETTE=selected`. This compares steady-state brightness during background-only and louder playback segments; it does not infer physical brightness from USB write success.

## Optional Tauri development and bundles

```sh
cargo install tauri-cli --version '^2' --locked
cargo tauri dev
cargo tauri build
```

Bundle targets are configured in `src-tauri/tauri.conf.json`. Packaging configuration exists, but generated installers need independent validation and USB-permission documentation before being advertised as supported downloads.

## Structure

| Path | Role |
|---|---|
| `ui/` | Interface, local fonts and settings controls |
| `src-tauri/src/` | USB protocol, lighting/audio workers, capture, native persistence and D-Bus |
| `packaging/install.py` | Portable user installation |
| `packaging/omarchy/` | Dedicated widget, service template and bridge |
| `tests/` | Rust-adjacent browser, installer and live-device checks |
| `docs/images/` | Public documentation images |

The inherited `gnome-extension/` is legacy upstream material and is not integrated with this derivative's D-Bus identity. Do not use it as a supported installation route.
