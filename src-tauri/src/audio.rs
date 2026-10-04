use crate::hid::{HidController, LedColor};
use crate::audio_effects::{Options, Palette, Renderer};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use libpulse_binding as pulse;
use libpulse_simple_binding as psimple;

const SAMPLE_RATE: u32 = 44100;
const BUFFER_FRAMES: usize = 512;
const NUM_BANDS: usize = 8;

#[derive(Clone, Default, serde::Serialize)]
pub struct AudioMetrics {
    pub frames: u64,
    pub level: f64,
    pub source: String,
    pub output_brightness: u8,
    pub palette: String,
    pub mode: String,
    pub distinct_colors: usize,
}

pub struct AudioSync {
    worker: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
    metrics: Arc<std::sync::Mutex<AudioMetrics>>,
    error: Arc<std::sync::Mutex<Option<String>>>,
    running: std::sync::Mutex<Arc<AtomicBool>>,
}

impl AudioSync {
    pub fn new() -> Self {
        AudioSync {
            worker: std::sync::Mutex::new(None),
            metrics: Arc::new(std::sync::Mutex::new(AudioMetrics::default())),
            error: Arc::new(std::sync::Mutex::new(None)),
            running: std::sync::Mutex::new(Arc::new(AtomicBool::new(false))),
        }
    }

    pub fn metrics(&self) -> AudioMetrics { self.metrics.lock().unwrap().clone() }

    pub fn error(&self) -> Option<String> { self.error.lock().unwrap().clone() }

    pub fn is_running(&self) -> bool {
        self.running.lock().unwrap().load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.running.lock().unwrap().store(false, Ordering::Relaxed);
        if let Some(worker)=self.worker.lock().unwrap().take() {
            if worker.join().is_err() { *self.error.lock().unwrap()=Some("Capture worker panicked. Restart sync.".into()); }
        }
    }

    pub fn start(
        &self,
        hid: Arc<HidController>,
        led_count: u16,
        mode: &str,
        sensitivity: f64,
        source: Option<String>,
        options: Options,
    ) -> Result<(), String> {
        self.stop();
        thread::sleep(Duration::from_millis(200));
        *self.running.lock().unwrap() = Arc::new(AtomicBool::new(true));

        *self.error.lock().unwrap() = None;
        *self.metrics.lock().unwrap() = AudioMetrics::default();
        let metrics = self.metrics.clone();
        let error = self.error.clone();
        let running = self.running.lock().unwrap().clone();
        let mode = mode.to_string();
        let source = source.clone();

        let worker=thread::spawn(move || {
            if let Err(e) = run_audio_capture(hid, running.clone(), led_count, &mode, sensitivity, source.as_deref(), metrics, options) {
                log::error!("audio sync error: {}", e);
                *error.lock().unwrap() = Some(e);
            }
            running.store(false, Ordering::Relaxed);
        });
        *self.worker.lock().unwrap()=Some(worker);
        Ok(())
    }

