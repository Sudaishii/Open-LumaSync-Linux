#!/usr/bin/env bash
set -euo pipefail
project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
install_mode=standalone
cargo_flags=(--locked)
for argument in "$@"; do
  case "$argument" in
    --omarchy) install_mode=omarchy ;;
    --offline) cargo_flags+=(--offline) ;;
    --help|-h)
      cat <<'HELP'
Usage: ./install.sh [--omarchy] [--offline]

Build and install the Linux app for your user, with an application-menu entry.
  --omarchy  Also install the Backlight bar widget and enable login startup.
  --offline  Build only from already cached Cargo dependencies.

Install system dependencies and the USB rule as described in INSTALL.md first.
Quit standalone app instances before installation. Do not run this script as root.
HELP
      exit 0 ;;
    *) printf 'Unknown option: %s\n' "$argument" >&2; exit 2 ;;
  esac
done
if (( EUID == 0 )); then
  echo 'Run the installer as your desktop user, without sudo.' >&2
  exit 1
fi
for command in cargo python3 pkg-config; do
  command -v "$command" >/dev/null || { echo "Missing $command. See INSTALL.md for dependencies." >&2; exit 1; }
done
if [[ "$install_mode" == omarchy ]]; then
  for command in omarchy omarchy-shell systemctl gdbus; do
    command -v "$command" >/dev/null || { echo "Missing $command. The plugin requires an Omarchy Quickshell session." >&2; exit 1; }
  done
  systemctl --user show-environment >/dev/null
  omarchy plugin validate "$project_root/packaging/omarchy"
fi
printf 'Building snzhy-OpenSycnlights (%s installation)…\n' "$install_mode"
cargo build --release "${cargo_flags[@]}" --manifest-path "$project_root/src-tauri/Cargo.toml"
python3 "$project_root/packaging/install.py" "$install_mode"
