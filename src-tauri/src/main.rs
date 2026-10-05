#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ambilight;
mod audio;
mod audio_effects;
mod dbus;
mod effects;
mod hid;
mod state;
mod validation;

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
    Manager, Emitter,
};

struct AppHid(Arc<HidController>);
struct AppEffects(Arc<EffectRunner>);
struct AppAudio(Arc<AudioSync>);
struct AppAmbilight(Arc<Ambilight>);
struct AppShellStatus(std::sync::Mutex<serde_json::Value>);

#[tauri::command]
fn get_controller_config() -> Option<serde_json::Value> { state::load_controller_config() }

#[tauri::command]
fn save_controller_config(config: serde_json::Value) -> Result<(), String> { state::save_controller_config(&config) }

#[tauri::command]
fn list_screen_outputs() -> Result<Vec<serde_json::Value>, String> {
    ambilight::list_outputs()
}

#[tauri::command]
fn publish_controller_status(app: tauri::AppHandle, status: serde_json::Value) -> Result<(), String> {
    if !status.is_object() { return Err("Invalid controller status".into()); }
    *app.state::<AppShellStatus>().0.lock().unwrap()=status;
    Ok(())
}

#[tauri::command]
fn get_saved_state() -> state::AppState { load_state() }

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
    let total = validation::sections(sections)?;
    if let Some(info) = hid.0.firmware_info.lock().unwrap().as_ref() {
        if total as u64 > info["ledCount"].as_u64().unwrap_or(254) { return Err(format!("Your backlight reports {} LEDs; reduce the layout total to that count.", info["ledCount"])); }
    }
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
        hid.0.set_color(1, r, g, b)?;
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
    if index >= hid.0.get_total_leds() as usize {
        return Err("LED index is outside the configured layout.".into());
    }
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
    let dur = duration_ms.unwrap_or(300);
    if dur > 3000 { return Err("Power-off fade must be at most 3000 milliseconds.".into()); }
    if !hid.0.is_open() { return Err("Backlight disconnected. Reconnect the USB device.".into()); }
    fx.0.stop();
    audio.0.stop();
    ambi.0.stop();
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
    let count = led_count.unwrap_or_else(|| hid.0.get_total_leds());
    if !(1..=254).contains(&count) { return Err("Effect LED count must be 1–254.".into()); }
    let spd = validation::finite_range(speed.unwrap_or(5.0), 1., 10., "Effect speed")?;
    if !effects::AVAILABLE_EFFECTS.contains(&name.as_str()) && name != "static" { return Err("Unknown lighting effect.".into()); }
    if !hid.0.is_open() { return Err("Backlight disconnected. Reconnect the USB device.".into()); }
    fx.0.stop();
    audio.0.stop();
    ambi.0.stop();
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
        "totalLeds": hid.0.get_total_leds(),
        "sections": *hid.0.sections.lock().unwrap(),
        "firmwareInfo": *hid.0.firmware_info.lock().unwrap()
    }))
}

#[tauri::command]
fn connect_device(hid: tauri::State<AppHid>) -> Result<serde_json::Value, String> {
    initialize_device(&hid.0)?;
    Ok(serde_json::json!({"ok": true}))
}

// ── Audio sync commands ──

