# Official SyncLight protocol reference

Reference: official Windows SyncLight 3.15.0, extracted locally from the Robobloq download. Protocol facts were independently implemented; no official program code or assets are bundled.

## USB transport

The stock app selects Robobloq HID interface 0. Our device's vendor usage page is `0xff00` (`1a86:fe07`). Each write prepends report ID zero to the actual packet bytes. Packets longer than 64 bytes are split into consecutive 64-byte chunks, each with its own zero report ID. A complete frame must be serialized so different workers cannot interleave its chunks.

## Packets

RB: `R B length:u8 id:u8 action:u8 payload checksum:u8`.
Screen SC: `S C length:u16BE id:u8 action:0x80 payload checksum:u8`.
Both use byte-sum modulo 256; keyboard/mouse SC CRC framing is a different protocol. Length includes header and checksum. Stock IDs begin at 2 and cycle through 1–254.

LED segments are `[start, red, green, blue, end]`, one-based and inclusive. Whole-strip static lighting covers 1 through the configured count and clears the remaining range through 254. Screen frames carry all segments in one SC packet.

| Command | Action | Payload |
|---|---|---|
| Device information | `0x82` | empty |
| Firmware effect | `0x85` | type, index |
| Static LED segments | `0x86` | five-byte segments |
| Brightness | `0x87` | brightness |
| Automatic off | `0x89` | enabled, duration |
| Dynamic speed | `0x8a` | speed (stock slider is inverted) |
| Sound sensitivity | `0x8b` | sensitivity |
| Open URL setting | `0x93` | enabled |
| LED count | `0x95` | count |
| Turn off light | `0x97` | empty |

**0x97 is never a keepalive.** The inherited three-second background sender caused lights to shut down after Apply. It has been removed, including the pre-color probe.

Stock device information matches the response ID; model bytes are 5–7, LED count byte 11, firmware bytes 21–23. The controller requests this information and matches header, ID, action, length and field bounds with a bounded timeout. Device-info replies on firmware 1.9.4 have a zero trailing byte; the stock reader does not require the request checksum on those replies. Lighting writes still do not prove a physical change.

## Scope of parity

USB framing, static ranges, brightness, and screen packet format now follow the official protocol. Firmware is now detected; versions above 1.0.2 use SC frames, while unknown/older firmware uses compatible RB segments. Oversized default layouts are reduced to the reported LED count. Existing named custom effects remain host-generated; they are not claimed to be identical firmware effects. Official renderer offers dynamic type 2, indices 0–6; visual names/mappings are not assumed. Firmware effects, automatic-off configuration and onboard microphone modes are not yet exposed in our interface.

Linux screen/audio capture uses grim/PulseAudio, since the stock app's platform capture components cannot run natively here. Primary-color screen matching has been physically confirmed by the user; final multicolor audio behavior awaits in-app visual confirmation.

## Static-mode brightness refresh

Official brightness controller sends `setBrightness`, waits 20ms, then resends `setSectionLED` when the device is in static lighting rather than a firmware effect or screen sync. Energy's uniform-color brightness response now follows this sequence; brightness-only writes previously left physical output steady despite changing captured signal levels.

## Stock feature comparison

Inspected the stock renderer and main process as well as USB framing. The stock audio view exposes seven rhythm effect indices (type 3, indices 0–6), selectable computer capture or controller microphone, and per-effect rhythm colors. Those indices are not mapped to invented names. Our nine audio styles are independently implemented and are not claimed to reproduce those seven algorithms exactly.

| Feature | Linux controller status |
|---|---|
| USB connection, static colors, brightness | Implemented using stock packet framing and static refresh sequence |
| Screen synchronization | Implemented using Linux capture and stock SC frames; primary-color physical matching confirmed |
| Computer playback synchronization | Implemented with PulseAudio/PipeWire monitor capture; nine custom styles and ten palettes |
| Controller microphone synchronization | Stock supports it; not exposed in our app |
| Stock dynamic/rhythm effects | Protocol identified; our named effects are host-generated, not exact stock replicas |
| Automatic-off duration | Stock command identified; not exposed in our app |
| Screen mounting/layout controls | Left/top/right layout, reverse order, display, sample depth and smoothing available; physical placement needs calibration |

Matching the transport ensures compatible commands, not identical visual algorithms or complete feature parity.
