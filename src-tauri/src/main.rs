#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ambilight;
mod audio;
mod dbus;
mod effects;
mod hid;
mod state;

use ambilight::Ambilight;
use audio::AudioSync;
use effects::EffectRunner;
use hid::{HidController, LedColor};
use state::{load_state, save_state};
use std::sync::Arc;
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    Manager,
};

struct AppHid(Arc<HidController>);
struct AppEffects(Arc<EffectRunner>);
struct AppAudio(Arc<AudioSync>);
struct AppAmbilight(Arc<Ambilight>);

// ── Tauri commands ──────────────────────────────────────────────────

#[tauri::command]
fn get_sections(hid: tauri::State<AppHid>) -> Result<serde_json::Value, String> {
    let sections = *hid.0.sections.lock().unwrap();
    let total: u16 = sections.iter().sum();
    Ok(serde_json::json!({
        "sections": sections,
        "totalLeds": total
    }))
}

#[tauri::command]
fn set_sections(hid: tauri::State<AppHid>, sections: [u16; 3]) -> Result<serde_json::Value, String> {
    let total: u16 = sections.iter().sum();
    hid.0.set_total_leds(total);
    *hid.0.sections.lock().unwrap() = sections;
    let mut st = load_state();
    st.sections = Some(sections);
    save_state(&st);
    Ok(serde_json::json!({"ok": true, "totalLeds": total}))
}

#[tauri::command]
fn set_color_brightness(
    hid: tauri::State<AppHid>,
    fx: tauri::State<AppEffects>,
    audio: tauri::State<AppAudio>,
    ambi: tauri::State<AppAmbilight>,
    section: u8,
    r: u8,
    g: u8,
    b: u8,
    brightness: u8,
) -> Result<serde_json::Value, String> {
    fx.0.stop();
    audio.0.stop();
    ambi.0.stop();
    hid.0.set_global_color(LedColor { r, g, b });
    hid.0.set_color_and_brightness(section, r, g, b, brightness)?;
    let mut st = load_state();
    st.section = section;
    st.r = r;
    st.g = g;
    st.b = b;
    st.brightness = Some(brightness);
    save_state(&st);
    Ok(serde_json::json!({"ok": true}))
}

#[tauri::command]
fn set_color(
    hid: tauri::State<AppHid>,
    fx: tauri::State<AppEffects>,
    audio: tauri::State<AppAudio>,
    ambi: tauri::State<AppAmbilight>,
    section: u8,
    r: u8,
    g: u8,
    b: u8,
) -> Result<serde_json::Value, String> {
    fx.0.stop();
    audio.0.stop();
    ambi.0.stop();
    hid.0.set_global_color(LedColor { r, g, b });
    hid.0.set_color(section, r, g, b)?;
    let mut st = load_state();
    st.section = section;
    st.r = r;
    st.g = g;
    st.b = b;
    save_state(&st);
    Ok(serde_json::json!({"ok": true}))
}

#[tauri::command]
fn update_global_color(
    hid: tauri::State<AppHid>,
    r: u8,
    g: u8,
    b: u8,
    apply: Option<bool>,
) -> Result<serde_json::Value, String> {
    hid.0.set_global_color(LedColor { r, g, b });
    if apply.unwrap_or(false) {
        let _ = hid.0.set_color(1, r, g, b);
    }
    let mut st = load_state();
    st.r = r;
    st.g = g;
    st.b = b;
    save_state(&st);
    Ok(serde_json::json!({"ok": true}))
}

#[tauri::command]
fn set_brightness(
    hid: tauri::State<AppHid>,
    fx: tauri::State<AppEffects>,
    audio: tauri::State<AppAudio>,
    ambi: tauri::State<AppAmbilight>,
    value: u8,
) -> Result<serde_json::Value, String> {
    let has_active = fx.0.is_running() || audio.0.is_running() || ambi.0.is_running();
    if has_active {
        hid.0.set_brightness_only(value)?;
    } else {
        hid.0.set_brightness(value)?;
    }
    let mut st = load_state();
    st.brightness = Some(value);
    save_state(&st);
    Ok(serde_json::json!({"ok": true}))
}

#[tauri::command]
fn set_single_led(hid: tauri::State<AppHid>, index: usize, r: u8, g: u8, b: u8) -> Result<serde_json::Value, String> {
    hid.0.set_single_led(index, r, g, b)?;
    let mut st = load_state();
    let total = hid.0.get_total_leds() as usize;
    let mut per_led = st.per_led_state.unwrap_or_default();
    per_led.resize(total, LedColor::default());
    per_led[index] = LedColor { r, g, b };
    st.per_led_state = Some(per_led);
    save_state(&st);
    Ok(serde_json::json!({"ok": true}))
}

