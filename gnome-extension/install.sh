#!/bin/bash
EXT_DIR="$HOME/.local/share/gnome-shell/extensions/synclights@synclights.local"
mkdir -p "$EXT_DIR"
cp metadata.json extension.js "$EXT_DIR/"
echo "Installed to $EXT_DIR"
echo "Enable with: gnome-extensions enable synclights@synclights.local"
echo "Then restart GNOME Shell (Alt+F2 -> r -> Enter, or log out/in on Wayland)"
