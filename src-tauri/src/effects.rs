use crate::hid::{HidController, LedColor};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const AVAILABLE_EFFECTS: &[&str] = &[
    "rainbow",
    "pulse",
    "chase",
    "chase_bounce",
    "breathe",
    "fire",
    "wave",
    "sparkle",
    "heartbeat",
    "aurora",
    "ocean",
    "gradient",
    "theater_chase",
    "color_wipe",
    "scanner",
    "meteor",
    "twinkle",
    "fireworks",
    "rainbow_wave",
];

const FRAME_INTERVAL: Duration = Duration::from_millis(33);

pub struct EffectRunner {
    // Serialize the entire mode transition, including joining the previous writer.
    state: Mutex<RunnerState>,
}

struct RunnerState {
    running: Arc<AtomicBool>,
    active_name: Option<String>,
    worker: Option<JoinHandle<()>>,
}

impl RunnerState {
    fn stop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            // HID sends are synchronous and can span several packets. Joining is
            // the barrier that prevents any old packet from following a new mode.
            if worker.join().is_err() {
                log::error!("lighting effect worker panicked");
            }
        }
        self.active_name = None;
    }
}

impl EffectRunner {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(RunnerState {
                running: Arc::new(AtomicBool::new(false)),
                active_name: None,
                worker: None,
            }),
        }
    }

    pub fn is_running(&self) -> bool {
        self.state.lock().unwrap().running.load(Ordering::Acquire)
    }

    pub fn get_status(&self) -> Option<String> {
        let state = self.state.lock().unwrap();
        if state.worker.is_some() && !state.running.load(Ordering::Acquire) {
            None
        } else {
            state.active_name.clone()
        }
    }

    pub fn stop(&self) {
        self.state.lock().unwrap().stop();
    }

    pub fn start(
        &self,
        name: &str,
        hid: Arc<HidController>,
        led_count: u16,
        speed: f64,
    ) -> Result<(), String> {
        let effect = Effect::parse(name);
        if effect.is_none() && name != "static" {
            return Err(format!("unknown effect: {name}"));
        }
        if !(1..=254).contains(&led_count) {
            return Err("Effect LED count must be 1–254.".into());
        }
        if !speed.is_finite() || !(1.0..=10.0).contains(&speed) {
            return Err("Effect speed must be a finite number from 1 to 10.".into());
        }

        // Validate first: a rejected request must not stop the current effect.
        let mut state = self.state.lock().unwrap();
        state.stop();
        if name == "static" {
            let color = hid.get_global_color();
            hid.set_color(1, color.r, color.g, color.b)?;
            state.active_name = Some(name.to_string());
            return Ok(());
        }

        let running = Arc::new(AtomicBool::new(true));
        state.running = running.clone();
        let renderer = FrameRenderer::new(effect.unwrap(), led_count as usize, speed);
        match thread::Builder::new()
            .name(format!("lighting-{name}"))
            .spawn(move || {
                run_effect(
                    renderer,
                    &running,
                    || hid.get_global_color(),
                    |frame| send_frame(&hid, frame, &running),
                );
            }) {
            Ok(worker) => {
                state.worker = Some(worker);
                state.active_name = Some(name.to_string());
                Ok(())
            }
            Err(error) => {
                state.running.store(false, Ordering::Release);
                Err(format!("Cannot start lighting effect: {error}"))
            }
        }
    }
}

impl Drop for EffectRunner {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone, Copy, Debug)]
enum Effect {
    Rainbow,
    Pulse,
    Chase,
    ChaseBounce,
    Breathe,
    Fire,
    Wave,
    Sparkle,
    Heartbeat,
    Aurora,
    Ocean,
    Gradient,
    TheaterChase,
    ColorWipe,
    Scanner,
    Meteor,
    Twinkle,
    Fireworks,
    RainbowWave,
}

impl Effect {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "rainbow" => Self::Rainbow,
            "pulse" => Self::Pulse,
            "chase" => Self::Chase,
            "chase_bounce" => Self::ChaseBounce,
            "breathe" => Self::Breathe,
            "fire" => Self::Fire,
            "wave" => Self::Wave,
            "sparkle" => Self::Sparkle,
            "heartbeat" => Self::Heartbeat,
            "aurora" => Self::Aurora,
            "ocean" => Self::Ocean,
            "gradient" => Self::Gradient,
            "theater_chase" => Self::TheaterChase,
            "color_wipe" => Self::ColorWipe,
            "scanner" => Self::Scanner,
            "meteor" => Self::Meteor,
            "twinkle" => Self::Twinkle,
            "fireworks" => Self::Fireworks,
            "rainbow_wave" => Self::RainbowWave,
            _ => return None,
        })
    }
}

struct FrameRenderer {
    effect: Effect,
    speed: f64,
    frame: Vec<LedColor>,
    heat: Vec<u8>,
    previous_heat: Vec<u8>,
    fire_step: u64,
    seed: u64,
}

