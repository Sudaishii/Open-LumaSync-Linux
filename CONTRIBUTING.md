# Contributing

This project is a Linux backlight controller adapted from openLightsSync. Contributions should keep upstream attribution and the CC BY-NC-SA 4.0 license. Omarchy/Hyprland and the USB HID SyncLight controller are the primary tested environment.

For bugs, provide device IDs, distribution/desktop versions, mode/source, reproduction steps and the actual status or error. Distinguish a capture problem from a USB or physical layout problem. Never include passwords, session tokens or unrelated desktop content.

For changes, explain the user-visible behavior and how it was checked. Follow [BUILD.md](BUILD.md). Run Rust tests for controller changes, browser checks for interface behavior and installer checks for packaging changes. Physical results must be labeled separately from mocked tests. Do not advertise support for an untested device or desktop.

Use pull requests against the published repository. Changes to USB framing should cite [STOCK-PROTOCOL.md](STOCK-PROTOCOL.md) or a reproducible device observation.
