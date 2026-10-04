use crate::hid::{HidController, LedColor};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Clone)]
pub struct CaptureOptions {
    pub output: Option<String>,
    pub smoothing: f64,
    pub depth: usize,
    pub reverse: bool,
}

#[derive(Clone, Default, serde::Serialize)]
pub struct ScreenMetrics {
    pub frames: u64,
    pub output: Option<String>,
    pub average_rgb: [u8; 3],
    pub section_rgb: [[u8; 3]; 3],
    pub achieved_fps: f64,
    pub frame_ms: f64,
}

pub struct Ambilight {
    worker: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
    metrics: Arc<std::sync::Mutex<ScreenMetrics>>,
    error: Arc<std::sync::Mutex<Option<String>>>,
    running: std::sync::Mutex<Arc<AtomicBool>>,
}

#[derive(Clone, Copy)]
enum CaptureBackend {
    Grim,
    GnomeScreenshot,
}

impl Ambilight {
    pub fn new() -> Self {
        Ambilight {
            worker: std::sync::Mutex::new(None),
            metrics: Arc::new(std::sync::Mutex::new(ScreenMetrics::default())),
            error: Arc::new(std::sync::Mutex::new(None)),
            running: std::sync::Mutex::new(Arc::new(AtomicBool::new(false))),
        }
    }

    pub fn metrics(&self) -> ScreenMetrics { self.metrics.lock().unwrap().clone() }

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
        sections: [u16; 3],
        fps: u32,
        mut options: CaptureOptions,
    ) -> Result<(), String> {
        if options.output.as_deref().unwrap_or("").is_empty() { options.output = default_output(); }
        let backend = detect_backend()?;
        self.stop();
        thread::sleep(Duration::from_millis(120));
        *self.running.lock().unwrap() = Arc::new(AtomicBool::new(true));

        *self.error.lock().unwrap() = None;
        *self.metrics.lock().unwrap() = ScreenMetrics { output: options.output.clone(), ..Default::default() };
        let metrics = self.metrics.clone();
        let error = self.error.clone();
        let running = self.running.lock().unwrap().clone();
        let worker=thread::spawn(move || {
            if let Err(e) = run_capture(hid, running.clone(), sections, fps, backend, options, metrics) {
                log::error!("ambilight error: {}", e);
                *error.lock().unwrap() = Some(e);
            }
            running.store(false, Ordering::Relaxed);
        });
        *self.worker.lock().unwrap()=Some(worker);
        Ok(())
    }
}

pub fn list_outputs() -> Result<Vec<serde_json::Value>, String> {
    let result = Command::new("hyprctl").args(["monitors", "-j"]).output().map_err(|e| e.to_string())?;
    if !result.status.success() { return Err("Hyprland monitor discovery unavailable".into()); }
    let monitors: serde_json::Value = serde_json::from_slice(&result.stdout).map_err(|e| e.to_string())?;
    Ok(monitors.as_array().ok_or("Invalid monitor response")?.iter()
        .filter(|m| m["disabled"].as_bool() != Some(true) && m["name"].as_str().is_some())
        .map(|m| serde_json::json!({"name":m["name"],"label":m["model"],"size":{"width":m["width"],"height":m["height"]}})).collect())
}

fn default_output() -> Option<String> {
    let result = Command::new("hyprctl").args(["monitors", "-j"]).output().ok()?;
    if !result.status.success() { return None; }
    let monitors: serde_json::Value = serde_json::from_slice(&result.stdout).ok()?;
    choose_output(&monitors)
}

