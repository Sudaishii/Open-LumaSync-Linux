use crate::hid::{HidController, LedColor};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub const AVAILABLE_EFFECTS: &[&str] = &[
    "rainbow", "pulse", "chase", "chase_bounce", "breathe", "fire", "wave", "sparkle", "heartbeat",
];

pub struct EffectRunner {
    running: std::sync::Mutex<Arc<AtomicBool>>,
    active_name: std::sync::Mutex<Option<String>>,
}

impl EffectRunner {
    pub fn new() -> Self {
        EffectRunner {
            running: std::sync::Mutex::new(Arc::new(AtomicBool::new(false))),
            active_name: std::sync::Mutex::new(None),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.lock().unwrap().load(Ordering::Relaxed)
    }

    pub fn get_status(&self) -> Option<String> {
        self.active_name.lock().unwrap().clone()
    }

    pub fn stop(&self) {
        self.running.lock().unwrap().store(false, Ordering::Relaxed);
        *self.active_name.lock().unwrap() = None;
    }

    pub fn start(
        &self,
        name: &str,
        hid: Arc<HidController>,
        led_count: u16,
        speed: f64,
    ) -> Result<(), String> {
        if !AVAILABLE_EFFECTS.contains(&name) && name != "static" {
            return Err(format!("unknown effect: {}", name));
        }
        self.stop();
        thread::sleep(Duration::from_millis(150));

        if name == "static" {
            *self.active_name.lock().unwrap() = Some(name.to_string());
            let color = hid.get_global_color();
            hid.set_color(1, color.r, color.g, color.b)?;
            return Ok(());
        }

        *self.running.lock().unwrap() = Arc::new(AtomicBool::new(true));
        *self.active_name.lock().unwrap() = Some(name.to_string());

        let running = self.running.lock().unwrap().clone();
        let name = name.to_string();

        thread::spawn(move || {
            match name.as_str() {
                "rainbow" => run_rainbow(hid, running.clone(), led_count, speed),
                "pulse" => run_pulse(hid, running.clone(), led_count, speed),
                "chase" => run_chase(hid, running.clone(), led_count, speed, false),
                "chase_bounce" => run_chase(hid, running.clone(), led_count, speed, true),
                "breathe" => run_breathe(hid, running.clone(), led_count, speed),
                "fire" => run_fire(hid, running.clone(), led_count, speed),
                "wave" => run_wave(hid, running.clone(), led_count, speed),
                "sparkle" => run_sparkle(hid, running.clone(), led_count, speed),
                "heartbeat" => run_heartbeat(hid, running.clone(), led_count, speed),
                _ => {}
            }
            running.store(false, Ordering::Relaxed);
        });
        Ok(())
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn speed_t(speed: f64) -> f64 {
    (speed - 1.0) / 9.0
}

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> LedColor {
    let i = (h * 6.0).floor() as i32;
    let f = h * 6.0 - i as f64;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match i % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        5 => (v, p, q),
        _ => (0.0, 0.0, 0.0),
    };
    LedColor {
        r: (r * 255.0).round() as u8,
        g: (g * 255.0).round() as u8,
        b: (b * 255.0).round() as u8,
    }
}

fn clamp_u8(v: f64) -> u8 {
    v.round().max(0.0).min(255.0) as u8
}

fn simple_rand(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

fn send_frame(hid: &HidController, colors: &[LedColor], running: &AtomicBool) -> bool {
    if !running.load(Ordering::Relaxed) {
        return false;
    }
    // Hardware brightness (0x87) already scales output; do not dim RGB twice.
    if let Err(e) = hid.send_per_led_colors(colors) {
        log::error!("effect frame error: {}", e);
        return false;
    }
    true
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// ── Rainbow: smooth cycling hue across all LEDs ──
// Speed 1 ≈ old speed 5, Speed 10 ≈ slightly beyond old speed 20
fn run_rainbow(hid: Arc<HidController>, running: Arc<AtomicBool>, led_count: u16, speed: f64) {
    let tick_ms = 30u64;
    let phase_step = lerp(0.002, 0.02, speed_t(speed));
    let mut phase: f64 = 0.0;
    let count = led_count as usize;

    while running.load(Ordering::Relaxed) {
        let colors: Vec<LedColor> = (0..count)
            .map(|i| hsv_to_rgb((i as f64 / count as f64 + phase) % 1.0, 1.0, 1.0))
            .collect();
        if !send_frame(&hid, &colors, &running) {
            break;
        }
        phase = (phase + phase_step) % 1.0;
        thread::sleep(Duration::from_millis(tick_ms));
    }
}

// ── Pulse: all LEDs breathe between off and the global color ──
// Speed 1 = 0.15 Hz, Speed 10 = 1.5 Hz
fn run_pulse(hid: Arc<HidController>, running: Arc<AtomicBool>, led_count: u16, speed: f64) {
    let freq = lerp(0.15, 1.5, speed_t(speed));
    let tick_ms = 33u64;
    let mut t: f64 = 0.0;
    let count = led_count as usize;

    while running.load(Ordering::Relaxed) {
        let color = hid.get_global_color();
        t += tick_ms as f64 / 1000.0;
        let v = ((2.0 * std::f64::consts::PI * freq * t).sin() + 1.0) / 2.0;
        let colors: Vec<LedColor> = (0..count)
            .map(|_| LedColor {
                r: clamp_u8(color.r as f64 * v),
                g: clamp_u8(color.g as f64 * v),
                b: clamp_u8(color.b as f64 * v),
            })
            .collect();
        if !send_frame(&hid, &colors, &running) {
            break;
        }
        thread::sleep(Duration::from_millis(tick_ms));
    }
}

// ── Chase: single lit LED with smooth fading tail ──
// bounce=false: wraps around; bounce=true: reverses at ends
// Speed 1 ≈ old speed 3, Speed 10 = much faster than old max
fn run_chase(
    hid: Arc<HidController>,
    running: Arc<AtomicBool>,
    led_count: u16,
    speed: f64,
    bounce: bool,
) {
    let tick_ms = 25u64;
    let count = led_count as usize;
    if count == 0 {
        return;
    }
    let tail_len = (count as f64 / 4.0).max(4.0).min(16.0);
    let step = lerp(0.15, 2.5, speed_t(speed));
    let mut pos: f64 = 0.0;
    let mut dir: f64 = 1.0;

    while running.load(Ordering::Relaxed) {
        let color = hid.get_global_color();
        let colors: Vec<LedColor> = (0..count)
            .map(|i| {
                let dist = if bounce {
                    (i as f64 - pos).abs()
                } else {
                    let d = (pos - i as f64).rem_euclid(count as f64);
                    d
                };
                if dist <= tail_len {
                    let fade = 1.0 - (dist / tail_len);
                    let v = smoothstep(fade);
                    LedColor {
                        r: clamp_u8(color.r as f64 * v),
                        g: clamp_u8(color.g as f64 * v),
                        b: clamp_u8(color.b as f64 * v),
                    }
                } else {
                    LedColor::default()
                }
            })
            .collect();
        if !send_frame(&hid, &colors, &running) {
            break;
        }
        pos += step * dir;
        if bounce {
            if pos >= (count - 1) as f64 {
                pos = (count - 1) as f64;
                dir = -1.0;
            }
            if pos <= 0.0 {
                pos = 0.0;
                dir = 1.0;
            }
        } else {
            pos = pos.rem_euclid(count as f64);
        }
        thread::sleep(Duration::from_millis(tick_ms));
    }
}

// ── Breathe: smooth fade in AND fade out (full cycle) ──
// Speed 1 = ~6s cycle, Speed 10 ≈ old speed 10 (~800ms cycle)
fn run_breathe(
    hid: Arc<HidController>,
    running: Arc<AtomicBool>,
    led_count: u16,
    speed: f64,
) {
    let cycle_ms = lerp(6000.0, 800.0, speed_t(speed));
    let tick_ms = 30u64;
    let mut t: f64 = 0.0;
    let count = led_count as usize;

    while running.load(Ordering::Relaxed) {
        let color = hid.get_global_color();
        let progress = (t % cycle_ms) / cycle_ms;
        let v = if progress < 0.5 {
            let p = progress * 2.0;
            smoothstep(p)
        } else {
            let p = (progress - 0.5) * 2.0;
            1.0 - smoothstep(p)
        };
        let colors: Vec<LedColor> = (0..count)
            .map(|_| LedColor {
                r: clamp_u8(color.r as f64 * v),
                g: clamp_u8(color.g as f64 * v),
                b: clamp_u8(color.b as f64 * v),
            })
            .collect();
        if !send_frame(&hid, &colors, &running) {
            break;
        }
        t += tick_ms as f64;
        thread::sleep(Duration::from_millis(tick_ms));
    }
}

// ── Fire: flickering warm colors, heat rises from both bottom corners ──
fn run_fire(
    hid: Arc<HidController>,
    running: Arc<AtomicBool>,
    led_count: u16,
    speed: f64,
) {
    let tick_ms = lerp(55.0, 15.0, speed_t(speed)) as u64;
    let count = led_count as usize;
    let mid = count / 2;
    let len_a = mid.max(1);
    let len_b = (count - mid).max(1);
    let mut heat_a = vec![0u8; len_a];
    let mut heat_b = vec![0u8; len_b];
    let mut seed: u64 = 0xDEADBEEF_u64;

    while running.load(Ordering::Relaxed) {
        fire_sim(&mut heat_a, &mut seed);
        fire_sim(&mut heat_b, &mut seed);

        let mut colors = Vec::with_capacity(count);
        for i in 0..len_a {
            colors.push(heat_to_color(heat_a[i]));
        }
        for i in (0..len_b).rev() {
            colors.push(heat_to_color(heat_b[i]));
        }
        if !send_frame(&hid, &colors, &running) {
            break;
        }
        thread::sleep(Duration::from_millis(tick_ms));
    }
}

fn fire_sim(heat: &mut [u8], seed: &mut u64) {
    let n = heat.len();
    if n < 3 {
        return;
    }
    for i in 0..n {
        let cooling = (simple_rand(seed) % 25) as u8;
        heat[i] = heat[i].saturating_sub(cooling);
    }
    for i in (2..n).rev() {
        heat[i] = ((heat[i - 1] as u16 + heat[i - 2] as u16 * 2) / 3).min(255) as u8;
    }
    let spark_zone = (n / 4).max(2);
    let spark_count = (n / 12).max(1);
    for _ in 0..spark_count {
        let idx = (simple_rand(seed) as usize) % spark_zone;
        heat[idx] = heat[idx].saturating_add(((simple_rand(seed) % 96) + 160) as u8);
    }
}

fn heat_to_color(h: u8) -> LedColor {
    let t = h as f64 / 255.0;
    if t < 0.33 {
        let v = t / 0.33;
        LedColor { r: clamp_u8(255.0 * v), g: 0, b: 0 }
    } else if t < 0.66 {
        let v = (t - 0.33) / 0.33;
        LedColor { r: 255, g: clamp_u8(200.0 * v), b: 0 }
    } else {
        let v = (t - 0.66) / 0.34;
        LedColor { r: 255, g: 200, b: clamp_u8(230.0 * v) }
    }
}

// ── Wave: color wave propagating across the bar with smooth fade ──
fn run_wave(
    hid: Arc<HidController>,
    running: Arc<AtomicBool>,
    led_count: u16,
    speed: f64,
) {
    let tick_ms = 30u64;
    let wave_speed = lerp(0.04, 0.35, speed_t(speed));
    let mut phase: f64 = 0.0;
    let count = led_count as usize;

    while running.load(Ordering::Relaxed) {
        let color = hid.get_global_color();
        let colors: Vec<LedColor> = (0..count)
            .map(|i| {
                let pos = i as f64 / count as f64;
                let raw = ((pos * 3.0 * std::f64::consts::PI + phase).sin() + 1.0) / 2.0;
                let v = smoothstep(raw);
                LedColor {
                    r: clamp_u8(color.r as f64 * v),
                    g: clamp_u8(color.g as f64 * v),
                    b: clamp_u8(color.b as f64 * v),
                }
            })
            .collect();
        if !send_frame(&hid, &colors, &running) {
            break;
        }
        phase += wave_speed;
        thread::sleep(Duration::from_millis(tick_ms));
    }
}

// ── Sparkle: random LEDs with smooth fade-in then fade-out ──
// Speed 1 ≈ old speed 20 (slow sparkle), Speed 10 = much faster
fn run_sparkle(
    hid: Arc<HidController>,
    running: Arc<AtomicBool>,
    led_count: u16,
    speed: f64,
) {
    let tick_ms = 25u64;
    let count = led_count as usize;
    let sparks_per_tick = lerp(2.0, 10.0, speed_t(speed)) as usize;
    let max_life = lerp(22.0, 8.0, speed_t(speed));
    let mut lives = vec![0.0f64; count];
    let mut max_lives = vec![0.0f64; count];
    let mut seed: u64 = 0xCAFEBABE_u64;

    while running.load(Ordering::Relaxed) {
        let color = hid.get_global_color();
        for i in 0..count {
            if lives[i] > 0.0 {
                lives[i] -= 1.0;
                if lives[i] < 0.0 {
                    lives[i] = 0.0;
                }
            }
        }
        for _ in 0..sparks_per_tick {
            let idx = (simple_rand(&mut seed) as usize) % count;
            if lives[idx] <= 0.0 {
                lives[idx] = max_life;
                max_lives[idx] = max_life;
            }
        }
        let colors: Vec<LedColor> = (0..count)
            .map(|i| {
                if lives[i] <= 0.0 {
                    LedColor::default()
                } else {
                    let t = 1.0 - lives[i] / max_lives[i];
                    let v = (t * std::f64::consts::PI).sin();
                    LedColor {
                        r: clamp_u8(color.r as f64 * v),
                        g: clamp_u8(color.g as f64 * v),
                        b: clamp_u8(color.b as f64 * v),
                    }
                }
            })
            .collect();
        if !send_frame(&hid, &colors, &running) {
            break;
        }
        thread::sleep(Duration::from_millis(tick_ms));
    }
}

// ── Heartbeat: realistic double-pulse like a real heartbeat ──
fn run_heartbeat(
    hid: Arc<HidController>,
    running: Arc<AtomicBool>,
    led_count: u16,
    speed: f64,
) {
    let cycle_ms = lerp(1600.0, 400.0, speed_t(speed));
    let tick_ms = 20u64;
    let mut t: f64 = 0.0;
    let count = led_count as usize;

    while running.load(Ordering::Relaxed) {
        let color = hid.get_global_color();
        let phase = (t % cycle_ms) / cycle_ms;

        let v = if phase < 0.08 {
            smoothstep(phase / 0.08)
        } else if phase < 0.15 {
            1.0 - smoothstep((phase - 0.08) / 0.07)
        } else if phase < 0.22 {
            0.0
        } else if phase < 0.27 {
            smoothstep((phase - 0.22) / 0.05) * 0.6
        } else if phase < 0.34 {
            (1.0 - smoothstep((phase - 0.27) / 0.07)) * 0.6
        } else {
            0.0
        };

        let colors: Vec<LedColor> = (0..count)
            .map(|_| LedColor {
                r: clamp_u8(color.r as f64 * v),
                g: clamp_u8(color.g as f64 * v),
                b: clamp_u8(color.b as f64 * v),
            })
            .collect();
        if !send_frame(&hid, &colors, &running) {
            break;
        }
        t += tick_ms as f64;
        thread::sleep(Duration::from_millis(tick_ms));
    }
}
