use crate::hid::{HidController, LedColor};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub struct Ambilight {
    running: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
enum CaptureBackend {
    Grim,
    GnomeScreenshot,
}

impl Ambilight {
    pub fn new() -> Self {
        Ambilight {
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
        sections: [u16; 3],
        fps: u32,
    ) -> Result<(), String> {
        let backend = detect_backend()?;
        self.stop();
        thread::sleep(Duration::from_millis(120));
        self.running.store(true, Ordering::Relaxed);

        let running = self.running.clone();
        thread::spawn(move || {
            if let Err(e) = run_capture(hid, running.clone(), sections, fps, backend) {
                log::error!("ambilight error: {}", e);
            }
            running.store(false, Ordering::Relaxed);
        });
        Ok(())
    }
}

fn detect_backend() -> Result<CaptureBackend, String> {
    let grim_test = Command::new("grim").args(["-t", "ppm", "-"]).output();
    if let Ok(o) = grim_test {
        if o.status.success() && !o.stdout.is_empty() {
            log::info!("ambilight: using grim backend");
            return Ok(CaptureBackend::Grim);
        }
    }
    let gs_test = Command::new("gnome-screenshot").args(["-f", "/tmp/ols_test.png"]).output();
    if let Ok(o) = gs_test {
        if o.status.success() {
            let _ = std::fs::remove_file("/tmp/ols_test.png");
            log::info!("ambilight: using gnome-screenshot backend (lower FPS)");
            return Ok(CaptureBackend::GnomeScreenshot);
        }
    }
    Err("No screen capture tool available. Install grim (wlroots) or gnome-screenshot (GNOME).".into())
}

fn run_capture(
    hid: Arc<HidController>,
    running: Arc<AtomicBool>,
    sections: [u16; 3],
    fps: u32,
    backend: CaptureBackend,
) -> Result<(), String> {
    let frame_ms = (1000 / fps.max(1).min(30)) as u64;
    let mut prev_colors: Option<Vec<LedColor>> = None;
    let smooth = 0.5_f64;
    let tmp_path = "/tmp/ols_ambi.png";

    while running.load(Ordering::Relaxed) {
        let start = std::time::Instant::now();

        let (width, height, pixels) = match capture_screen(backend, tmp_path) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("screen capture failed: {}", e);
                thread::sleep(Duration::from_millis(2000));
                if !running.load(Ordering::Relaxed) {
                    break;
                }
                continue;
            }
        };

        if !running.load(Ordering::Relaxed) {
            break;
        }

        let colors = sample_border_colors(&pixels, width, height, &sections);

        let final_colors = if let Some(ref prev) = prev_colors {
            colors
                .iter()
                .zip(prev.iter())
                .map(|(c, p)| LedColor {
                    r: (c.r as f64 * smooth + p.r as f64 * (1.0 - smooth)).round() as u8,
                    g: (c.g as f64 * smooth + p.g as f64 * (1.0 - smooth)).round() as u8,
                    b: (c.b as f64 * smooth + p.b as f64 * (1.0 - smooth)).round() as u8,
                })
                .collect()
        } else {
            colors.clone()
        };

        prev_colors = Some(final_colors.clone());

        if let Err(e) = hid.send_per_led_colors(&final_colors) {
            log::error!("ambilight send error: {}", e);
            break;
        }

        let elapsed = start.elapsed().as_millis() as u64;
        if elapsed < frame_ms {
            thread::sleep(Duration::from_millis(frame_ms - elapsed));
        }
    }
    let _ = std::fs::remove_file(tmp_path);
    Ok(())
}

fn capture_screen(backend: CaptureBackend, tmp_path: &str) -> Result<(usize, usize, Vec<u8>), String> {
    match backend {
        CaptureBackend::Grim => capture_grim(),
        CaptureBackend::GnomeScreenshot => capture_gnome_screenshot(tmp_path),
    }
}

fn capture_grim() -> Result<(usize, usize, Vec<u8>), String> {
    let result = Command::new("grim")
        .args(["-t", "ppm", "-"])
        .output()
        .map_err(|e| format!("failed to run grim: {}", e))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("grim failed: {}", stderr));
    }
    if result.stdout.is_empty() {
        return Err("grim returned empty output".into());
    }
    parse_ppm(&result.stdout)
}

fn capture_gnome_screenshot(tmp_path: &str) -> Result<(usize, usize, Vec<u8>), String> {
    let result = Command::new("gnome-screenshot")
        .args(["-f", tmp_path])
        .output()
        .map_err(|e| format!("failed to run gnome-screenshot: {}", e))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("gnome-screenshot failed: {}", stderr));
    }

    let png_data = std::fs::read(tmp_path)
        .map_err(|e| format!("failed to read screenshot: {}", e))?;

    decode_png(&png_data)
}

