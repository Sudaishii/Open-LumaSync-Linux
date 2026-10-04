#!/usr/bin/env bash
set -euo pipefail
controller_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
controller_bin="$controller_root/src-tauri/target/release/snzhy-OpenSycnlights"
if [[ ! -x "$controller_bin" ]]; then
  cargo build --release --locked --manifest-path "$controller_root/src-tauri/Cargo.toml"
fi
# WebKit DMA-BUF surfaces trigger a Wayland protocol error on this machine.
# XWayland is used for the window; grim still captures the Wayland desktop.
export WEBKIT_DISABLE_DMABUF_RENDERER="${WEBKIT_DISABLE_DMABUF_RENDERER:-1}"
export GDK_BACKEND="${GDK_BACKEND:-x11}"
exec "$controller_bin" "$@"