fn choose_output(monitors: &serde_json::Value) -> Option<String> {
    let entries = monitors.as_array()?;
    let active: Vec<_> = entries.iter().filter(|m| m["disabled"].as_bool() != Some(true)).collect();
    let chosen = active.iter().find(|m| m["focused"].as_bool() == Some(true)).or_else(||active.first())?;
    chosen["name"].as_str().map(str::to_owned)
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
    options: CaptureOptions,
    metrics: Arc<std::sync::Mutex<ScreenMetrics>>,
) -> Result<(), String> {
    let frame_ms = (1000 / fps.max(1).min(30)) as u64;
    let mut prev_colors: Option<Vec<LedColor>> = None;
    let smooth = 1.0 - options.smoothing.clamp(0.0, 0.95);
    let tmp_path = "/tmp/ols_ambi.png";
    let session_start = std::time::Instant::now();

    while running.load(Ordering::Relaxed) {
        let start = std::time::Instant::now();

        let (width, height, pixels) = match capture_screen(backend, tmp_path, options.output.as_deref()) {
            Ok(v) => v,
            Err(e) => return Err(format!("Screen capture failed: {}",e)),
        };

        if !running.load(Ordering::Relaxed) {
            break;
        }

        let mut colors = sample_border_colors(&pixels, width, height, &sections, options.depth);
        if options.reverse { colors.reverse(); }

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

        hid.send_screen_colors(&final_colors).map_err(|e| format!("Screen LED write failed: {e}"))?;
        let mut stats = metrics.lock().unwrap();
        stats.frames += 1;
        if !final_colors.is_empty() {
            let n = final_colors.len() as u64;
            stats.average_rgb = [
                (final_colors.iter().map(|c| c.r as u64).sum::<u64>() / n) as u8,
                (final_colors.iter().map(|c| c.g as u64).sum::<u64>() / n) as u8,
                (final_colors.iter().map(|c| c.b as u64).sum::<u64>() / n) as u8,
            ];
        }
        let mut offset=0;
        for (index,count) in sections.iter().enumerate() {
            let end=(offset+*count as usize).min(final_colors.len());
            let group=&final_colors[offset..end];
            stats.section_rgb[index]=if group.is_empty() {[0;3]} else {
                let n=group.len() as u64;
                [(group.iter().map(|c|c.r as u64).sum::<u64>()/n) as u8,
                 (group.iter().map(|c|c.g as u64).sum::<u64>()/n) as u8,
                 (group.iter().map(|c|c.b as u64).sum::<u64>()/n) as u8]
            };
            offset=end;
        }
        stats.frame_ms=start.elapsed().as_secs_f64()*1000.;
        drop(stats);

        let elapsed = start.elapsed().as_millis() as u64;
        if elapsed < frame_ms {
            thread::sleep(Duration::from_millis(frame_ms - elapsed));
        }
        let mut stats=metrics.lock().unwrap();
        stats.achieved_fps=stats.frames as f64/session_start.elapsed().as_secs_f64().max(0.001);
    }
    let _ = std::fs::remove_file(tmp_path);
    Ok(())
}

fn capture_screen(backend: CaptureBackend, tmp_path: &str, output: Option<&str>) -> Result<(usize, usize, Vec<u8>), String> {
    match backend {
        CaptureBackend::Grim => capture_grim(output),
        CaptureBackend::GnomeScreenshot => capture_gnome_screenshot(tmp_path),
    }
}

fn capture_grim(output: Option<&str>) -> Result<(usize, usize, Vec<u8>), String> {
    let mut command = Command::new("grim");
    command.args(["-t", "ppm"]);
    if let Some(output) = output.filter(|v| !v.is_empty()) { command.args(["-o", output]); }
    let result = command.arg("-").output()
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
    let (maxval, new_pos) = parse_num(data, pos)?;
    if maxval != 255 || width == 0 || height == 0 {
        return Err("unsupported PPM dimensions or bit depth".into());
    }
    pos = new_pos;
    if pos >= data.len() || !data[pos].is_ascii_whitespace() {
        return Err("missing PPM pixel separator".into());
    }
    if data[pos] == b'\r' && data.get(pos + 1) == Some(&b'\n') { pos += 2; } else { pos += 1; }
    let count = width.checked_mul(height).and_then(|n| n.checked_mul(3))
        .ok_or("PPM dimensions overflow")?;
    if data.len() - pos != count { return Err("PPM pixel data is incomplete".into()); }
    Ok((width, height, data[pos..].to_vec()))
}

