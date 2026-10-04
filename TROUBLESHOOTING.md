# Troubleshooting

## The light is not found

Connect the backlight's USB controller, not just a Wi-Fi connection. This project supports specific Robobloq USB IDs. If available, `lsusb` helps identify your device. The tested HID ID is `1a86:fe07`; `1a86:fe0c` is inherited support, not a confirmed device test.

If the app says **USB permission needed**, install the rule from [INSTALL.md](INSTALL.md#usb-access), reload the rules, unplug/reconnect the device and click **Reconnect**. Do not run the app as root. A logged-in local desktop session is needed for `uaccess`.

Close other light-control applications. One process should own the strip; the bar and app already share the same engine.

## Audio outputs are missing or there is no response

Check the desktop's audio service and retry **Refresh**. On Omarchy, PipeWire normally provides PulseAudio-compatible playback monitors:

```sh
pactl info
pactl list short sources
```

Choose the monitor of the output actually playing music. A microphone entry will not capture computer playback. If the audio source disappears after changing outputs, choose the new output monitor and restart Audio sync.

If the signal percentage changes but the strip looks steady, try **Bounce** or **Volume bars**, a multicolor palette and a lower quiet-sound gate. **Selected color** uses one color by design. Silence can darken effects without stopping capture. Firmware's built-in microphone mode is separate from software audio capture.

## Screen sync is wrong, static or stopped

Select the monitor behind the strip, and check LED counts and direction. Hyprland output IDs are listed by:

```sh
hyprctl monitors -j
```

Screen capture uses `grim`, which must be installed and run in the graphical session. Desktop permissions or capture errors appear in the app's status. Choose a lower target FPS or the Low CPU profile if performance is poor. Dark or uniform screen edges can legitimately produce dark or uniform lighting.

The app samples left/top/right edges, not the whole screen's average. Focused-display selection happens when sync starts. Other desktops and protected content can have different capture restrictions; Hyprland is the primary tested environment.

## Omarchy widget or login service is missing

The widget needs Omarchy's Quickshell plugin system. It is not a Waybar configuration snippet. Install with `./install.sh --omarchy` in your normal desktop session.

```sh
systemctl --user status snzhy-backlight.service
journalctl --user -u snzhy-backlight.service -n 50 --no-pager
~/.local/bin/snzhy-backlight status
omarchy-shell shell rescanPlugins
```

If the shell retains a stale widget after an update, `omarchy restart shell` clears its QML cache. The separate backlight service continues running during a shell refresh. Do not use `omarchy refresh shell`, which resets desktop configuration.

Startup and resume are separate. Enable **Resume at login**, start a mode successfully and leave it running. Stop and Turn off intentionally cancel the next resume. Capture errors also cancel resume; fix the source and start the mode again.

## The window does not open

Start it from a terminal to see errors:

```sh
~/.local/bin/snzhy-opensycnlights
```

The launcher defaults to XWayland (`GDK_BACKEND=x11`) and disables WebKit DMA-BUF rendering because that combination worked on the tested desktop. Install `xorg-xwayland` on Arch/Omarchy. Advanced users can override these environment variables when diagnosing another desktop; this does not change screen capture's Wayland output source.

## Save a useful bug report

Include your distribution, desktop/Omarchy version, USB vendor/product IDs, firmware/LED count if known, selected mode/source and exact reproduction steps. Copy the relevant error and service log. Avoid posting private desktop captures or personal configuration. See [CONTRIBUTING.md](CONTRIBUTING.md).
