# Presets and performance implementation plan

**Goal:** Expand the existing Linux USB controller to 60 useful scenes and 20 lighting effects, and reduce avoidable capture and interface work.

**Architecture:** Keep the existing Tauri commands and local USB owner. Store the scene catalog in a small frontend module, retain personal scene import/export, and extend screen settings with an optional capture scale. Preserve the existing studio interface and the workspace's audio changes.

**Tech stack:** Rust, Tauri 2, vanilla JavaScript modules, HTML/CSS, Playwright.

**Brief:** The user requested a wider preset selection, fuller application capabilities and improved performance. Existing flows are extended directly; installation and hardware operation are separate from source verification.

## Constraints and review focus

- Preserve existing scene IDs, LED layout, display/source selection, appearance and Stop/resume behavior.
- Built-in profiles must only replace scene settings; imported personal scenes must normalize invalid values.
- Search and favorite controls must work by keyboard and at desktop and narrow widths.
- Reduced-motion and static previews must settle; changes must still repaint immediately.
- Rapid slider events must persist the latest settings without queuing every intermediate disk write.
- Mode workers must stop before another mode writes USB frames; capture errors must remain visible.

## Tasks

- [ ] Add browser regressions for scene discovery, favorites, new-effect wiring, capture quality, persistence and idle preview work; observe the original app fail the new requirements.
- [ ] Add `ui/presets.js` with 32 lighting scenes, 20 audio scenes and eight screen profiles; expose search, mode filtering, favorites and result counts in `ui/main.js` and `ui/style.css`.
- [ ] Extend `src-tauri/src/effects.rs` with ten distinct effects, reusable frame buffers, finite parameter validation and joined worker cancellation; verify rendered behavior at small and maximum LED counts.
- [ ] Extend `src-tauri/src/ambilight.rs` with scaled capture, timing metrics, reusable smoothing buffers and safe temporary-file cleanup; verify pixel parsing and sampling at edge cases.
- [ ] Wire optional `capture_scale` in `src-tauri/src/main.rs` and `captureScale` in the UI. Expose economy, balanced, detailed and full capture plus live FPS/timing information.
- [ ] Coalesce native saves and shell-status publishing; stop repeated preview frames when static, reduced-motion or hidden.
- [ ] Run the full Rust suite, existing and new browser regressions, installer checks and a release build. Inspect desktop/narrow screenshots in one batch; fix any concrete defects and confirm once.
- [ ] Update usage, build and changelog documentation with supported settings and honest verification limits.