#[tauri::command]
fn list_audio_sources() -> Result<serde_json::Value, String> {
    let sources = AudioSync::list_sources()?;
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
    palette: Option<String>,
    secondary_color: Option<LedColor>,
    speed: Option<f64>,
    width: Option<f64>,
    noise_gate: Option<f64>,
    reverse: Option<bool>,
) -> Result<serde_json::Value, String> {
    let led_count = hid.0.get_total_leds();
    let m = mode.unwrap_or_else(|| "spectrum".to_string());
    if !audio_effects::MODES.contains(&m.as_str()) { return Err("Choose a listed audio response style.".into()); }
    let sens = validation::finite_range(sensitivity.unwrap_or(1.0), 0.1, 5., "Audio sensitivity")?;
    let options = audio_effects::Options {
        palette: audio_effects::Palette::parse(palette.as_deref().unwrap_or("rainbow"))?,
        secondary: secondary_color.unwrap_or_else(|| audio_effects::Options::default().secondary),
        speed: validation::finite_range(speed.unwrap_or(4.),1.,10.,"Audio movement speed")?,
        width: validation::finite_range(width.unwrap_or(0.24),0.08,0.8,"Audio effect width")?,
        noise_gate: validation::finite_range(noise_gate.unwrap_or(0.),0.,0.3,"Audio noise gate")?,
        reverse: reverse.unwrap_or(false),
    };
    if !hid.0.is_open() { return Err("Backlight disconnected. Reconnect the USB device.".into()); }
    fx.0.stop();
    audio.0.stop();
    ambi.0.stop();
    audio.0.start(hid.0.clone(), led_count, &m, sens, source, options)?;
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
        "error": audio.0.error(),
        "metrics": audio.0.metrics(),
        "modes": audio_effects::MODES,
        "palettes": audio_effects::PALETTES
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
    output: Option<String>,
    smoothing: Option<f64>,
    depth: Option<usize>,
    reverse: Option<bool>,
    capture_scale: Option<f64>,
) -> Result<serde_json::Value, String> {
    let sections = *hid.0.sections.lock().unwrap();
    let fps = fps.unwrap_or(15);
    if !(1..=30).contains(&fps) { return Err("Capture rate must be 1–30 fps.".into()); }
    let smoothing = validation::finite_range(smoothing.unwrap_or(0.5),0.,0.95,"Smoothing")?;
    let depth = depth.unwrap_or(40);
    if !(1..=200).contains(&depth) { return Err("Sample depth must be 1–200 pixels.".into()); }
    let capture_scale = validation::finite_range(capture_scale.unwrap_or(0.35),0.1,1.,"Capture scale")?;
    if !hid.0.is_open() { return Err("Backlight disconnected. Reconnect the USB device.".into()); }
    fx.0.stop();
    audio.0.stop();
    ambi.0.stop();
    ambi.0.start(hid.0.clone(), sections, fps, ambilight::CaptureOptions {
        output, smoothing, depth, reverse: reverse.unwrap_or(false), capture_scale,
    })?;
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
        "running": ambi.0.is_running(),
        "error": ambi.0.error(),
        "metrics": ambi.0.metrics()
    }))
}

fn initialize_device(hid: &HidController) -> Result<(), String> {
    hid.open_device()?;
    let info = hid.read_device_info()?;
    let count = info["ledCount"].as_u64().ok_or("Missing firmware LED count")? as u16;
    let sections = *hid.sections.lock().unwrap();
    let total: u16 = sections.iter().sum();
    if total > count || total == 0 {
        // Preserve left/right proportions when replacing an oversized default.
        let denominator = total.max(1) as u32;
        let left = (sections[0] as u32 * count as u32 / denominator) as u16;
        let right = (sections[2] as u32 * count as u32 / denominator) as u16;
        let adjusted = [left, count - left - right, right];
        *hid.sections.lock().unwrap() = adjusted;
        hid.set_total_leds(count);
        let mut state = load_state();
        state.sections = Some(adjusted);
        save_state(&state);
    }
    Ok(())
}

fn hardware_check(test: bool, sustain: bool) -> Result<serde_json::Value,String> {
    let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
    let interfaces: Vec<_> = api.device_list().filter(|d| d.vendor_id()==0x1a86 && [0xfe07,0xfe0c].contains(&d.product_id())).map(|d| serde_json::json!({"path":d.path().to_string_lossy(),"usagePage":d.usage_page(),"interface":d.interface_number()})).collect();
    let hid = HidController::new();
    hid.open_device()?;
    let firmware_info = hid.read_device_info();
    if test || sustain {
        hid.set_color_and_brightness(1,40,150,200,80)?;
        if sustain {
            std::thread::sleep(std::time::Duration::from_secs(21));
        } else { std::thread::sleep(std::time::Duration::from_secs(2)); }
        let state = load_state();
        let (r,g,b) = if state.r==0 && state.g==0 && state.b==0 {(239,181,117)} else {(state.r,state.g,state.b)};
        hid.set_color_and_brightness(1,r,g,b,state.brightness.filter(|&v|v>0).unwrap_or(119))?;
    }
    Ok(serde_json::json!({"ok":true,"firmwareInfo":firmware_info.as_ref().ok(),"firmwareInfoError":firmware_info.as_ref().err(),"interfaces":interfaces,"vendorInterfaceOpened":hid.is_open(),"testWritesAccepted":test||sustain,"sustainedSeconds":if sustain {21} else {0}}))
}