    pub fn list_sources() -> Result<Vec<(String, String)>, String> {
        let output = std::process::Command::new("pactl")
            .args(["--format=json", "list", "sources"]).output()
            .map_err(|e| format!("Cannot discover audio sources: pactl could not start ({e})"))?;
        if !output.status.success() {
            return Err(format!("Audio source discovery failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
        parse_sources(&output.stdout)
    }

}

fn parse_sources(data: &[u8]) -> Result<Vec<(String, String)>, String> {
    let entries: serde_json::Value = serde_json::from_slice(data).map_err(|e| format!("Invalid audio device list: {e}"))?;
    let list = entries.as_array().ok_or("Audio device list is not an array")?;
    Ok(list.iter().filter_map(|s| {
        let name = s["name"].as_str()?;
        let description = s["description"].as_str().unwrap_or(name);
        let kind = if name.ends_with(".monitor") { "Playback" } else { "Microphone / input" };
        Some((name.to_owned(), format!("{kind}: {description}")))
    }).collect())
}

fn run_audio_capture(
    hid: Arc<HidController>,
    running: Arc<AtomicBool>,
    led_count: u16,
    mode: &str,
    sensitivity: f64,
    source: Option<&str>,
    metrics: Arc<std::sync::Mutex<AudioMetrics>>,
    options: Options,
) -> Result<(), String> {
    let spec = pulse::sample::Spec {
        format: pulse::sample::Format::S16le,
        channels: 1,
        rate: SAMPLE_RATE,
    };

    let default_monitor = std::process::Command::new("pactl").arg("get-default-sink").output()
        .ok().filter(|o| o.status.success())
        .map(|o| format!("{}.monitor", String::from_utf8_lossy(&o.stdout).trim()))
        .unwrap_or_else(|| "@DEFAULT_MONITOR@".into());
    let source_name = source.filter(|s| !s.is_empty()).unwrap_or(&default_monitor);
    {
        let mut stats=metrics.lock().unwrap();
        stats.source=source_name.to_owned();
        stats.mode=mode.to_owned();
        stats.palette=format!("{:?}",options.palette).to_lowercase();
    }
    log::info!("audio sync: connecting to source '{}', mode={}, sens={}", source_name, mode, sensitivity);

    let attr = pulse::def::BufferAttr {
        maxlength: (BUFFER_FRAMES * 2 * 4) as u32,
        tlength: u32::MAX,
        prebuf: u32::MAX,
        minreq: u32::MAX,
        fragsize: (BUFFER_FRAMES * 2) as u32,
    };

    let recorder = psimple::Simple::new(
        None,
        "snzhy-OpenSycnlights",
        pulse::stream::Direction::Record,
        Some(source_name),
        "audio_sync",
        &spec,
        None,
        Some(&attr),
    )
    .map_err(|e| format!("PulseAudio connect failed (source: {}): {}", source_name, e))?;

    let latest = std::sync::Mutex::new(None::<(f64, Vec<f64>)>);
    let capture_error = std::sync::Mutex::new(None::<String>);
    thread::scope(|scope| {
        let producer = scope.spawn(|| {
            let mut buf = vec![0u8; BUFFER_FRAMES * 2];
            let mut smooth_level = 0.;
            let mut smooth_bands = vec![0.; NUM_BANDS];
            while running.load(Ordering::Relaxed) {
                if let Err(e) = recorder.read(&mut buf) {
                    *capture_error.lock().unwrap() = Some(format!("Audio capture read failed: {e}"));
                    running.store(false, Ordering::Relaxed);
                    break;
                }
                let samples: Vec<f64> = buf.chunks_exact(2).map(|c|
                    i16::from_le_bytes([c[0],c[1]]) as f64 / 32768.).collect();
                let rms = (samples.iter().map(|s|s*s).sum::<f64>()/samples.len() as f64).sqrt();
                let level = response_level(rms,sensitivity);
                let factor = if level > smooth_level {0.2} else {0.05};
                smooth_level += (level-smooth_level)*factor;
                if mode == "spectrum" {
                    let bands = simple_bands(&samples,sensitivity);
                    for i in 0..NUM_BANDS {
                        let factor = if bands[i]>smooth_bands[i] {0.2} else {0.05};
                        smooth_bands[i] += (bands[i]-smooth_bands[i])*factor;
                    }
                }
                // One replaceable snapshot, never a queue of old sound samples.
                *latest.lock().unwrap() = Some((smooth_level,smooth_bands.clone()));
                metrics.lock().unwrap().level = smooth_level;
            }
        });
        let result = (|| -> Result<(), String> {
            let mut renderer=Renderer::default();
            let mut last_frame=std::time::Instant::now();
            let mut previous_energy: Option<([u8;3],u8)> = None;
            let mut previous_colors: Option<Vec<LedColor>> = None;
            while running.load(Ordering::Relaxed) {
                let start = std::time::Instant::now();
                let snapshot = latest.lock().unwrap().clone();
                if let Some((level,bands)) = snapshot {
                    let color = hid.get_global_color();
                    let count = led_count as usize;
                    if !running.load(Ordering::Relaxed) { break; }
                    if mode == "energy" && options.palette==Palette::Selected {
                        let rgb = [color.r,color.g,color.b];
                        // Energy is a single selected color. Keep the firmware
                        // in static mode and vary hardware brightness, avoiding
                        // a screen-mode restart on every identical audio frame.
                        let target = hid.get_brightness();
                        let response=if level<=options.noise_gate {0.} else {level};
                        let brightness = if target==0 {0} else {((response*target as f64).round() as u8).max(1)};
                        if previous_energy != Some((rgb,brightness)) {
                            hid.refresh_static_brightness(&color,brightness)?;
                            metrics.lock().unwrap().output_brightness = brightness;
                        }
                        previous_energy = Some((rgb,brightness));
                    } else {
                        let now=std::time::Instant::now();
                        let colors=renderer.render(mode,count,level,&bands,&color,&options,now.duration_since(last_frame).as_secs_f64());
                        last_frame=now;
                        metrics.lock().unwrap().distinct_colors=colors.iter().map(|c|(c.r,c.g,c.b)).collect::<std::collections::HashSet<_>>().len();
                        let unchanged = previous_colors.as_ref().is_some_and(|p|
                            p.iter().zip(&colors).all(|(a,b)|a.r==b.r&&a.g==b.g&&a.b==b.b));
                        if !unchanged {
                            hid.send_screen_colors(&colors).map_err(|e|format!("Audio LED write failed: {e}"))?;
                            previous_colors = Some(colors);
                        }
                    }
                    metrics.lock().unwrap().frames += 1;
                }
                thread::sleep(Duration::from_millis(33).saturating_sub(start.elapsed()));
            }
            Ok(())
        })();
        running.store(false, Ordering::Relaxed);
        producer.join().map_err(|_| "Audio capture worker panicked".to_string())?;
        if mode == "energy" && options.palette==Palette::Selected && hid.is_open() {
            hid.refresh_static_brightness(&hid.get_global_color(),hid.get_brightness())?;
        }
        result?;
        if let Some(e) = capture_error.lock().unwrap().take() { return Err(e); }
        Ok(())
    })
}

fn response_level(rms: f64, sensitivity: f64) -> f64 {
    // Smooth compression keeps loud music responsive instead of clipping all
    // peaks to the same 100% value. Silence remains zero.
    1.0 - (-rms * sensitivity * 6.0).exp()
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
            sum += magnitudes[k] * magnitudes[k];
        }
        // Sum band energy rather than averaging amplitude across bins. The old
        // average made wider bands almost black even when playback was audible.
        bands[b] = ((2.0 * sum).sqrt() * sensitivity * 6.0).min(1.0);
    }
    bands
}

fn freq_to_bin(freq: f64, n: usize) -> usize {
    (freq * n as f64 / SAMPLE_RATE as f64).round() as usize
}

#[cfg(test)]
mod source_tests {
    #[test]
    fn playback_monitors_are_distinct_from_microphones() {
        let entries = super::parse_sources(br#"[
            {"name":"usb.monitor","description":"Monitor of POPCORN CS012"},
            {"name":"usb.input","description":"POPCORN microphone"}
        ]"#).unwrap();
        assert_eq!(entries[0], ("usb.monitor".into(), "Playback: Monitor of POPCORN CS012".into()));
        assert!(entries[1].1.starts_with("Microphone / input:"));
        assert!(super::parse_sources(b"invalid").is_err());
    }
}

#[cfg(test)]
mod signal_tests {
    #[test]
    fn quiet_audible_tone_produces_visible_spectrum_and_silence_is_zero() {
        let samples: Vec<_> = (0..super::BUFFER_FRAMES).map(|i|
            (2.0*std::f64::consts::PI*440.0*i as f64/super::SAMPLE_RATE as f64).sin()*0.08).collect();
        let bands = super::simple_bands(&samples, 1.0);
        assert!(bands[2] > 0.2, "440 Hz should be visible in the 250–500 Hz band: {bands:?}");
        assert!(super::simple_bands(&vec![0.;super::BUFFER_FRAMES],1.).iter().all(|b| *b==0.));
    }
}

#[cfg(test)]
mod response_tests {
    #[test]
    fn loud_music_keeps_dynamics_instead_of_clipping_flat() {
        assert_eq!(super::response_level(0.,1.),0.);
        let quiet=super::response_level(0.08,1.);
        let loud=super::response_level(0.3,1.);
        let louder=super::response_level(0.5,1.);
        assert!(quiet>0. && quiet<loud && loud<louder && louder<1.);
        assert!(super::response_level(0.08,2.)>quiet);
    }
}