impl FrameRenderer {
    fn new(effect: Effect, count: usize, speed: f64) -> Self {
        let heat_count = if matches!(effect, Effect::Fire) {
            count
        } else {
            0
        };
        Self {
            effect,
            speed: speed_t(speed),
            frame: vec![LedColor::default(); count],
            heat: vec![0; heat_count],
            previous_heat: vec![0; heat_count],
            fire_step: 0,
            seed: 0xDEADBEEF,
        }
    }

    fn render(&mut self, elapsed: Duration, color: &LedColor) -> &[LedColor] {
        use std::f64::consts::{PI, TAU};
        let seconds = elapsed.as_secs_f64();
        let speed = self.speed;
        let count = self.frame.len() as f64;
        match self.effect {
            Effect::Rainbow => {
                let phase = seconds * lerp(0.002, 0.02, speed) / 0.03;
                paint(&mut self.frame, |i| {
                    hsv_to_rgb(i as f64 / count + phase, 1.0, 1.0)
                });
            }
            Effect::Pulse => {
                let value = 0.5 + 0.5 * (TAU * seconds * lerp(0.15, 1.5, speed)).sin();
                self.frame.fill(scale_color(color, value));
            }
            Effect::Breathe => {
                let phase = seconds / lerp(6.0, 0.8, speed);
                self.frame
                    .fill(scale_color(color, smoothstep(triangle(phase))));
            }
            Effect::Chase | Effect::ChaseBounce => {
                if count == 1.0 {
                    let value = 0.15 + 0.85 * smoothstep(triangle(seconds / lerp(4.0, 1.4, speed)));
                    self.frame.fill(scale_color(color, value));
                } else {
                    let tail = (count / 4.0).clamp(4.0, 16.0).min(count - 1.0);
                    let bounce = matches!(self.effect, Effect::ChaseBounce);
                    // Tiny layouts must not turn a fast chase into rapid flashing.
                    let path = if bounce { 2.0 * (count - 1.0) } else { count };
                    let velocity = lerp(6.0, 100.0, speed).min(path / 1.2);
                    let head = if bounce {
                        triangle(seconds * velocity / path) * (count - 1.0)
                    } else {
                        (seconds * velocity).rem_euclid(count)
                    };
                    paint(&mut self.frame, |i| {
                        let distance = if bounce {
                            (i as f64 - head).abs()
                        } else {
                            (head - i as f64).rem_euclid(count)
                        };
                        let value = if distance <= tail {
                            smoothstep(1.0 - distance / tail)
                        } else if !bounce && distance > count - 1.0 {
                            // Crossfade the leading LED rather than snapping it on.
                            smoothstep(distance - (count - 1.0))
                        } else {
                            0.0
                        };
                        scale_color(color, value)
                    });
                }
            }
            Effect::Fire => self.render_fire(seconds),
            Effect::Wave => {
                let phase = seconds * lerp(0.04, 0.35, speed) / 0.03;
                paint(&mut self.frame, |i| {
                    scale_color(
                        color,
                        smoothstep(0.5 + 0.5 * (i as f64 / count * 3.0 * PI + phase).sin()),
                    )
                });
            }
            Effect::Sparkle => {
                let period = lerp(2.8, 0.9, speed);
                paint(&mut self.frame, |i| {
                    let phase = seconds / period + hash_unit(i, 0) * 3.0;
                    let age = phase.fract();
                    let active =
                        count == 1.0 || hash_unit(i, (phase.floor() as u64).wrapping_add(1)) > 0.72;
                    let value = if active && age < 0.65 {
                        (PI * age / 0.65).sin().powi(2)
                    } else {
                        0.0
                    };
                    scale_color(color, value)
                });
            }
            Effect::Heartbeat => {
                // Broad double pulses, at most 50 beats/minute at speed 10.
                let phase = (seconds / lerp(2.8, 1.2, speed)).fract();
                let value = smooth_lobe(phase, 0.18, 0.18) + 0.6 * smooth_lobe(phase, 0.52, 0.16);
                self.frame.fill(scale_color(color, value));
            }
            Effect::Aurora => {
                let time = seconds * lerp(0.04, 0.22, speed);
                paint(&mut self.frame, |i| {
                    let x = i as f64 / count;
                    let curtain = 0.5
                        + 0.5
                            * (TAU * (x * 2.3 - time) + 0.6 * (TAU * (x * 0.7 + time * 0.5)).sin())
                                .sin();
                    let hue = x * 1.2 + time * 0.6 + 0.09 * (TAU * (x + time)).sin();
                    scale_color(
                        &palette_color(&AURORA_PALETTE, hue),
                        0.3 + 0.7 * smoothstep(curtain),
                    )
                });
            }
            Effect::Ocean => {
                let time = seconds * lerp(0.05, 0.28, speed);
                paint(&mut self.frame, |i| {
                    let x = i as f64 / count;
                    let swell = 0.5 + 0.5 * (TAU * (x * 1.7 - time)).sin();
                    let ripple = 0.5 + 0.5 * (TAU * (x * 4.1 + time * 0.6)).sin();
                    let water =
                        palette_color(&OCEAN_PALETTE, x * 0.8 + time * 0.35 + ripple * 0.08);
                    scale_color(&water, 0.35 + 0.5 * smoothstep(swell) + 0.15 * ripple)
                });
            }
            Effect::Gradient => {
                let (hue, saturation, value) = rgb_to_hsv(color);
                // Keep the chosen color as the anchor; neutral colors still
                // need tinted stops for a visible, animated gradient.
                let saturation = saturation.max(0.65);
                let palette = [
                    color.clone(),
                    hsv_to_rgb(hue + 0.18, saturation, value),
                    hsv_to_rgb(hue + 0.5, saturation, value),
                ];
                let phase = seconds * lerp(0.04, 0.24, speed);
                paint(&mut self.frame, |i| {
                    palette_color(&palette, i as f64 / count * 0.75 + phase)
                });
            }
            Effect::TheaterChase => {
                let phase = seconds * lerp(0.25, 1.4, speed);
                paint(&mut self.frame, |i| {
                    let distance = ((i as f64 - phase + 1.5).rem_euclid(3.0) - 1.5).abs();
                    scale_color(color, 0.04 + 0.96 * smoothstep(1.0 - distance / 0.95))
                });
            }
            Effect::ColorWipe => {
                let progress = triangle(seconds / lerp(6.0, 2.0, speed));
                let edge = (count * 0.15).max(1.0);
                let head = progress * (count + 2.0 * edge) - edge;
                paint(&mut self.frame, |i| {
                    scale_color(color, smoothstep((head - i as f64) / edge))
                });
            }
            Effect::Scanner => {
                let phase = seconds / lerp(6.0, 1.8, speed);
                if count == 1.0 {
                    self.frame.fill(scale_color(
                        color,
                        0.15 + 0.85 * (0.5 + 0.5 * (TAU * phase).cos()),
                    ));
                } else {
                    let head = triangle(phase) * (count - 1.0);
                    let width = (count * 0.055).max(0.7);
                    paint(&mut self.frame, |i| {
                        let distance = (i as f64 - head) / width;
                        scale_color(color, 0.015 + 0.985 * (-0.5 * distance * distance).exp())
                    });
                }
            }
            Effect::Meteor => {
                let tail = (count * 0.35).max(3.0);
                let front = (count * 0.035).max(1.0);
                let phase = (seconds / lerp(6.0, 1.8, speed)).fract();
                let head = phase * (count + tail + 2.0 * front) - front;
                paint(&mut self.frame, |i| {
                    let distance = head - i as f64;
                    let value = if distance < 0.0 {
                        smoothstep(1.0 + distance / front)
                    } else {
                        (-distance / (tail * 0.45)).exp() * smoothstep(1.0 - distance / tail)
                    };
                    scale_color(color, value)
                });
            }
            Effect::Twinkle => {
                let period = lerp(5.0, 1.5, speed);
                paint(&mut self.frame, |i| {
                    let phase =
                        seconds / (period * lerp(0.8, 1.4, hash_unit(i, 13))) + hash_unit(i, 27);
                    let shimmer = (0.5 + 0.5 * (TAU * phase).sin()).powi(2);
                    scale_color(color, 0.06 + 0.94 * shimmer)
                });
            }
            Effect::Fireworks => {
                let period = lerp(5.0, 2.4, speed);
                let (hue, saturation, value) = rgb_to_hsv(color);
                // Staggered expanding rings. Their envelopes start/end at zero,
                // so changing the seeded origin never produces a sudden flash.
                let bursts: [(f64, f64, f64, LedColor); 3] = std::array::from_fn(|k| {
                    let phase = seconds / period + k as f64 / 3.0;
                    let age = phase.fract();
                    let center =
                        hash_unit(k, (phase.floor() as u64).wrapping_add(99)) * (count - 1.0);
                    let radius = age * count * 0.75;
                    let envelope = smoothstep(age / 0.16) * (1.0 - smoothstep((age - 0.3) / 0.7));
                    let accent =
                        hsv_to_rgb(hue + [-0.08, 0.13, 0.32][k], saturation.max(0.65), value);
                    (center, radius, envelope, mix_color(color, &accent, 0.55))
                });
                let width = (count * 0.045).max(0.8);
                paint(&mut self.frame, |i| {
                    let mut r = 0.0;
                    let mut g = 0.0;
                    let mut b = 0.0;
                    for (center, radius, envelope, tint) in &bursts {
                        let distance = ((i as f64 - center).abs() - radius).abs() / width;
                        let intensity = smoothstep(1.0 - distance) * envelope;
                        r += tint.r as f64 * intensity;
                        g += tint.g as f64 * intensity;
                        b += tint.b as f64 * intensity;
                    }
                    LedColor {
                        r: clamp_u8(r),
                        g: clamp_u8(g),
                        b: clamp_u8(b),
                    }
                });
            }
            Effect::RainbowWave => {
                let phase = seconds * lerp(0.05, 0.4, speed);
                paint(&mut self.frame, |i| {
                    let x = i as f64 / count;
                    let crest = smoothstep(0.5 + 0.5 * (TAU * (x * 3.0 - phase * 0.8)).sin());
                    hsv_to_rgb(x * 2.0 + phase, 1.0, 0.12 + 0.88 * crest)
                });
            }
        }
        &self.frame
    }