fn integration_check() -> Result<serde_json::Value,String> {
    let hid = Arc::new(HidController::new());
    initialize_device(&hid)?;
    let count = hid.get_total_leds();
    hid.set_color_and_brightness(1,239,181,117,80)?;
    let led_colors = vec![LedColor {r:40,g:150,b:200};count as usize];
    hid.send_per_led_colors(&led_colors)?;
    let fx = EffectRunner::new();
    fx.start("rainbow",hid.clone(),count,2.)?;
    std::thread::sleep(std::time::Duration::from_secs(8));
    let effects_running = fx.is_running();
    fx.stop();
    std::thread::sleep(std::time::Duration::from_millis(250));
    let audio = AudioSync::new();
    audio.start(hid.clone(),count,"energy",1.,None,audio_effects::Options::default())?;
    std::thread::sleep(std::time::Duration::from_millis(800));
    let audio_running=audio.is_running();
    let audio_error=audio.error();
    let audio_metrics=audio.metrics();
    audio.stop();
    std::thread::sleep(std::time::Duration::from_millis(250));
    let screen=Ambilight::new();
    let screen_started=screen.start(hid.clone(),*hid.sections.lock().unwrap(),3,ambilight::CaptureOptions {output:None,smoothing:0.5,depth:40,reverse:false,capture_scale:1.});
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let screen_running=screen.is_running();
    let screen_error=screen_started.err().or_else(||screen.error());
    let screen_metrics=screen.metrics();
    screen.stop();
    std::thread::sleep(std::time::Duration::from_millis(400));
    let state=load_state();
    let (r,g,b)=if state.r==0 && state.g==0 && state.b==0 {(239,181,117)} else {(state.r,state.g,state.b)};
    hid.set_color_and_brightness(1,r,g,b,state.brightness.filter(|&v|v>0).unwrap_or(119))?;
    Ok(serde_json::json!({"vendorInterfaceOpened":true,"perLedWritesAccepted":true,"effectsRunning":effects_running,"audioRunning":audio_running,"audioError":audio_error,"audioMetrics":audio_metrics,"screenRunning":screen_running,"screenError":screen_error,"screenMetrics":screen_metrics,"colorRestored":true}))
}

fn audio_check() -> Result<serde_json::Value, String> {
    let hid = Arc::new(HidController::new());
    initialize_device(&hid)?;
    hid.set_global_color(LedColor {r:40,g:150,b:240});
    hid.set_color_and_brightness(1,40,150,240,100)?;
    let audio = AudioSync::new();
    let source = std::env::args().find_map(|a|a.strip_prefix("--audio-source=").map(str::to_owned));
    let mode=std::env::args().find_map(|a|a.strip_prefix("--audio-mode=").map(str::to_owned)).unwrap_or_else(||"energy".into());
    if !audio_effects::MODES.contains(&mode.as_str()) { return Err("Unknown audio diagnostic mode".into()); }
    let palette=std::env::args().find_map(|a|a.strip_prefix("--audio-palette=").map(str::to_owned)).unwrap_or_else(||"selected".into());
    let options=audio_effects::Options {palette:audio_effects::Palette::parse(&palette)?,..Default::default()};
    let sensitivity = std::env::args().find_map(|a|a.strip_prefix("--audio-sensitivity=").map(str::to_owned))
        .unwrap_or_else(||"2".into()).parse::<f64>().map_err(|_|"Invalid audio diagnostic sensitivity".to_string())?;
    let sensitivity = validation::finite_range(sensitivity,0.1,5.,"Audio diagnostic sensitivity")?;
    audio.start(hid.clone(),hid.get_total_leds(),&mode,sensitivity,source,options)?;
    let mut snapshots = Vec::new();
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let metrics = audio.metrics();
        println!("{}", serde_json::json!({"audioSample": metrics}));
        snapshots.push(metrics);
        if !audio.is_running() { break; }
    }
    audio.stop();
    std::thread::sleep(std::time::Duration::from_millis(250));
    let state = load_state();
    let color = if state.r==0 && state.g==0 && state.b==0 {(239,181,117)} else {(state.r,state.g,state.b)};
    hid.set_color_and_brightness(1,color.0,color.1,color.2,state.brightness.filter(|&v|v>0).unwrap_or(119))?;
    if let Some(e) = audio.error() { return Err(e); }
    Ok(serde_json::json!({"audioSnapshots":snapshots,"colorRestored":true}))
}

