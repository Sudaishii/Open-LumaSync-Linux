# Use the backlight controller

## Controller and power

Open **snzhy-OpenSycnlights** from your application menu, or choose **Open controller** in the Omarchy Backlight popup. Confirm that the light is connected before starting a mode.

Choose **Lighting**, **Screen sync** or **Audio sync**, then press **Start selected mode**. Selecting a mode prepares it; opening a settings page keeps the running mode active. The status line shows the real capture source or worker state. The illustrative LED diagram does not show captured pixels or hardware telemetry.

**Stop** ends animation/capture and leaves the most recent light frame. **Turn off** stops the mode and turns the strip off. Turning on starts the selected mode. Brightness applies across modes; starting at zero restores the last nonzero brightness.

Enable **Resume running mode on launch** to restore the last successfully running mode and its settings. Stop and Turn off cancel the next resume. The Omarchy service starts at login even if resume is off; in that case a mode must be started manually. Closing the window keeps the process running; **Quit** in the tray exits it.

## LED layout

Specify the LED count along the left, top and right edges of your monitor. Match the actual mounting and direction. A 24-inch monitor does not imply a particular LED count: strips, controller capacity and mounting differ.

The tested device reports 54 LEDs, but other units may differ. The app constrains the total against reported capacity when available. Test individual LEDs to establish direction and adjust the split. Incorrect counts or direction can make Screen sync's colors appear on the wrong side.

## Screen sync

1. Select the monitor behind your backlight. On Hyprland, names such as `DP-1` and `HDMI-A-1` are capture output IDs.
2. Select a screen preset or set capture rate, capture quality, smoothing and sample depth.
3. Start Screen sync and move between visibly different screen content.

Colors are sampled from the display edges; the mode has no manual color picker. Brightness still controls the overall light output. **Reverse LED order** changes how sampled colors map along the strip.

**Focused display when sync starts** selects an output at startup; it does not continuously follow window focus. Select a specific monitor to keep capture tied to that monitor. Display and sampling changes automatically restart active capture with the new settings.

| Profile | Target FPS | Smoothing | Edge depth |
|---|---:|---:|---:|
| Balanced | 15 | 35% | 60 px |
| Gaming | 30 | 15% | 24 px |
| Cinema | 12 | 75% | 120 px |
| Low CPU | 8 | 50% | 32 px |

Capture quality scales the screenshot before processing: 0.2× uses the least CPU, 0.35× is balanced, 0.5× keeps more detail and 1× captures full size. The status line reports measured FPS plus capture, processing and USB write timings. The target is not a guarantee; `grim` screenshot capture and USB overhead vary by machine. Screen sync is experimental.

## Audio sync

Choose a **Playback / Monitor of …** entry to react to audio playing through that output. **Current playback device (automatic)** chooses a source when the worker starts. Input/microphone entries listen to an input device. The microphone inside the physical backlight controller is a separate firmware feature, not a computer output source.

Play music and watch the signal percentage. **Refresh** retries source discovery if needed. Choose a style and palette separately:

| Style | Response |
|---|---|
| Bounce | A colored band travels back and forth with sound |
| Spectrum | Frequency energy shapes the strip |
| Energy | Overall volume changes light energy |
| Beat | Color spreads from the center with sound |
| Comet | A colored head and tail wrap around the strip |
| Twin bounce | Two bands move in mirrored directions |
| Ripple | A colored ring moves outward from the center |
| Volume bars | Bars grow from the center with volume |
| Ribbon wave | Traveling ribbons brighten with audio |
| Pulse | The full strip breathes with the signal |
| Swell | A soft illuminated region grows and contracts |
| Sparks | Small points of light appear with louder moments |
| Prism | A multi-color shimmer travels along the strip |
| Tremor | Fine brightness movement follows the signal |
| Orbit | A focused ring circles the strip |

Rainbow, Aurora, Sunset, Ocean, Neon arcade, Ember, Forest and Candy provide multicolor effects. **Custom blend** combines the primary and audio secondary colors. **Selected color** deliberately uses one primary color. Choose a multicolor palette if you want changing colors.

Adjust sensitivity and movement speed. **Shape the response** includes trail/band width, a gate that ignores quiet signals and reversed direction. Some styles use width differently. Silence or gating can dim or darken the strip while capture stays active. Ripple and Beat respond to sound; they do not claim BPM detection. Changes to audio settings restart the active worker automatically.

For continuous background music, start with **Bounce** or **Volume bars**, sensitivity **1.5**, and a quiet-sound gate of **0%**. A Bounce trail width around **24%** makes its movement easier to see. Raising the gate ignores more quiet music; it does not increase responsiveness. Adjust the master brightness to suit your room.

Spectrum separates eight frequency ranges. Frequencies close to a range boundary can brighten both neighboring ranges; its frequency resolution is about **11 Hz**.

## Lighting and presets

Lighting provides 20 modes, including Static, Rainbow, Pulse, Chase, Aurora, Ocean currents, Color sweep, Scanner, Meteor, Twinkle, Color blossoms and Rainbow wave. Manual colors apply where the effect uses them; palette effects generate their own colors.

In **Presets**, search 60 built-in scenes by name or tag, filter by lighting/audio/screen/favorites/personal, star favorites, load a scene to prepare its settings or use **Run now** to start it. Built-in presets retain your LED layout, selected display/source and direction. Personal scenes include their saved configuration, so check the layout when importing scenes from another setup.

Save personal scenes and use JSON **Export** / **Import** to move them between machines. Theme, accents, compact controls and reduced motion are available in **Settings**.

## Dedicated Omarchy plugin

The **Backlight** icon opens controls for modes, Stop, power, brightness, monitor selection, Balanced/Gaming profiles, resume at login and the app window. The popup uses Omarchy's theme. Both interfaces control one engine and serialize commands; the popup does not open a second device connection.

The local command bridge is also available:

```sh
~/.local/bin/snzhy-backlight status
~/.local/bin/snzhy-backlight screen
~/.local/bin/snzhy-backlight audio
~/.local/bin/snzhy-backlight stop
~/.local/bin/snzhy-backlight show
```

A command's `accepted` reply means it reached the controller queue. Check subsequent status for the running mode or an error. If the controller was quit, a bar action starts the user service again.