    fn render_fire(&mut self, seconds: f64) {
        // On tiny layouts a single spark changes most/all of the bar. Stretch
        // those transitions while retaining faster, local flicker on large bars.
        let interval = if self.heat.len() <= 3 {
            lerp(0.8, 0.35, self.speed)
        } else {
            lerp(0.09, 0.04, self.speed)
        };
        let position = seconds / interval;
        let target_step = (position.floor() as u64).saturating_add(1);
        // Bound catch-up after a suspended process; a slow HID send normally
        // requires only a few steps. Repeated rendering at one time is stable.
        let steps = target_step.saturating_sub(self.fire_step).min(32);
        let mid = self.heat.len() / 2;
        for _ in 0..steps {
            self.previous_heat.copy_from_slice(&self.heat);
            fire_sim(&mut self.heat[..mid], &mut self.seed);
            fire_sim(&mut self.heat[mid..], &mut self.seed);
        }
        self.fire_step = target_step;
        let count = self.frame.len();
        for (i, output) in self.frame.iter_mut().enumerate() {
            let index = if i < mid { i } else { count - 1 - (i - mid) };
            let heat = lerp(
                self.previous_heat[index] as f64,
                self.heat[index] as f64,
                position.fract(),
            );
            *output = heat_to_color(heat);
        }
    }
}