#[tauri::command]
fn power_on(
    hid: tauri::State<AppHid>,
    section: u8,
    r: u8,
    g: u8,
    b: u8,
    brightness: Option<u8>,
) -> Result<serde_json::Value, String> {
    hid.0.power_on(section, r, g, b, brightness)?;
    let mut st = load_state();
    st.section = section;
    st.r = r;
    st.g = g;
    st.b = b;
    save_state(&st);
    Ok(serde_json::json!({"ok": true}))
}

#[tauri::command]
fn power_off_fade(
    hid: tauri::State<AppHid>,
    fx: tauri::State<AppEffects>,
    audio: tauri::State<AppAudio>,
    ambi: tauri::State<AppAmbilight>,
    current_brightness: Option<u8>,
    duration_ms: Option<u64>,
    section: u8,
    r: u8,
    g: u8,
    b: u8,
) -> Result<serde_json::Value, String> {
    // Stop all active modes first
    fx.0.stop();
    audio.0.stop();
    ambi.0.stop();
    std::thread::sleep(std::time::Duration::from_millis(80));
    let dur = duration_ms.unwrap_or(2000);
    let steps = (dur / 100).max(8) as u32;
    let color_payload = Some(hid.0.build_section_payload(section, r, g, b));
    hid.0.fade_to_off(current_brightness, color_payload, dur, steps)?;
    // Don't save faded brightness — it's just an animation to off
    Ok(serde_json::json!({"ok": true}))
}

#[tauri::command]
fn effects_status(fx: tauri::State<AppEffects>) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "available": effects::AVAILABLE_EFFECTS,
        "running": fx.0.is_running(),
        "active": fx.0.get_status()
    }))
}

#[tauri::command]
fn effects_start(
    hid: tauri::State<AppHid>,
    fx: tauri::State<AppEffects>,
    audio: tauri::State<AppAudio>,
    ambi: tauri::State<AppAmbilight>,
    name: String,
    led_count: Option<u16>,
    speed: Option<f64>,
) -> Result<serde_json::Value, String> {
    fx.0.stop();
    audio.0.stop();
    ambi.0.stop();
    std::thread::sleep(std::time::Duration::from_millis(150));
    let count = led_count.unwrap_or_else(|| hid.0.get_total_leds());
    let spd = speed.unwrap_or(5.0);
    fx.0.start(&name, hid.0.clone(), count, spd)?;
    Ok(serde_json::json!({"ok": true}))
}

#[tauri::command]
fn effects_stop(
    fx: tauri::State<AppEffects>,
    audio: tauri::State<AppAudio>,
    ambi: tauri::State<AppAmbilight>,
) -> Result<serde_json::Value, String> {
    fx.0.stop();
    audio.0.stop();
    ambi.0.stop();
    Ok(serde_json::json!({"ok": true}))
}

#[tauri::command]
fn device_status(hid: tauri::State<AppHid>) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "found": HidController::find_device(),
        "open": hid.0.is_open(),
        "totalLeds": hid.0.get_total_leds()
    }))
}

#[tauri::command]
fn connect_device(hid: tauri::State<AppHid>) -> Result<serde_json::Value, String> {
    hid.0.open_device()?;
    Ok(serde_json::json!({"ok": true}))
}

// ── Audio sync commands ──

#[tauri::command]
fn list_audio_sources() -> Result<serde_json::Value, String> {
    let sources = AudioSync::list_sources();
    Ok(serde_json::json!(sources))
}

#[tauri::command]
fn audio_start(
    hid: tauri::State<AppHid>,
    fx: tauri::State<AppEffects>,
    audio: tauri::State<AppAudio>,
    ambi: tauri::State<AppAmbilight>,
    mode: Option<String>,
    sensitivity: Option<f64>,
    source: Option<String>,
) -> Result<serde_json::Value, String> {
    fx.0.stop();
    audio.0.stop();
    ambi.0.stop();
    std::thread::sleep(std::time::Duration::from_millis(150));
    let led_count = hid.0.get_total_leds();
    let m = mode.unwrap_or_else(|| "spectrum".to_string());
    let sens = sensitivity.unwrap_or(1.0);
    audio.0.start(hid.0.clone(), led_count, &m, sens, source)?;
    Ok(serde_json::json!({"ok": true}))
}

#[tauri::command]
fn audio_stop(audio: tauri::State<AppAudio>) -> Result<serde_json::Value, String> {
    audio.0.stop();
    Ok(serde_json::json!({"ok": true}))
}

#[tauri::command]
fn audio_status(audio: tauri::State<AppAudio>) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "running": audio.0.is_running(),
        "modes": ["spectrum", "energy", "beat"]
    }))
}

// ── Ambilight commands ──