fn screen_check() -> Result<serde_json::Value, String> {
    let fps=std::env::args().find_map(|a|a.strip_prefix("--screen-fps=").map(str::to_owned))
        .map(|v|v.parse::<u32>().map_err(|_|"Screen diagnostic FPS must be a whole number".to_string())).transpose()?.unwrap_or(8);
    if !(1..=30).contains(&fps) { return Err("Screen diagnostic FPS must be 1–30".into()); }
    let hid = Arc::new(HidController::new());
    initialize_device(&hid)?;
    hid.set_color_and_brightness(1, 120, 120, 120, 100)?;
    let screen = Ambilight::new();
    let started = screen.start(hid.clone(), *hid.sections.lock().unwrap(), fps,
        ambilight::CaptureOptions {output:None, smoothing:0., depth:80, reverse:false,capture_scale:1.});
    let mut snapshots = Vec::new();
    if started.is_ok() {
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let metrics = screen.metrics();
            println!("{}", serde_json::json!({"screenSample": metrics}));
            snapshots.push(metrics);
            if !screen.is_running() { break; }
        }
    }
    screen.stop();
    std::thread::sleep(std::time::Duration::from_millis(250));
    let state = load_state();
    let color = if state.r==0 && state.g==0 && state.b==0 {(239,181,117)} else {(state.r,state.g,state.b)};
    hid.set_color_and_brightness(1,color.0,color.1,color.2,state.brightness.filter(|&v|v>0).unwrap_or(119))?;
    started?;
    if let Some(e) = screen.error() { return Err(e); }
    Ok(serde_json::json!({"screenSnapshots":snapshots,"colorRestored":true}))
}

fn main() {
    env_logger::init();
    if std::env::args().any(|a| a == "--hardware-check" || a == "--hardware-test" || a == "--integration-test" || a == "--sustain-test" || a == "--screen-check" || a == "--audio-check") {
        let test = std::env::args().any(|a| a == "--hardware-test");
        let result = if std::env::args().any(|a| a == "--audio-check") {audio_check()} else if std::env::args().any(|a| a == "--screen-check") {screen_check()} else if std::env::args().any(|a| a == "--integration-test") {integration_check()} else {hardware_check(test, std::env::args().any(|a| a == "--sustain-test"))};
        match result { Ok(value) => println!("{}", value), Err(error) => { eprintln!("{}", serde_json::json!({"ok": false,"error":error})); std::process::exit(1); } }
        return;
    }

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

    if let Err(e) = initialize_device(&hid_ctrl) { log::warn!("Device initialization: {e}"); }
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
        .manage(AppShellStatus(std::sync::Mutex::new(serde_json::json!({"ready":false}))))
        .invoke_handler(tauri::generate_handler![
            get_saved_state,
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
            publish_controller_status,
            list_screen_outputs,
            get_controller_config,
            save_controller_config,
        ])
        .setup(move |app| {
            if std::env::args().any(|a|a=="--background") {
                if let Some(window)=app.get_webview_window("main") { window.hide()?; }
            }
            // System tray
            let show = MenuItemBuilder::with_id("show", "Show snzhy-OpenSycnlights").build(app)?;
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
                .tooltip("snzhy-OpenSycnlights")
                .on_menu_event(move |app, event| {
                    match event.id().as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        "toggle_power" => { let _=app.emit("controller-action","toggle"); }
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
                        if let Ok(()) = initialize_device(&hid_for_scanner) {
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

            // D-Bus server for GNOME extension
            let hid_dbus = hid_ctrl.clone();
            let app_dbus=app.handle().clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                if let Err(e) = rt.block_on(dbus::start_dbus_server(hid_dbus,app_dbus)) {
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