fn parse_num(data: &[u8], start: usize) -> Result<(usize, usize), String> {
    let mut pos = start;
    let mut n = 0usize;
    while pos < data.len() && data[pos] >= b'0' && data[pos] <= b'9' {
        n = n.checked_mul(10).and_then(|v| v.checked_add((data[pos] - b'0') as usize)).ok_or("PPM number overflow")?;
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
    sample_depth: usize,
) -> Vec<LedColor> {
    let left = sections[0] as usize;
    let top = sections[1] as usize;
    let right = sections[2] as usize;
    let total = left + top + right;
    let mut colors = Vec::with_capacity(total);


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
        for idx in (0..w*h).step_by(step) {
                    let dx=idx%w;
                    let dy=idx/w;
                    let (r, g, b) = get_pixel(
                        (x0 + dx).min(width.saturating_sub(1)),
                        (y0 + dy).min(height.saturating_sub(1)),
                    );
                    r_sum += r as u64;
                    g_sum += g as u64;
                    b_sum += b as u64;
                    count += 1;
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

    // Left: bottom to top
    for i in 0..left {
        let y = height-(i+1)*height/left;
        let end=height-i*height/left;
        colors.push(avg_region(0, y, depth, end-y));
    }
    // Top: left to right
    for i in 0..top {
        let x = i*width/top;
        let end=(i+1)*width/top;
        colors.push(avg_region(x, 0, end-x, depth));
    }
    // Right: top to bottom
    for i in 0..right {
        let y = i*height/right;
        let end=(i+1)*height/right;
        colors.push(avg_region(width.saturating_sub(depth), y, depth, end-y));
    }

    colors
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ppm_preserves_whitespace_pixel_bytes() {
        let (w, h, pixels) = parse_ppm(b"P6\n1 1\n255\n\n\r ").unwrap();
        assert_eq!((w,h), (1,1));
        assert_eq!(pixels, vec![10,13,32]);
    }
    #[test]
    fn ppm_rejects_truncated_and_unsupported_input() {
        assert!(parse_ppm(b"P6\n1 1\n255\n\x00").is_err());
        assert!(parse_ppm(b"P6\n1 1\n65535\n\x00\x00\x00").is_err());
        assert!(parse_ppm(b"P6\n999999999999999999999999999999 1\n255\n").is_err());
    }
    #[test]
    fn border_follows_left_top_right_order() {
        let mut pixels = vec![0; 8*8*3];
        for y in 0..8 { for x in 0..8 { let i=(y*8+x)*3; pixels[i]=if x==0 {255} else {0}; pixels[i+1]=if y==0 {255} else {0}; pixels[i+2]=if x==7 {255} else {0}; } }
        let colors=sample_border_colors(&pixels,8,8,&[1,1,1],1);
        assert_eq!(colors.len(),3); assert_eq!(colors[0].r,255); assert_eq!(colors[1].g,255); assert_eq!(colors[2].b,255);
    }
    #[test]
    fn uneven_top_segments_include_the_last_pixel_column() {
        let mut pixels=vec![0;5*8*3];
        pixels[4*3]=250;
        let colors=sample_border_colors(&pixels,5,8,&[0,2,0],1);
        assert_eq!(colors[0].r,0);
        assert_eq!(colors[1].r,83,"the final segment must cover columns 2, 3 and 4");
    }
}

#[cfg(test)]
mod output_tests {
    #[test]
    fn chooses_one_focused_monitor_excluding_disabled_outputs() {
        let monitors = serde_json::json!([
            {"name":"HDMI-A-1", "focused":false},
            {"name":"disabled", "focused":true, "disabled":true},
            {"name":"DP-1", "focused":true}
        ]);
        assert_eq!(super::choose_output(&monitors).as_deref(), Some("DP-1"));
        assert_eq!(super::choose_output(&serde_json::json!([])), None);
    }
}