const AURORA_PALETTE: [LedColor; 4] = [
    LedColor {
        r: 10,
        g: 220,
        b: 92,
    },
    LedColor {
        r: 10,
        g: 166,
        b: 230,
    },
    LedColor {
        r: 135,
        g: 36,
        b: 215,
    },
    LedColor {
        r: 40,
        g: 230,
        b: 170,
    },
];
const OCEAN_PALETTE: [LedColor; 4] = [
    LedColor { r: 0, g: 14, b: 64 },
    LedColor {
        r: 0,
        g: 76,
        b: 150,
    },
    LedColor {
        r: 0,
        g: 185,
        b: 190,
    },
    LedColor {
        r: 110,
        g: 220,
        b: 245,
    },
];

fn paint(frame: &mut [LedColor], mut color: impl FnMut(usize) -> LedColor) {
    for (i, output) in frame.iter_mut().enumerate() {
        *output = color(i);
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn speed_t(speed: f64) -> f64 {
    (speed - 1.0) / 9.0
}

fn clamp_u8(value: f64) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn triangle(phase: f64) -> f64 {
    1.0 - (2.0 * phase.rem_euclid(1.0) - 1.0).abs()
}

fn smooth_lobe(phase: f64, center: f64, width: f64) -> f64 {
    smoothstep(1.0 - (phase - center).abs() / width)
}

fn scale_color(color: &LedColor, value: f64) -> LedColor {
    let value = value.clamp(0.0, 1.0);
    LedColor {
        r: clamp_u8(color.r as f64 * value),
        g: clamp_u8(color.g as f64 * value),
        b: clamp_u8(color.b as f64 * value),
    }
}

fn mix_color(a: &LedColor, b: &LedColor, t: f64) -> LedColor {
    LedColor {
        r: clamp_u8(lerp(a.r as f64, b.r as f64, t)),
        g: clamp_u8(lerp(a.g as f64, b.g as f64, t)),
        b: clamp_u8(lerp(a.b as f64, b.b as f64, t)),
    }
}

fn palette_color(palette: &[LedColor], phase: f64) -> LedColor {
    let position = phase.rem_euclid(1.0) * palette.len() as f64;
    let index = position.floor() as usize;
    mix_color(
        &palette[index],
        &palette[(index + 1) % palette.len()],
        smoothstep(position.fract()),
    )
}

fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> LedColor {
    let h = hue.rem_euclid(1.0) * 6.0;
    let sector = h.floor() as u8;
    let f = h.fract();
    let s = saturation.clamp(0.0, 1.0);
    let v = value.clamp(0.0, 1.0);
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match sector {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    LedColor {
        r: clamp_u8(r * 255.0),
        g: clamp_u8(g * 255.0),
        b: clamp_u8(b * 255.0),
    }
}

fn rgb_to_hsv(color: &LedColor) -> (f64, f64, f64) {
    let (r, g, b) = (
        color.r as f64 / 255.0,
        color.g as f64 / 255.0,
        color.b as f64 / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };
    (hue, if max == 0.0 { 0.0 } else { delta / max }, max)
}

fn hash_unit(index: usize, event: u64) -> f64 {
    let mut value = (index as u64)
        .wrapping_mul(0x9E3779B97F4A7C15)
        .wrapping_add(event.wrapping_mul(0xD1B54A32D192ED03))
        .wrapping_add(0xCAFEBABE);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D049BB133111EB);
    value ^= value >> 31;
    (value >> 11) as f64 / (1u64 << 53) as f64
}

fn simple_rand(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

fn fire_sim(heat: &mut [u8], seed: &mut u64) {
    let count = heat.len();
    if count == 0 {
        return;
    }
    for value in heat.iter_mut() {
        let cooling = if count < 3 {
            40 + simple_rand(seed) % 80
        } else {
            simple_rand(seed) % 30
        };
        *value = value.saturating_sub(cooling as u8);
    }
    for i in (2..count).rev() {
        heat[i] = ((heat[i - 1] as u16 + 2 * heat[i - 2] as u16) / 3) as u8;
    }
    let zone = (count / 4).max(2).min(count);
    for _ in 0..(count / 12).max(1) {
        if simple_rand(seed) % 100 < if count < 3 { 40 } else { 70 } {
            let index = simple_rand(seed) as usize % zone;
            heat[index] = heat[index].saturating_add((160 + simple_rand(seed) % 96) as u8);
        }
    }
}

fn heat_to_color(heat: f64) -> LedColor {
    let t = heat / 255.0;
    if t < 0.33 {
        LedColor {
            r: clamp_u8(255.0 * t / 0.33),
            g: 0,
            b: 0,
        }
    } else if t < 0.66 {
        LedColor {
            r: 255,
            g: clamp_u8(200.0 * (t - 0.33) / 0.33),
            b: 0,
        }
    } else {
        LedColor {
            r: 255,
            g: 200,
            b: clamp_u8(230.0 * (t - 0.66) / 0.34),
        }
    }
}

struct RunningGuard<'a>(&'a AtomicBool);

impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn run_effect<C, S>(
    mut renderer: FrameRenderer,
    running: &AtomicBool,
    mut current_color: C,
    mut output: S,
) where
    C: FnMut() -> LedColor,
    S: FnMut(&[LedColor]) -> bool,
{
    let _finished = RunningGuard(running);
    let started = Instant::now();
    while running.load(Ordering::Acquire) {
        let frame_started = Instant::now();
        let color = current_color();
        let frame = renderer.render(started.elapsed(), &color);
        if !running.load(Ordering::Acquire) || !output(frame) {
            break;
        }
        // HID already sleeps 20ms between segment packets. Only wait for the
        // unused part of the frame budget; elapsed time skips missed frames.
        let remaining = FRAME_INTERVAL.saturating_sub(frame_started.elapsed());
        if !remaining.is_zero() {
            thread::park_timeout(remaining);
        }
    }
}

fn send_frame(hid: &HidController, colors: &[LedColor], running: &AtomicBool) -> bool {
    if !running.load(Ordering::Acquire) {
        return false;
    }
    // Hardware brightness (0x87) already scales output; do not dim RGB twice.
    if let Err(error) = hid.send_per_led_colors(colors) {
        log::error!("effect frame error: {error}");
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const NEW_EFFECTS: &[&str] = &[
        "aurora",
        "ocean",
        "gradient",
        "theater_chase",
        "color_wipe",
        "scanner",
        "meteor",
        "twinkle",
        "fireworks",
        "rainbow_wave",
    ];

    #[test]
    fn catalog_exposes_exactly_nineteen_unique_animated_effects() {
        assert_eq!(AVAILABLE_EFFECTS.len(), 19);
        assert_eq!(AVAILABLE_EFFECTS.iter().collect::<HashSet<_>>().len(), 19);
        for name in NEW_EFFECTS {
            assert!(AVAILABLE_EFFECTS.contains(name), "missing {name}");
        }
        assert!(!AVAILABLE_EFFECTS.contains(&"static"));
    }

    #[test]
    fn public_start_rejects_invalid_counts_and_nonfinite_or_out_of_range_speeds() {
        let runner = EffectRunner::new();
        let hid = Arc::new(HidController::new());
        for name in ["pulse", "static"] {
            for count in [0, 255, u16::MAX] {
                assert!(
                    runner.start(name, hid.clone(), count, 5.0).is_err(),
                    "{name}: count {count}"
                );
            }
            for speed in [
                f64::NAN,
                f64::INFINITY,
                f64::NEG_INFINITY,
                0.0,
                0.99,
                10.01,
                11.0,
            ] {
                assert!(
                    runner.start(name, hid.clone(), 71, speed).is_err(),
                    "{name}: speed {speed}"
                );
            }
        }
        assert!(runner.start("unknown", hid, 71, 5.0).is_err());
        assert!(!runner.is_running());
        assert_eq!(runner.get_status(), None);
    }

    fn renderer(name: &str, count: usize, speed: f64) -> FrameRenderer {
        FrameRenderer::new(Effect::parse(name).unwrap(), count, speed)
    }

    fn rgb(colors: &[LedColor]) -> Vec<[u8; 3]> {
        colors.iter().map(|c| [c.r, c.g, c.b]).collect()
    }

    fn sample(
        name: &str,
        count: usize,
        speed: f64,
        seconds: f64,
        color: &LedColor,
    ) -> Vec<[u8; 3]> {
        rgb(renderer(name, count, speed).render(Duration::from_secs_f64(seconds), color))
    }

    #[test]
    fn rainbow_and_uniform_fades_match_hand_checked_fixed_times() {
        let color = LedColor {
            r: 200,
            g: 80,
            b: 20,
        };
        assert_eq!(
            sample("rainbow", 6, 1.0, 0.0, &color),
            vec![
                [255, 0, 0],
                [255, 255, 0],
                [0, 255, 0],
                [0, 255, 255],
                [0, 0, 255],
                [255, 0, 255],
            ]
        );
        assert_eq!(sample("pulse", 3, 1.0, 0.0, &color), vec![[100, 40, 10]; 3]);
        assert_eq!(sample("breathe", 3, 1.0, 0.0, &color), vec![[0, 0, 0]; 3]);
        assert_eq!(
            sample("breathe", 3, 1.0, 3.0, &color),
            vec![[200, 80, 20]; 3]
        );
    }

    #[test]
    fn chase_has_a_directional_tail_and_scanner_reflects_at_both_ends() {
        let red = LedColor { r: 255, g: 0, b: 0 };
        let chase = sample("chase", 12, 1.0, 0.5, &red);
        assert_eq!(chase[3], [255, 0, 0]);
        assert!(chase[2][0] > chase[1][0]);
        assert_eq!(chase[4], [0, 0, 0]);
        for (seconds, peak) in [(0.0, 0), (1.5, 4), (3.0, 8), (4.5, 4), (6.0, 0)] {
            let scanner = sample("scanner", 9, 1.0, seconds, &red);
            assert_eq!(scanner[peak], [255, 0, 0]);
            assert_eq!(
                scanner
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, c)| c[0])
                    .unwrap()
                    .0,
                peak
            );
        }
    }

    #[test]
    fn theater_marquee_moves_every_third_light_and_wipe_fills_a_prefix() {
        let blue = LedColor { r: 0, g: 0, b: 200 };
        for (seconds, offset) in [(0.0, 0), (4.0, 1)] {
            let theater = sample("theater_chase", 9, 1.0, seconds, &blue);
            for (i, c) in theater.iter().enumerate() {
                assert_eq!(c[2] == 200, i % 3 == offset);
            }
        }
        assert_eq!(sample("color_wipe", 9, 1.0, 0.0, &blue), vec![[0, 0, 0]; 9]);
        assert_eq!(
            sample("color_wipe", 9, 1.0, 3.0, &blue),
            vec![[0, 0, 200]; 9]
        );
        let partial = sample("color_wipe", 9, 1.0, 1.5, &blue);
        assert_eq!(partial[0], [0, 0, 200]);
        assert_eq!(partial[8], [0, 0, 0]);
        assert!(partial.windows(2).all(|p| p[0][2] >= p[1][2]));
    }

    #[test]
    fn all_effects_render_exact_tiny_and_maximum_layouts_without_going_permanently_dark() {
        let color = LedColor {
            r: 180,
            g: 110,
            b: 220,
        };
        for name in AVAILABLE_EFFECTS {
            for count in [1, 2, 3, 254] {
                for speed in [1.0, 10.0] {
                    let mut effect = renderer(name, count, speed);
                    let mut lit = false;
                    for seconds in [0.0, 0.37, 1.13, 3.17] {
                        let colors = effect.render(Duration::from_secs_f64(seconds), &color);
                        assert_eq!(colors.len(), count, "{name}, count {count}");
                        lit |= colors.iter().any(|c| c.r > 0 || c.g > 0 || c.b > 0);
                    }
                    assert!(lit, "{name} stays dark at count {count}, speed {speed}");
                }
            }
        }
    }

    #[test]
    fn additions_have_distinct_animation_and_respond_to_speed() {
        let color = LedColor {
            r: 160,
            g: 90,
            b: 230,
        };
        let mut signatures = HashSet::new();
        for name in NEW_EFFECTS {
            let early = sample(name, 31, 5.0, 0.37, &color);
            let late = sample(name, 31, 5.0, 1.23, &color);
            assert_ne!(early, late, "{name} does not animate");
            assert_ne!(
                sample(name, 31, 1.0, 0.73, &color),
                sample(name, 31, 10.0, 0.73, &color),
                "{name} ignores speed"
            );
            assert!(
                signatures.insert((early, late)),
                "{name} duplicates another effect"
            );
        }
    }

    #[test]
    fn colored_palettes_have_spatial_and_temporal_variation() {
        let color = LedColor {
            r: 180,
            g: 90,
            b: 30,
        };
        for name in ["aurora", "ocean", "gradient", "rainbow_wave"] {
            let first = sample(name, 254, 5.0, 0.37, &color);
            assert!(
                first.iter().collect::<HashSet<_>>().len() > 16,
                "{name} is a flat palette"
            );
            assert_ne!(first, sample(name, 254, 5.0, 1.23, &color));
        }
        let aurora = sample("aurora", 254, 5.0, 0.37, &color);
        assert!(aurora.iter().any(|c| c[1] as u16 > c[0] as u16 * 2));
        assert!(aurora.iter().any(|c| c[2] > c[1]));
        let ocean = sample("ocean", 254, 5.0, 0.37, &color);
        assert!(ocean.iter().all(|c| c[2] > c[0]));
    }

    #[test]
    fn gradient_retains_a_palette_with_desaturated_global_colors() {
        for value in [64, 255] {
            let color = LedColor {
                r: value,
                g: value,
                b: value,
            };
            let early = sample("gradient", 31, 5.0, 0.37, &color);
            assert!(
                early.iter().collect::<HashSet<_>>().len() > 8,
                "a neutral global color flattened the gradient"
            );
            assert_ne!(early, sample("gradient", 31, 5.0, 1.23, &color));
        }
        assert_eq!(
            sample("gradient", 31, 5.0, 0.73, &LedColor::default()),
            vec![[0, 0, 0]; 31]
        );
    }

    #[test]
    fn global_color_updates_are_read_on_each_render_without_double_dimming() {
        let red = LedColor { r: 200, g: 0, b: 0 };
        let green = LedColor { r: 0, g: 180, b: 0 };
        for name in [
            "pulse",
            "breathe",
            "chase",
            "chase_bounce",
            "wave",
            "sparkle",
            "heartbeat",
            "theater_chase",
            "color_wipe",
            "scanner",
            "meteor",
            "twinkle",
        ] {
            let mut effect = renderer(name, 31, 5.0);
            let time = Duration::from_millis(730);
            let a = rgb(effect.render(time, &red));
            let b = rgb(effect.render(time, &green));
            assert!(a.iter().any(|c| c[0] > 0), "{name} has no colored output");
            assert!(a.iter().all(|c| c[1] == 0 && c[2] == 0));
            assert!(b.iter().all(|c| c[0] == 0 && c[2] == 0));
            assert_ne!(a, b, "{name} ignores a live color change");
        }
        for name in ["gradient", "fireworks"] {
            assert_ne!(
                sample(name, 31, 5.0, 0.73, &red),
                sample(name, 31, 5.0, 0.73, &green)
            );
        }
    }

    #[test]
    fn sparkle_is_sparse_while_twinkle_keeps_a_dim_background() {
        let white = LedColor {
            r: 255,
            g: 255,
            b: 255,
        };
        let sparkle = sample("sparkle", 71, 5.0, 0.8, &white);
        let twinkle = sample("twinkle", 71, 5.0, 0.8, &white);
        assert!(sparkle.iter().any(|c| c[0] == 0));
        assert!(sparkle.iter().any(|c| c[0] > 100));
        assert!(twinkle.iter().all(|c| c[0] > 0));
    }

    #[test]
    fn fire_is_time_driven_and_varies_even_on_one_led() {
        let mut effect = renderer("fire", 1, 5.0);
        let color = LedColor::default();
        let first = rgb(effect.render(Duration::from_millis(120), &color));
        assert_eq!(
            first,
            rgb(effect.render(Duration::from_millis(120), &color))
        );
        let frames: HashSet<_> = (2..40)
            .map(|i| rgb(effect.render(Duration::from_millis(i * 120), &color)))
            .collect();
        assert!(frames.len() > 3, "one-LED fire never changes");
        assert!(frames.iter().any(|f| f[0] != [0, 0, 0]));
    }

    #[test]
    fn tiny_fire_layouts_do_not_flash_between_cold_and_hot_frames() {
        for count in [1, 2, 3] {
            let mut effect = renderer("fire", count, 10.0);
            let color = LedColor::default();
            let mut previous = rgb(effect.render(Duration::ZERO, &color));
            for i in 1..300 {
                let current = rgb(effect.render(Duration::from_millis(i * 30), &color));
                assert!(
                    current
                        .iter()
                        .zip(&previous)
                        .all(|(a, b)| a.iter().zip(b).all(|(&a, &b)| a.abs_diff(b) <= 80)),
                    "fire changes too abruptly on {count} LEDs"
                );
                previous = current;
            }
        }
    }

    #[test]
    fn slow_output_advances_animation_by_wall_time() {
        let running = AtomicBool::new(true);
        let started = Instant::now();
        let mut attempts = 0;
        run_effect(
            renderer("pulse", 1, 10.0),
            &running,
            || LedColor { r: 255, g: 0, b: 0 },
            |frame| {
                let expected = ((1.0
                    + (std::f64::consts::TAU * 1.5 * started.elapsed().as_secs_f64()).sin())
                    * 127.5)
                    .round() as u8;
                assert!(
                    frame[0].r.abs_diff(expected) <= 16,
                    "animation drifted behind the output clock"
                );
                attempts += 1;
                if attempts == 4 {
                    return false;
                }
                thread::sleep(Duration::from_millis(80));
                true
            },
        );
        assert_eq!(attempts, 4);
    }

    #[test]
    fn cancellation_while_reading_color_prevents_the_next_output() {
        let running = AtomicBool::new(true);
        run_effect(
            renderer("rainbow", 254, 5.0),
            &running,
            || {
                running.store(false, Ordering::Release);
                LedColor::default()
            },
            |_| panic!("a cancelled worker wrote another frame"),
        );
        assert!(!running.load(Ordering::Acquire));
    }

    #[test]
    fn repeated_rendering_reuses_the_frame_allocation_for_every_effect() {
        let color = LedColor {
            r: 200,
            g: 100,
            b: 50,
        };
        for name in AVAILABLE_EFFECTS {
            let mut effect = renderer(name, 254, 10.0);
            let address = effect.render(Duration::ZERO, &color).as_ptr();
            for i in 1..100 {
                assert_eq!(
                    effect
                        .render(Duration::from_millis(i * 33), &color)
                        .as_ptr(),
                    address,
                    "{name} reallocates"
                );
            }
        }
    }

    #[test]
    fn additions_avoid_abrupt_single_led_flashes_at_maximum_speed() {
        let white = LedColor {
            r: 255,
            g: 255,
            b: 255,
        };
        for name in NEW_EFFECTS {
            let mut effect = renderer(name, 1, 10.0);
            let mut previous = rgb(effect.render(Duration::ZERO, &white))[0];
            for i in 1..200 {
                let current = rgb(effect.render(Duration::from_millis(i * 30), &white))[0];
                assert!(
                    current
                        .iter()
                        .zip(previous)
                        .all(|(&a, b)| a.abs_diff(b) <= 80),
                    "{name} jumps from {previous:?} to {current:?}"
                );
                previous = current;
            }
        }
    }

    fn attach_worker(
        runner: &EffectRunner,
        running: Arc<AtomicBool>,
        worker: thread::JoinHandle<()>,
    ) {
        let mut state = runner.state.lock().unwrap();
        state.running = running;
        state.worker = Some(worker);
        state.active_name = Some("rainbow".into());
    }

    #[test]
    fn stop_cancels_and_waits_for_an_in_flight_write_to_finish() {
        use std::sync::atomic::AtomicUsize;
        use std::sync::mpsc;
        let runner = Arc::new(EffectRunner::new());
        let running = Arc::new(AtomicBool::new(true));
        let completed_writes = Arc::new(AtomicUsize::new(0));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (cancelled_tx, cancelled_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let flag = running.clone();
        let writes = completed_writes.clone();
        let worker = thread::spawn(move || {
            entered_tx.send(()).unwrap();
            while flag.load(Ordering::Acquire) {
                thread::park_timeout(Duration::from_millis(5));
            }
            cancelled_tx.send(()).unwrap();
            // Simulate the synchronous, multi-packet HID write already in progress.
            let _ = release_rx.recv();
            writes.fetch_add(1, Ordering::Relaxed);
        });
        attach_worker(&runner, running, worker);
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let (stopped_tx, stopped_rx) = mpsc::channel();
        let stopping_runner = runner.clone();
        let stopping = thread::spawn(move || {
            stopping_runner.stop();
            stopped_tx.send(()).unwrap();
        });
        cancelled_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            matches!(stopped_rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
            "stop returned before the write completed"
        );
        release_tx.send(()).unwrap();
        stopped_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        stopping.join().unwrap();
        assert_eq!(completed_writes.load(Ordering::Relaxed), 1);
        assert!(!runner.is_running());
        assert_eq!(runner.get_status(), None);
        assert!(runner.state.lock().unwrap().worker.is_none());
        runner.stop();
    }

    #[test]
    fn invalid_start_preserves_the_current_worker() {
        let runner = EffectRunner::new();
        let running = Arc::new(AtomicBool::new(true));
        let flag = running.clone();
        let worker = thread::spawn(move || {
            while flag.load(Ordering::Acquire) {
                thread::park_timeout(Duration::from_millis(5));
            }
        });
        attach_worker(&runner, running, worker);
        let hid = Arc::new(HidController::new());
        assert!(runner.start("pulse", hid.clone(), 71, f64::NAN).is_err());
        assert!(runner.start("static", hid.clone(), 0, 5.0).is_err());
        assert!(runner.start("unknown", hid, 71, 5.0).is_err());
        assert!(runner.is_running());
        assert_eq!(runner.get_status().as_deref(), Some("rainbow"));
        runner.stop();
    }

    #[test]
    fn dropping_the_runner_joins_its_worker() {
        let runner = EffectRunner::new();
        let running = Arc::new(AtomicBool::new(true));
        let flag = running.clone();
        let finished = Arc::new(AtomicBool::new(false));
        let completed = finished.clone();
        let worker = thread::spawn(move || {
            while flag.load(Ordering::Acquire) {
                thread::park_timeout(Duration::from_millis(5));
            }
            completed.store(true, Ordering::Release);
        });
        attach_worker(&runner, running, worker);
        drop(runner);
        assert!(finished.load(Ordering::Acquire));
    }

    #[test]
    fn output_failure_clears_running_and_stops_after_one_attempt() {
        let running = AtomicBool::new(true);
        let mut attempts = 0;
        run_effect(
            renderer("rainbow", 254, 5.0),
            &running,
            LedColor::default,
            |_| {
                attempts += 1;
                false
            },
        );
        assert_eq!(attempts, 1);
        assert!(!running.load(Ordering::Acquire));
    }
}
