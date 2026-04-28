use crate::hid::{HidController, LedColor};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use libpulse_binding as pulse;
use libpulse_simple_binding as psimple;

const SAMPLE_RATE: u32 = 44100;
const BUFFER_FRAMES: usize = 512;
const NUM_BANDS: usize = 8;

pub struct AudioSync {
    running: Arc<AtomicBool>,
}

impl AudioSync {
    pub fn new() -> Self {
        AudioSync {
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    pub fn start(
        &self,
        hid: Arc<HidController>,
        led_count: u16,
        mode: &str,
        sensitivity: f64,
        source: Option<String>,
    ) -> Result<(), String> {
        self.stop();
        thread::sleep(Duration::from_millis(200));
        self.running.store(true, Ordering::Relaxed);

        let running = self.running.clone();
        let mode = mode.to_string();
        let source = source.clone();

        thread::spawn(move || {
            if let Err(e) = run_audio_capture(hid, running.clone(), led_count, &mode, sensitivity, source.as_deref()) {
                log::error!("audio sync error: {}", e);
            }
            running.store(false, Ordering::Relaxed);
        });
        Ok(())
    }

    pub fn list_sources() -> Vec<(String, String)> {
        let output = std::process::Command::new("pactl")
            .args(["list", "short", "sources"])
            .output();
        match output {
            Ok(o) if o.status.success() => {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter_map(|line| {
                        let parts: Vec<&str> = line.split('\t').collect();
                        if parts.len() >= 2 {
                            let name = parts[1].to_string();
                            let label = if name.contains(".monitor") {
                                format!("Monitor: {}", name.split('.').next().unwrap_or(&name))
                            } else {
                                format!("Input: {}", name.split('.').next().unwrap_or(&name))
                            };
                            Some((name, label))
                        } else {
                            None
                        }
                    })
                    .collect()
            }
            _ => vec![],
        }
    }
}

fn run_audio_capture(
    hid: Arc<HidController>,
    running: Arc<AtomicBool>,
    led_count: u16,
    mode: &str,
    sensitivity: f64,
    source: Option<&str>,
) -> Result<(), String> {
    let spec = pulse::sample::Spec {
        format: pulse::sample::Format::S16le,
        channels: 1,
        rate: SAMPLE_RATE,
    };

    let source_name = source.unwrap_or("@DEFAULT_MONITOR@");
    log::info!("audio sync: connecting to source '{}', mode={}, sens={}", source_name, mode, sensitivity);

    let attr = pulse::def::BufferAttr {
        maxlength: u32::MAX,
        tlength: u32::MAX,
        prebuf: u32::MAX,
        minreq: u32::MAX,
        fragsize: (BUFFER_FRAMES * 2) as u32,
    };

    let recorder = psimple::Simple::new(
        None,
        "openLightsSync",
        pulse::stream::Direction::Record,
        Some(source_name),
        "audio_sync",
        &spec,
        None,
        Some(&attr),
    )
    .map_err(|e| format!("PulseAudio connect failed (source: {}): {}", source_name, e))?;

    let count = led_count as usize;
    let mut buf = vec![0u8; BUFFER_FRAMES * 2];
    let mut smooth_level: f64 = 0.0;
    let mut smooth_bands = vec![0.0f64; NUM_BANDS];
    let smooth_up = 0.6;
    let smooth_down = 0.15;
    let mut frame_count = 0u64;

    while running.load(Ordering::Relaxed) {
        if let Err(e) = recorder.read(&mut buf) {
            log::error!("PulseAudio read error: {}", e);
            thread::sleep(Duration::from_millis(100));
            continue;
        }

        if !running.load(Ordering::Relaxed) {
            break;
        }

        let samples: Vec<f64> = buf
            .chunks_exact(2)
            .map(|chunk| {
                let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                sample as f64 / 32768.0
            })
            .collect();

        let rms = (samples.iter().map(|s| s * s).sum::<f64>() / samples.len() as f64).sqrt();
        let level = (rms * sensitivity * 6.0).min(1.0);

        let factor = if level > smooth_level { smooth_up } else { smooth_down };
        smooth_level = smooth_level * (1.0 - factor) + level * factor;

        let bands = simple_bands(&samples, sensitivity);
        for i in 0..NUM_BANDS {
            let f = if bands[i] > smooth_bands[i] { smooth_up } else { smooth_down };
            smooth_bands[i] = smooth_bands[i] * (1.0 - f) + bands[i] * f;
        }

        let bri = hid.get_brightness() as f64 / 255.0;
        let color = hid.get_global_color();
        let colors = match mode {
            "spectrum" => spectrum_mode(&smooth_bands, count, bri),
            "energy" => energy_mode(smooth_level, count, &color, bri),
            "beat" => beat_mode(smooth_level, count, &color, bri),
            _ => spectrum_mode(&smooth_bands, count, bri),
        };

        if let Err(e) = hid.send_per_led_colors(&colors) {
            log::error!("audio sync frame error: {}", e);
            break;
        }
        frame_count += 1;
        if frame_count % 500 == 0 {
            log::debug!("audio sync: frame {}, level={:.2}", frame_count, smooth_level);
        }
    }
    log::info!("audio sync stopped after {} frames", frame_count);
    Ok(())
}

fn simple_bands(samples: &[f64], sensitivity: f64) -> Vec<f64> {
    let n = samples.len();
    let half = n / 2;
    let band_edges: [usize; NUM_BANDS + 1] = [
        freq_to_bin(20.0, n),
        freq_to_bin(60.0, n),
        freq_to_bin(250.0, n),
        freq_to_bin(500.0, n),
        freq_to_bin(2000.0, n),
        freq_to_bin(4000.0, n),
        freq_to_bin(6000.0, n),
        freq_to_bin(12000.0, n),
        freq_to_bin(20000.0, n).min(half),
    ];

    let mut magnitudes = vec![0.0f64; half + 1];
    for k in 0..=band_edges[NUM_BANDS].min(half) {
        let w = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
        let mut re = 0.0;
        let mut im = 0.0;
        for (i, &s) in samples.iter().enumerate() {
            let a = w * i as f64;
            re += s * a.cos();
            im -= s * a.sin();
        }
        magnitudes[k] = (re * re + im * im).sqrt() / n as f64;
    }

    let mut bands = vec![0.0f64; NUM_BANDS];
    for b in 0..NUM_BANDS {
        let lo = band_edges[b].max(1);
        let hi = band_edges[b + 1].max(lo + 1).min(half);
        let mut sum = 0.0;
        for k in lo..hi {
            sum += magnitudes[k];
        }
        let num = (hi - lo).max(1) as f64;
        bands[b] = (sum / num * sensitivity * 20.0).min(1.0);
    }
    bands
}

fn freq_to_bin(freq: f64, n: usize) -> usize {
    (freq * n as f64 / SAMPLE_RATE as f64).round() as usize
}

fn spectrum_mode(bands: &[f64], led_count: usize, bri: f64) -> Vec<LedColor> {
    (0..led_count)
        .map(|i| {
            let pos = i as f64 / led_count as f64;
            let band_f = pos * (NUM_BANDS as f64 - 1.0);
            let band_lo = band_f.floor() as usize;
            let band_hi = (band_lo + 1).min(NUM_BANDS - 1);
            let frac = band_f - band_lo as f64;
            let val = bands[band_lo] * (1.0 - frac) + bands[band_hi] * frac;
            let v = val.max(0.01) * bri;
            hsv_to_rgb(pos, 1.0, v)
        })
        .collect()
}

fn energy_mode(level: f64, led_count: usize, color: &LedColor, bri: f64) -> Vec<LedColor> {
    let v = level * bri;
    (0..led_count)
        .map(|_| LedColor {
            r: (color.r as f64 * v).round() as u8,
            g: (color.g as f64 * v).round() as u8,
            b: (color.b as f64 * v).round() as u8,
        })
        .collect()
}

fn beat_mode(level: f64, led_count: usize, color: &LedColor, bri: f64) -> Vec<LedColor> {
    let fill = (level * led_count as f64).round() as usize;
    let mid = led_count / 2;

    (0..led_count)
        .map(|i| {
            let dist = if i >= mid { i - mid } else { mid - i };
            if dist * 2 <= fill {
                let fade = 1.0 - (dist as f64 / (fill as f64 / 2.0 + 0.01)).min(1.0);
                let v = fade * bri;
                LedColor {
                    r: (color.r as f64 * v).round() as u8,
                    g: (color.g as f64 * v).round() as u8,
                    b: (color.b as f64 * v).round() as u8,
                }
            } else {
                LedColor::default()
            }
        })
        .collect()
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