fn decode_png(data: &[u8]) -> Result<(usize, usize, Vec<u8>), String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(data));
    let mut reader = decoder.read_info().map_err(|e| format!("PNG decode error: {}", e))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("PNG frame error: {}", e))?;
    let width = info.width as usize;
    let height = info.height as usize;

    let rgb = match info.color_type {
        png::ColorType::Rgb => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgba => {
            buf[..info.buffer_size()]
                .chunks_exact(4)
                .flat_map(|px| [px[0], px[1], px[2]])
                .collect()
        }
        _ => return Err(format!("unsupported PNG color type: {:?}", info.color_type)),
    };

    Ok((width, height, rgb))
}

fn parse_ppm(data: &[u8]) -> Result<(usize, usize, Vec<u8>), String> {
    if data.len() < 3 || &data[0..2] != b"P6" {
        return Err("not PPM P6".into());
    }
    let mut pos = 2;

    let skip_ws = |data: &[u8], mut p: usize| -> usize {
        loop {
            while p < data.len() && (data[p] == b' ' || data[p] == b'\n' || data[p] == b'\r' || data[p] == b'\t') {
                p += 1;
            }
            if p < data.len() && data[p] == b'#' {
                while p < data.len() && data[p] != b'\n' { p += 1; }
            } else {
                break;
            }
        }
        p
    };

    pos = skip_ws(data, pos);
    let (width, new_pos) = parse_num(data, pos)?;
    pos = skip_ws(data, new_pos);
    let (height, new_pos) = parse_num(data, pos)?;
    pos = skip_ws(data, new_pos);
    let (_maxval, new_pos) = parse_num(data, pos)?;
    pos = new_pos;
    if pos < data.len() && (data[pos] == b'\n' || data[pos] == b' ') {
        pos += 1;
    }

    let pixels = data[pos..].to_vec();
    Ok((width, height, pixels))
}

fn parse_num(data: &[u8], start: usize) -> Result<(usize, usize), String> {
    let mut pos = start;
    let mut n = 0usize;
    while pos < data.len() && data[pos] >= b'0' && data[pos] <= b'9' {
        n = n * 10 + (data[pos] - b'0') as usize;
        pos += 1;
    }
    if pos == start {
        return Err("expected number in PPM".into());
    }
    Ok((n, pos))
}

fn sample_border_colors(
    pixels: &[u8],
    width: usize,
    height: usize,
    sections: &[u16; 3],
) -> Vec<LedColor> {
    let left = sections[0] as usize;
    let top = sections[1] as usize;
    let right = sections[2] as usize;
    let total = left + top + right;
    let mut colors = Vec::with_capacity(total);

    let sample_depth = 40usize;

    let get_pixel = |x: usize, y: usize| -> (u8, u8, u8) {
        let idx = (y * width + x) * 3;
        if idx + 2 < pixels.len() {
            (pixels[idx], pixels[idx + 1], pixels[idx + 2])
        } else {
            (0, 0, 0)
        }
    };

    let avg_region = |x0: usize, y0: usize, w: usize, h: usize| -> LedColor {
        let step = ((w * h) / 32).max(1);
        let mut r_sum = 0u64;
        let mut g_sum = 0u64;
        let mut b_sum = 0u64;
        let mut count = 0u64;
        let mut idx = 0;
        for dy in 0..h {
            for dx in 0..w {
                if idx % step == 0 {
                    let (r, g, b) = get_pixel(
                        (x0 + dx).min(width.saturating_sub(1)),
                        (y0 + dy).min(height.saturating_sub(1)),
                    );
                    r_sum += r as u64;
                    g_sum += g as u64;
                    b_sum += b as u64;
                    count += 1;
                }
                idx += 1;
            }
        }
        if count == 0 {
            return LedColor::default();
        }
        LedColor {
            r: (r_sum / count) as u8,
            g: (g_sum / count) as u8,
            b: (b_sum / count) as u8,
        }
    };

    let depth = sample_depth.min(width / 4).min(height / 4);
    let seg_h = if left > 0 { height / left } else { 0 };
    let seg_w = if top > 0 { width / top } else { 0 };
    let seg_h_r = if right > 0 { height / right } else { 0 };

    // Left: bottom to top
    for i in 0..left {
        let y = height.saturating_sub((i + 1) * seg_h);
        colors.push(avg_region(0, y, depth, seg_h));
    }
    // Top: left to right
    for i in 0..top {
        let x = i * seg_w;
        colors.push(avg_region(x, 0, seg_w, depth));
    }
    // Right: top to bottom
    for i in 0..right {
        let y = i * seg_h_r;
        colors.push(avg_region(width.saturating_sub(depth), y, depth, seg_h_r));
    }

    colors
}