#[tauri::command]
fn ambilight_start(
    hid: tauri::State<AppHid>,
    fx: tauri::State<AppEffects>,
    audio: tauri::State<AppAudio>,
    ambi: tauri::State<AppAmbilight>,
    fps: Option<u32>,
) -> Result<serde_json::Value, String> {
    fx.0.stop();
    audio.0.stop();
    ambi.0.stop();
    std::thread::sleep(std::time::Duration::from_millis(150));
    let sections = *hid.0.sections.lock().unwrap();
    ambi.0.start(hid.0.clone(), sections, fps.unwrap_or(15))?;
    Ok(serde_json::json!({"ok": true}))
}

#[tauri::command]
fn ambilight_stop(ambi: tauri::State<AppAmbilight>) -> Result<serde_json::Value, String> {
    ambi.0.stop();
    Ok(serde_json::json!({"ok": true}))
}

#[tauri::command]
fn ambilight_status(ambi: tauri::State<AppAmbilight>) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "running": ambi.0.is_running()
    }))
}

fn main() {
    env_logger::init();

    let hid_ctrl = Arc::new(HidController::new());
    let effect_runner = Arc::new(EffectRunner::new());
    let audio_sync = Arc::new(AudioSync::new());
    let ambilight = Arc::new(Ambilight::new());

    // Load saved state
    let saved = load_state();
    if let Some(sections) = saved.sections {
        let total: u16 = sections.iter().sum();
        hid_ctrl.set_total_leds(total);
        *hid_ctrl.sections.lock().unwrap() = sections;
    }
    if saved.r > 0 || saved.g > 0 || saved.b > 0 {
        hid_ctrl.set_global_color(LedColor { r: saved.r, g: saved.g, b: saved.b });
    }

    let hid_for_scanner = hid_ctrl.clone();
    let saved_for_scanner = saved.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .manage(AppHid(hid_ctrl.clone()))
        .manage(AppEffects(effect_runner))
        .manage(AppAudio(audio_sync))
        .manage(AppAmbilight(ambilight))
        .invoke_handler(tauri::generate_handler![
            get_sections,
            set_sections,
            set_color_brightness,
            set_color,
            update_global_color,
            set_brightness,
            set_single_led,
            power_on,
            power_off_fade,
            effects_status,
            effects_start,
            effects_stop,
            device_status,
            connect_device,
            list_audio_sources,
            audio_start,
            audio_stop,
            audio_status,
            ambilight_start,
            ambilight_stop,
            ambilight_status,
        ])
        .setup(move |app| {
            // System tray
            let show = MenuItemBuilder::with_id("show", "Show openLightsSync").build(app)?;
            let toggle_power = MenuItemBuilder::with_id("toggle_power", "Toggle Power").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&show)
                .item(&toggle_power)
                .separator()
                .item(&quit)
                .build()?;

            let icon = Image::from_bytes(include_bytes!("../icons/icon.png"))
                .expect("failed to load tray icon");

            let _tray = TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .tooltip("openLightsSync")
                .on_menu_event(move |app, event| {
                    match event.id().as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        "toggle_power" => {
                            let hid = app.state::<AppHid>();
                            if hid.0.is_open() {
                                let payload = hid.0.build_section_payload(1, 0, 0, 0);
                                let _ = hid.0.fade_to_off(None, Some(payload), 1000, 10);
                            } else {
                                let _ = hid.0.open_device();
                                let _ = hid.0.power_on(1, 255, 255, 255, None);
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            // Background device scanner
            std::thread::spawn(move || {
                let mut device_was_open = false;
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    let found = HidController::find_device();
                    if found && !hid_for_scanner.is_open() {
                        log::info!("Scanner: device detected, opening...");
                        if let Ok(()) = hid_for_scanner.open_device() {
                            log::info!("Scanner: device opened");
                            std::thread::sleep(std::time::Duration::from_millis(200));
                            if saved_for_scanner.r > 0 || saved_for_scanner.g > 0 || saved_for_scanner.b > 0 {
                                let _ = hid_for_scanner.set_color(
                                    saved_for_scanner.section.max(1),
                                    saved_for_scanner.r,
                                    saved_for_scanner.g,
                                    saved_for_scanner.b,
                                );
                            }
                            if let Some(bri) = saved_for_scanner.brightness {
                                let _ = hid_for_scanner.set_brightness(bri);
                            }
                            device_was_open = true;
                        }
                    } else if !found && device_was_open {
                        log::info!("Scanner: device removed");
                        hid_for_scanner.close_device();
                        device_was_open = false;
                    }
                }
            });

            // Idle keepalive
            let hid_keepalive = hid_ctrl.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(3));
                if hid_keepalive.is_open() {
                    let _ = hid_keepalive.send_rb(0x97, &[]);
                }
            });

            // D-Bus server for GNOME extension
            let hid_dbus = hid_ctrl.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                if let Err(e) = rt.block_on(dbus::start_dbus_server(hid_dbus)) {
                    log::error!("D-Bus server failed: {}", e);
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
