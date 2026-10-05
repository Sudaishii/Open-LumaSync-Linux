use crate::hid::{HidController, LedColor};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct CaptureOptions {
    pub output: Option<String>,
    pub smoothing: f64,
    pub depth: usize,
    pub reverse: bool,
    /// Fraction of the selected output's native resolution, in 0.1..=1.0.
    pub capture_scale: f64,
}

#[derive(Clone, Default, serde::Serialize)]
pub struct ScreenMetrics {
    pub frames: u64,
    pub output: Option<String>,
    pub average_rgb: [u8; 3],
    pub section_rgb: [[u8; 3]; 3],
    pub achieved_fps: f64,
    pub frame_ms: f64,
    pub capture_scale: f64,
    pub capture_width: usize,
    pub capture_height: usize,
    pub capture_ms: f64,
    pub processing_ms: f64,
    pub write_ms: f64,
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

    pub fn metrics(&self) -> ScreenMetrics {
        self.metrics.lock().unwrap().clone()
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap().clone()
    }

    pub fn is_running(&self) -> bool {
        self.running.lock().unwrap().load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.running.lock().unwrap().store(false, Ordering::Relaxed);
        if let Some(worker) = self.worker.lock().unwrap().take() {
            worker.thread().unpark();
            if worker.join().is_err() {
                *self.error.lock().unwrap() = Some("Capture worker panicked. Restart sync.".into());
            }
        }
    }

    pub fn start(
        &self,
        hid: Arc<HidController>,
        sections: [u16; 3],
        fps: u32,
        mut options: CaptureOptions,
    ) -> Result<(), String> {
        self.stop();
        *self.error.lock().unwrap() = None;
        if !options.capture_scale.is_finite() || !(0.1..=1.0).contains(&options.capture_scale) {
            let error = "Screen capture scale must be between 0.1 and 1.0".to_owned();
            *self.error.lock().unwrap() = Some(error.clone());
            return Err(error);
        }
        let monitors = discover_monitors();
        if options.output.as_deref().unwrap_or("").is_empty() {
            options.output = monitors.as_ref().and_then(choose_output);
        }
        let geometry = monitors
            .as_ref()
            .map(|m| monitor_geometry(m, options.output.as_deref()))
            .unwrap_or_default();
        *self.metrics.lock().unwrap() = ScreenMetrics {
            output: options.output.clone(),
            capture_scale: options.capture_scale,
            ..Default::default()
        };
        let backend = match detect_backend(options.output.as_deref(), &geometry) {
            Ok(backend) => backend,
            Err(error) => {
                *self.error.lock().unwrap() = Some(error.clone());
                return Err(error);
            }
        };
        *self.running.lock().unwrap() = Arc::new(AtomicBool::new(true));
        let metrics = self.metrics.clone();
        let error = self.error.clone();
        let running = self.running.lock().unwrap().clone();
        let worker = thread::spawn(move || {
            if let Err(e) = run_capture(
                hid,
                running.clone(),
                sections,
                fps,
                backend,
                options,
                geometry,
                metrics,
            ) {
                log::error!("ambilight error: {}", e);
                *error.lock().unwrap() = Some(e);
            }
            running.store(false, Ordering::Relaxed);
        });
        *self.worker.lock().unwrap() = Some(worker);
        Ok(())
    }
}

pub fn list_outputs() -> Result<Vec<serde_json::Value>, String> {
    let result = Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err("Hyprland monitor discovery unavailable".into());
    }
    let monitors: serde_json::Value =
        serde_json::from_slice(&result.stdout).map_err(|e| e.to_string())?;
    Ok(monitors.as_array().ok_or("Invalid monitor response")?.iter()
        .filter(|m| m["disabled"].as_bool() != Some(true) && m["name"].as_str().is_some())
        .map(|m| serde_json::json!({"name":m["name"],"label":m["model"],"size":{"width":m["width"],"height":m["height"]}})).collect())
}

fn discover_monitors() -> Option<serde_json::Value> {
    let result = Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .ok()?;
    if !result.status.success() {
        return None;
    }
    serde_json::from_slice(&result.stdout).ok()
}

fn choose_output(monitors: &serde_json::Value) -> Option<String> {
    let entries = monitors.as_array()?;
    let active: Vec<_> = entries
        .iter()
        .filter(|m| m["disabled"].as_bool() != Some(true) && m["name"].as_str().is_some())
        .collect();
    let chosen = active
        .iter()
        .find(|m| m["focused"].as_bool() == Some(true))
        .or_else(|| active.first())?;
    chosen["name"].as_str().map(str::to_owned)
}

#[derive(Clone, Copy)]
struct CaptureGeometry {
    output_scale: f64,
    native_size: Option<(usize, usize)>,
}

impl Default for CaptureGeometry {
    fn default() -> Self {
        Self {
            output_scale: 1.0,
            native_size: None,
        }
    }
}

impl CaptureGeometry {
    fn grim_scale(self, capture_scale: f64) -> f64 {
        self.output_scale * capture_scale
    }
}

fn monitor_geometry(monitors: &serde_json::Value, output: Option<&str>) -> CaptureGeometry {
    let Some(monitor) = monitors.as_array().and_then(|entries| {
        entries
            .iter()
            .find(|m| m["name"].as_str() == output && m["disabled"].as_bool() != Some(true))
    }) else {
        return CaptureGeometry::default();
    };
    let output_scale = monitor["scale"]
        .as_f64()
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(1.0);
    let native_size = monitor["width"]
        .as_u64()
        .zip(monitor["height"].as_u64())
        .and_then(|(w, h)| Some((usize::try_from(w).ok()?, usize::try_from(h).ok()?)))
        .filter(|(w, h)| *w > 0 && *h > 0)
        .map(|(w, h)| {
            if monitor["transform"].as_u64().unwrap_or(0) % 2 == 1 {
                (h, w)
            } else {
                (w, h)
            }
        });
    CaptureGeometry {
        output_scale,
        native_size,
    }
}

fn grim_command(output: Option<&str>, scale: f64) -> Command {
    let mut command = Command::new("grim");
    command.args(["-t", "ppm", "-s", &scale.to_string()]);
    if let Some(output) = output.filter(|v| !v.is_empty()) {
        command.args(["-o", output]);
    }
    command.arg("-");
    command
}

fn detect_backend(
    output: Option<&str>,
    geometry: &CaptureGeometry,
) -> Result<CaptureBackend, String> {
    // 1% of a single output is enough to verify capture and PPM parsing.
    choose_backend(
        output,
        capture_grim(output, geometry.grim_scale(0.01), None),
        || capture_command(Command::new("gnome-screenshot").arg("--version"), None),
    )
}

fn choose_backend(
    output: Option<&str>,
    grim_probe: Result<RgbFrame, String>,
    gnome_probe: impl FnOnce() -> Result<Output, String>,
) -> Result<CaptureBackend, String> {
    let grim_error = match grim_probe {
        Ok(_) => {
            log::info!("ambilight: using grim backend");
            return Ok(CaptureBackend::Grim);
        }
        Err(error) => error,
    };
    // GNOME's tool cannot honor -o. Never conceal a failed chosen output by
    // silently capturing the entire desktop with a different backend.
    if let Some(output) = output {
        return Err(format!(
            "Screen capture probe failed for output {output}: {grim_error}"
        ));
    }
    let gnome_error = match gnome_probe() {
        Ok(result) if result.status.success() => {
            log::info!(
                "ambilight: using gnome-screenshot backend (lower FPS); grim probe: {grim_error}"
            );
            return Ok(CaptureBackend::GnomeScreenshot);
        }
        Ok(result) => command_error("gnome-screenshot", &result),
        Err(error) => error,
    };
    Err(format!("No screen capture backend available. Install grim (wlroots) or gnome-screenshot (GNOME). grim: {grim_error}; GNOME: {gnome_error}"))
}

fn run_capture(
    hid: Arc<HidController>,
    running: Arc<AtomicBool>,
    sections: [u16; 3],
    fps: u32,
    backend: CaptureBackend,
    options: CaptureOptions,
    geometry: CaptureGeometry,
    metrics: Arc<std::sync::Mutex<ScreenMetrics>>,
) -> Result<(), String> {
    let period = frame_period(fps);
    let count = sections.iter().map(|n| *n as usize).sum();
    let mut colors = vec![LedColor::default(); count];
    let mut previous = vec![LedColor::default(); count];
    let mut have_previous = false;
    let smooth = 1.0 - options.smoothing.clamp(0.0, 0.95);
    let paths = match backend {
        CaptureBackend::Grim => None,
        CaptureBackend::GnomeScreenshot => Some(CapturePaths::new()?),
    };
    let session_start = Instant::now();
    let mut deadline = session_start;

    while running.load(Ordering::Relaxed) {
        let start = Instant::now();
        let frame = match backend {
            CaptureBackend::Grim => capture_grim(
                options.output.as_deref(),
                geometry.grim_scale(options.capture_scale),
                Some(&running),
            ),
            CaptureBackend::GnomeScreenshot => {
                capture_gnome_screenshot(paths.as_ref().unwrap(), options.capture_scale, &running)
            }
        };
        let frame = match frame {
            Ok(v) => v,
            Err(_) if !running.load(Ordering::Relaxed) => break,
            Err(e) => {
                return Err(format!(
                    "Screen capture failed (output {}): {e}",
                    options.output.as_deref().unwrap_or("desktop")
                ))
            }
        };
        let capture_ms = start.elapsed().as_secs_f64() * 1000.0;
        if !running.load(Ordering::Relaxed) {
            break;
        }
        let processing_start = Instant::now();
        let depth = scaled_depth(
            options.depth,
            frame.width,
            frame.height,
            options.capture_scale,
            frame.native_size.or(geometry.native_size),
        );
        sample_border_colors_into(
            frame.pixels(),
            frame.width,
            frame.height,
            &sections,
            depth,
            &mut colors,
        );
        if options.reverse {
            colors.reverse();
        }
        smooth_colors(&mut colors, &mut previous, smooth, have_previous);
        have_previous = true;
        let average_rgb = average_color(&colors);
        let mut section_rgb = [[0; 3]; 3];
        let mut offset = 0;
        for (index, count) in sections.iter().enumerate() {
            let end = offset + *count as usize;
            section_rgb[index] = average_color(&colors[offset..end]);
            offset = end;
        }
        let processing_ms = processing_start.elapsed().as_secs_f64() * 1000.0;
        if !running.load(Ordering::Relaxed) {
            break;
        }
        let write_start = Instant::now();
        hid.send_screen_colors(&colors)
            .map_err(|e| format!("Screen LED write failed: {e}"))?;
        let write_ms = write_start.elapsed().as_secs_f64() * 1000.0;
        let mut stats = metrics.lock().unwrap();
        stats.frames += 1;
        stats.average_rgb = average_rgb;
        stats.section_rgb = section_rgb;
        stats.capture_scale = options.capture_scale;
        stats.capture_width = frame.width;
        stats.capture_height = frame.height;
        stats.capture_ms = capture_ms;
        stats.processing_ms = processing_ms;
        stats.write_ms = write_ms;
        // As before, frame_ms is work time, excluding the pacing wait.
        stats.frame_ms = start.elapsed().as_secs_f64() * 1000.0;
        drop(stats);
        deadline = next_frame_deadline(deadline, period, Instant::now());
        wait_for_frame(deadline, &running);
        let mut stats = metrics.lock().unwrap();
        stats.achieved_fps = stats.frames as f64 / session_start.elapsed().as_secs_f64().max(0.001);
    }
    Ok(())
}

fn frame_period(fps: u32) -> Duration {
    Duration::from_secs_f64(1.0 / fps.clamp(1, 30) as f64)
}

fn next_frame_deadline(previous: Instant, period: Duration, now: Instant) -> Instant {
    (previous + period).max(now)
}

fn wait_for_frame(deadline: Instant, running: &AtomicBool) {
    while running.load(Ordering::Relaxed) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        thread::park_timeout(remaining);
    }
}

fn smooth_colors(
    colors: &mut [LedColor],
    previous: &mut [LedColor],
    smooth: f64,
    have_previous: bool,
) {
    for (color, prev) in colors.iter_mut().zip(previous.iter_mut()) {
        if have_previous {
            color.r = (color.r as f64 * smooth + prev.r as f64 * (1.0 - smooth)).round() as u8;
            color.g = (color.g as f64 * smooth + prev.g as f64 * (1.0 - smooth)).round() as u8;
            color.b = (color.b as f64 * smooth + prev.b as f64 * (1.0 - smooth)).round() as u8;
        }
        prev.r = color.r;
        prev.g = color.g;
        prev.b = color.b;
    }
}

fn average_color(colors: &[LedColor]) -> [u8; 3] {
    if colors.is_empty() {
        return [0; 3];
    }
    let mut sum = [0u64; 3];
    for color in colors {
        sum[0] += color.r as u64;
        sum[1] += color.g as u64;
        sum[2] += color.b as u64;
    }
    sum.map(|v| (v / colors.len() as u64) as u8)
}

struct RgbFrame {
    width: usize,
    height: usize,
    data: Vec<u8>,
    raster_offset: usize,
    native_size: Option<(usize, usize)>,
}

impl RgbFrame {
    fn pixels(&self) -> &[u8] {
        &self.data[self.raster_offset..]
    }
}

fn capture_grim(
    output: Option<&str>,
    scale: f64,
    running: Option<&AtomicBool>,
) -> Result<RgbFrame, String> {
    let result = capture_command(&mut grim_command(output, scale), running)?;
    if !result.status.success() {
        return Err(command_error("grim", &result));
    }
    if result.stdout.is_empty() {
        return Err("grim returned empty output".into());
    }
    let (width, height, raster_offset) = ppm_header(&result.stdout)?;
    // Keep the subprocess buffer; do not copy the entire PPM raster.
    Ok(RgbFrame {
        width,
        height,
        data: result.stdout,
        raster_offset,
        native_size: None,
    })
}

static CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct CapturePaths {
    directory: PathBuf,
    screenshot: PathBuf,
}

impl CapturePaths {
    fn new() -> Result<Self, String> {
        loop {
            let sequence = CAPTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir()
                .join(format!("ols-ambilight-{}-{sequence}", std::process::id()));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&directory) {
                Ok(()) => {
                    return Ok(Self {
                        screenshot: directory.join("capture.png"),
                        directory,
                    })
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("failed to create capture directory: {error}")),
            }
        }
    }
}

impl Drop for CapturePaths {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.screenshot);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

fn capture_gnome_screenshot(
    paths: &CapturePaths,
    scale: f64,
    running: &AtomicBool,
) -> Result<RgbFrame, String> {
    capture_png(
        Command::new("gnome-screenshot")
            .arg("-f")
            .arg(&paths.screenshot),
        paths,
        scale,
        running,
    )
}

fn capture_png(
    command: &mut Command,
    paths: &CapturePaths,
    scale: f64,
    running: &AtomicBool,
) -> Result<RgbFrame, String> {
    let result = (|| {
        let result = capture_command(command, Some(running))?;
        if !result.status.success() {
            return Err(command_error("gnome-screenshot", &result));
        }
        let png_data = std::fs::read(&paths.screenshot)
            .map_err(|e| format!("failed to read screenshot: {e}"))?;
        let (width, height, data) = decode_png(&png_data)?;
        resize_frame(
            RgbFrame {
                width,
                height,
                data,
                raster_offset: 0,
                native_size: Some((width, height)),
            },
            scale,
        )
    })();
    let _ = std::fs::remove_file(&paths.screenshot);
    result
}

fn resize_frame(frame: RgbFrame, scale: f64) -> Result<RgbFrame, String> {
    if scale == 1.0 {
        return Ok(frame);
    }
    let width = (frame.width as f64 * scale).round().max(1.0) as usize;
    let height = (frame.height as f64 * scale).round().max(1.0) as usize;
    let count = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(3))
        .ok_or("capture dimensions overflow")?;
    let mut data = vec![0; count];
    for y in 0..height {
        for x in 0..width {
            let source = ((y * frame.height / height) * frame.width + x * frame.width / width) * 3;
            let target = (y * width + x) * 3;
            data[target..target + 3].copy_from_slice(&frame.pixels()[source..source + 3]);
        }
    }
    Ok(RgbFrame {
        width,
        height,
        data,
        raster_offset: 0,
        native_size: frame.native_size,
    })
}

fn command_error(program: &str, result: &Output) -> String {
    let stderr = String::from_utf8_lossy(&result.stderr);
    let diagnostic = if stderr.trim().is_empty() {
        String::from_utf8_lossy(&result.stdout[..result.stdout.len().min(4096)])
    } else {
        stderr
    };
    format!(
        "{program} failed ({}): {}",
        result.status,
        diagnostic.trim()
    )
}

fn capture_command(command: &mut Command, running: Option<&AtomicBool>) -> Result<Output, String> {
    if running.is_some_and(|r| !r.load(Ordering::Relaxed)) {
        return Err("capture cancelled".into());
    }
    let program = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run {program}: {e}"))?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    thread::scope(|scope| {
        // Both pipes must be drained while the child runs, or a large PPM can
        // fill a pipe and deadlock try_wait(). Stop can still kill/reap it.
        let stdout_reader = scope.spawn(move || {
            let mut data = Vec::new();
            stdout.read_to_end(&mut data).map(|_| data)
        });
        let stderr_reader = scope.spawn(move || {
            let mut data = Vec::new();
            stderr.read_to_end(&mut data).map(|_| data)
        });
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => {}
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(format!("failed to wait for {program}: {error}"));
                }
            }
            let cancelled = running.is_some_and(|r| !r.load(Ordering::Relaxed));
            if cancelled || started.elapsed() >= Duration::from_secs(10) {
                let _ = child.kill();
                let _ = child.wait();
                break Err(if cancelled {
                    "capture cancelled".to_owned()
                } else {
                    format!("{program} capture timed out after 10 seconds")
                });
            }
            thread::park_timeout(Duration::from_millis(2));
        };
        let stdout = stdout_reader
            .join()
            .map_err(|_| format!("{program} stdout reader panicked"))?
            .map_err(|e| format!("failed to read {program} stdout: {e}"))?;
        let stderr = stderr_reader
            .join()
            .map_err(|_| format!("{program} stderr reader panicked"))?
            .map_err(|e| format!("failed to read {program} stderr: {e}"))?;
        Ok(Output {
            status: status?,
            stdout,
            stderr,
        })
    })
}

fn decode_png(data: &[u8]) -> Result<(usize, usize, Vec<u8>), String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(data));
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("PNG decode error: {}", e))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("PNG frame error: {}", e))?;
    let width = info.width as usize;
    let height = info.height as usize;

    buf.truncate(info.buffer_size());
    match info.color_type {
        png::ColorType::Rgb => {}
        png::ColorType::Rgba => {
            let count = buf.len() / 4;
            for i in 0..count {
                let rgb = [buf[i * 4], buf[i * 4 + 1], buf[i * 4 + 2]];
                buf[i * 3..i * 3 + 3].copy_from_slice(&rgb);
            }
            buf.truncate(count * 3);
        }
        _ => return Err(format!("unsupported PNG color type: {:?}", info.color_type)),
    }

    Ok((width, height, buf))
}

#[cfg(test)]
fn parse_ppm(data: &[u8]) -> Result<(usize, usize, Vec<u8>), String> {
    let (width, height, raster) = ppm_header(data)?;
    Ok((width, height, data[raster..].to_vec()))
}

fn ppm_header(data: &[u8]) -> Result<(usize, usize, usize), String> {
    let mut pos = skip_ppm_whitespace(data, 0);
    if data.get(pos..pos.saturating_add(2)) != Some(b"P6") {
        return Err("not PPM P6".into());
    }
    pos += 2;
    if !data.get(pos).is_some_and(ppm_whitespace) {
        return Err("missing PPM magic separator".into());
    }
    pos = skip_ppm_whitespace(data, pos);
    let (width, new_pos) = parse_num(data, pos)?;
    pos = skip_ppm_whitespace(data, new_pos);
    let (height, new_pos) = parse_num(data, pos)?;
    pos = skip_ppm_whitespace(data, new_pos);
    let (maxval, new_pos) = parse_num(data, pos)?;
    if maxval != 255 || width == 0 || height == 0 {
        return Err("unsupported PPM dimensions or bit depth".into());
    }
    pos = new_pos;
    if pos >= data.len() || !ppm_whitespace(&data[pos]) {
        return Err("missing PPM pixel separator".into());
    }
    let count = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(3))
        .ok_or("PPM dimensions overflow")?;
    // P6 has one raster separator. Never skip arbitrary whitespace here: it
    // can be RGB data. Use the exact raster length to disambiguate CR + an LF
    // pixel from a CRLF header separator.
    let crlf = data[pos] == b'\r' && data.get(pos + 1) == Some(&b'\n');
    pos += if crlf && data.len() - pos - 1 != count {
        2
    } else {
        1
    };
    if data.len() - pos != count {
        return Err("PPM pixel data is incomplete".into());
    }
    Ok((width, height, pos))
}

fn ppm_whitespace(byte: &u8) -> bool {
    matches!(*byte, b' ' | b'\t'..=b'\r')
}

fn skip_ppm_whitespace(data: &[u8], mut pos: usize) -> usize {
    loop {
        while data.get(pos).is_some_and(ppm_whitespace) {
            pos += 1;
        }
        if data.get(pos) != Some(&b'#') {
            return pos;
        }
        while data.get(pos).is_some_and(|v| *v != b'\n' && *v != b'\r') {
            pos += 1;
        }
    }
}

fn parse_num(data: &[u8], start: usize) -> Result<(usize, usize), String> {
    let mut pos = start;
    let mut n = 0usize;
    while pos < data.len() && data[pos] >= b'0' && data[pos] <= b'9' {
        n = n
            .checked_mul(10)
            .and_then(|v| v.checked_add((data[pos] - b'0') as usize))
            .ok_or("PPM number overflow")?;
        pos += 1;
    }
    if pos == start {
        return Err("expected number in PPM".into());
    }
    if !data
        .get(pos)
        .is_some_and(|v| ppm_whitespace(v) || *v == b'#')
    {
        return Err("missing PPM number separator".into());
    }
    Ok((n, pos))
}

fn scaled_depth(
    depth: usize,
    width: usize,
    height: usize,
    scale: f64,
    native_size: Option<(usize, usize)>,
) -> [usize; 2] {
    // grim -s is an absolute logical-output scale. Its argument is multiplied
    // by the monitor scale, while depth stays in native physical pixels.
    // Actual raster/native ratios also account for fractional-scale rounding.
    // Without compositor metadata, assume scale=1 for the monitor and infer
    // native dimensions from the requested fraction.
    let (native_width, native_height) = native_size.unwrap_or_else(|| {
        (
            (width as f64 / scale).round().max(1.0) as usize,
            (height as f64 / scale).round().max(1.0) as usize,
        )
    });
    let depth = depth
        .min((native_width / 4).max(1))
        .min((native_height / 4).max(1))
        .max(1);
    [
        (depth as f64 * width as f64 / native_width.max(1) as f64)
            .round()
            .max(1.0) as usize,
        (depth as f64 * height as f64 / native_height.max(1) as f64)
            .round()
            .max(1.0) as usize,
    ]
}

#[cfg(test)]
fn sample_border_colors(
    pixels: &[u8],
    width: usize,
    height: usize,
    sections: &[u16; 3],
    sample_depth: usize,
) -> Vec<LedColor> {
    let total = sections.iter().map(|n| *n as usize).sum();
    let mut colors = vec![LedColor::default(); total];
    let depth = sample_depth.min(width / 4).min(height / 4).max(1);
    sample_border_colors_into(pixels, width, height, sections, [depth; 2], &mut colors);
    colors
}

fn sample_border_colors_into(
    pixels: &[u8],
    width: usize,
    height: usize,
    sections: &[u16; 3],
    depth: [usize; 2],
    colors: &mut [LedColor],
) {
    let left = sections[0] as usize;
    let top = sections[1] as usize;
    let right = sections[2] as usize;
    if width == 0
        || height == 0
        || width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(3))
            .is_none_or(|n| n > pixels.len())
    {
        colors.fill(LedColor::default());
        return;
    }
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
        for idx in (0..w * h).step_by(step) {
            let dx = idx % w;
            let dy = idx / w;
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

    let side_depth = depth[0].clamp(1, width);
    let top_depth = depth[1].clamp(1, height);
    let mut offset = 0;

    // Left: bottom to top
    for i in 0..left {
        let y = height - (i + 1) * height / left;
        let end = height - i * height / left;
        let start = if end == y { end.saturating_sub(1) } else { y };
        colors[offset] = avg_region(0, start, side_depth, (end - y).max(1));
        offset += 1;
    }
    // Top: left to right
    for i in 0..top {
        let x = i * width / top;
        let end = (i + 1) * width / top;
        colors[offset] = avg_region(x.min(width - 1), 0, (end - x).max(1), top_depth);
        offset += 1;
    }
    // Right: top to bottom
    for i in 0..right {
        let y = i * height / right;
        let end = (i + 1) * height / right;
        colors[offset] = avg_region(
            width - side_depth,
            y.min(height - 1),
            side_depth,
            (end - y).max(1),
        );
        offset += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(colors: &[LedColor]) -> Vec<[u8; 3]> {
        colors.iter().map(|c| [c.r, c.g, c.b]).collect()
    }

    #[test]
    fn ppm_requires_a_separator_after_the_magic() {
        assert!(parse_ppm(b"P61 1\n255\n\x01\x02\x03").is_err());
    }

    #[test]
    fn ppm_accepts_header_whitespace_and_comments_with_crlf() {
        let (_, _, pixels) =
            parse_ppm(b" \t\r\nP6\r\n# dimensions\r\n1\t# height\r\n1\r\n255\r\n#\n ").unwrap();
        assert_eq!(pixels, [35, 10, 32]);
    }

    #[test]
    fn ppm_cr_separator_preserves_a_leading_lf_pixel() {
        let (_, _, pixels) = parse_ppm(b"P6\r1 1\r255\r\n\x01\x02").unwrap();
        assert_eq!(pixels, [10, 1, 2]);
    }

    #[test]
    fn ppm_accepts_every_ppm_header_whitespace_separator() {
        let (_, _, pixels) = parse_ppm(b"P6\x0b# comment\r1\x0c1\t255\x0b\x01\x02\x03").unwrap();
        assert_eq!(pixels, [1, 2, 3]);
    }

    #[test]
    fn ppm_preserves_every_leading_raster_byte_with_lf_cr_and_crlf_headers() {
        for separator in [b"\n".as_slice(), b"\r", b"\r\n", b" "] {
            for first in 0..=255 {
                let mut data = b"P6\n1 1\n255".to_vec();
                data.extend_from_slice(separator);
                data.extend_from_slice(&[first, 13, 35]);
                let (_, _, pixels) = parse_ppm(&data).unwrap();
                assert_eq!(pixels, [first, 13, 35]);
            }
        }
    }

    #[test]
    fn ppm_rejects_overflow_zero_and_extra_raster_bytes() {
        let overflow = format!("P6\n{} 2\n255\n", usize::MAX);
        assert!(parse_ppm(overflow.as_bytes()).is_err());
        for data in [
            b"P6\n0 1\n255\n".as_slice(),
            b"P6\n1 0\n255\n",
            b"P6\n1 1\n255\n\x01\x02\x03\x04",
            b"P6\n1 1\n255x\x01\x02\x03",
            b"P6\n1 1\n255\r\n\x01",
            b"P6\n1 # unfinished comment",
        ] {
            assert!(
                parse_ppm(data).is_err(),
                "accepted malformed input: {data:?}"
            );
        }
    }

    #[test]
    fn single_pixel_capture_supplies_every_led_in_rgb_order() {
        let (w, h, pixels) = parse_ppm(b"P6\n1 1\n255\n\x0c\x22\x38").unwrap();
        let colors = sample_border_colors(&pixels, w, h, &[2, 3, 2], 80);
        assert_eq!(rgb(&colors), vec![[12, 34, 56]; 7]);
    }

    #[test]
    fn tiny_top_capture_repeats_pixels_without_black_gaps() {
        let (w, h, pixels) = parse_ppm(b"P6\n2 1\n255\n\xff\x00\x00\x00\x00\xff").unwrap();
        let colors = sample_border_colors(&pixels, w, h, &[0, 4, 0], 1);
        assert_eq!(
            rgb(&colors),
            [[255, 0, 0], [255, 0, 0], [0, 0, 255], [0, 0, 255]]
        );
    }

    #[test]
    fn tiny_vertical_capture_preserves_left_bottom_to_top_and_right_top_to_bottom() {
        let (w, h, pixels) =
            parse_ppm(b"P6\n1 3\n255\n\xff\x00\x00\x00\xff\x00\x00\x00\xff").unwrap();
        let colors = sample_border_colors(&pixels, w, h, &[3, 0, 3], 1);
        assert_eq!(
            rgb(&colors),
            [
                [0, 0, 255],
                [0, 255, 0],
                [255, 0, 0],
                [255, 0, 0],
                [0, 255, 0],
                [0, 0, 255],
            ]
        );
    }

    #[test]
    fn scaled_capture_samples_the_same_full_resolution_edge_depth() {
        // A two-pixel red band in the 16px source becomes one pixel at 50%.
        // Keeping depth=2 in the smaller image would mix red and green.
        let mut full = vec![0; 16 * 16 * 3];
        let mut half = vec![0; 8 * 8 * 3];
        for (pixels, size, band) in [(&mut full, 16, 2), (&mut half, 8, 1)] {
            for y in 0..size {
                for x in 0..size {
                    let offset = (y * size + x) * 3;
                    pixels[offset..offset + 3].copy_from_slice(if x < band {
                        &[240, 0, 0]
                    } else {
                        &[0, 240, 0]
                    });
                }
            }
        }
        let geometry = CaptureGeometry {
            native_size: Some((16, 16)),
            ..Default::default()
        };
        let mut colors = vec![LedColor::default(); 1];
        sample_border_colors_into(
            &full,
            16,
            16,
            &[1, 0, 0],
            scaled_depth(2, 16, 16, 1.0, geometry.native_size),
            &mut colors,
        );
        assert_eq!(rgb(&colors), [[240, 0, 0]]);
        sample_border_colors_into(
            &half,
            8,
            8,
            &[1, 0, 0],
            scaled_depth(2, 8, 8, 0.5, geometry.native_size),
            &mut colors,
        );
        assert_eq!(rgb(&colors), [[240, 0, 0]]);
    }

    #[test]
    fn monitor_geometry_accounts_for_hidpi_rotation_and_rounding() {
        let monitors = serde_json::json!([
            {"name":"DP-1", "width":2160, "height":3840, "scale":2.0, "transform":1}
        ]);
        let geometry = monitor_geometry(&monitors, Some("DP-1"));
        assert_eq!(geometry.native_size, Some((3840, 2160)));
        assert_eq!(geometry.grim_scale(0.35), 0.7);
        assert_eq!(
            scaled_depth(80, 1344, 756, 0.35, geometry.native_size),
            [28, 28]
        );
        assert_eq!(scaled_depth(8, 35, 18, 0.35, Some((101, 51))), [3, 3]);
        assert_eq!(scaled_depth(80, 1, 1, 0.1, None), [1, 1]);
    }

    #[test]
    fn grim_arguments_target_only_the_selected_monitor_at_the_requested_scale() {
        let command = grim_command(Some("DP-1"), 0.7);
        let args: Vec<_> = command
            .get_args()
            .map(|v| v.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["-t", "ppm", "-s", "0.7", "-o", "DP-1", "-"]);
    }

    #[test]
    fn smoothing_updates_reused_colors_in_place() {
        let mut colors = vec![LedColor {
            r: 100,
            g: 20,
            b: 200,
        }];
        let pointer = colors.as_ptr();
        let mut previous = vec![LedColor { r: 0, g: 100, b: 0 }];
        smooth_colors(&mut colors, &mut previous, 0.5, true);
        assert_eq!(rgb(&colors), [[50, 60, 100]]);
        assert_eq!(rgb(&previous), rgb(&colors));
        assert_eq!(pointer, colors.as_ptr());
        colors[0] = LedColor { r: 240, g: 0, b: 0 };
        smooth_colors(&mut colors, &mut previous, 0.5, false);
        assert_eq!(
            rgb(&colors),
            [[240, 0, 0]],
            "first frame must not fade from black"
        );
        assert_eq!(rgb(&previous), rgb(&colors));
    }

    #[test]
    fn frame_period_keeps_fractional_milliseconds_and_subtracts_elapsed_work() {
        let start = std::time::Instant::now();
        let period = frame_period(30);
        assert!((period.as_secs_f64() - 1.0 / 30.0).abs() < 1e-9);
        assert_eq!(frame_period(0), Duration::from_secs(1));
        assert_eq!(frame_period(31), period);
        let deadline = next_frame_deadline(start, period, start + Duration::from_millis(12));
        assert_eq!(deadline, start + period);
        let overrun = start + Duration::from_millis(80);
        assert_eq!(next_frame_deadline(start, period, overrun), overrun);
    }

    #[test]
    fn stop_wakes_a_worker_paced_at_one_fps() {
        let ambilight = Ambilight::new();
        let running = ambilight.running.lock().unwrap().clone();
        running.store(true, Ordering::Relaxed);
        let (ready, received) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            ready.send(()).unwrap();
            wait_for_frame(std::time::Instant::now() + Duration::from_secs(1), &running);
        });
        *ambilight.worker.lock().unwrap() = Some(worker);
        received.recv().unwrap();
        let start = std::time::Instant::now();
        ambilight.stop();
        assert!(start.elapsed() < Duration::from_millis(250));
        assert!(!ambilight.is_running());
    }

    #[test]
    fn capture_paths_are_unique_and_cleaned_after_success_and_error() {
        let first = CapturePaths::new().unwrap();
        let second = CapturePaths::new().unwrap();
        assert_ne!(first.screenshot, second.screenshot);
        let first_path = first.screenshot.clone();
        let second_path = second.screenshot.clone();
        let first_dir = first.directory.clone();
        let second_dir = second.directory.clone();
        std::fs::write(&first_path, b"partial capture").unwrap();
        std::fs::write(&second_path, b"complete capture").unwrap();
        drop(first);
        assert!(!first_path.exists());
        assert!(!first_dir.exists());
        assert!(second_path.exists());
        drop(second);
        assert!(!second_dir.exists());
    }

    #[test]
    fn failed_capture_command_retains_exit_status_and_diagnostics() {
        let output = capture_command(
            Command::new("sh").args(["-c", "printf 'capture denied' >&2; exit 7"]),
            None,
        )
        .unwrap();
        let error = command_error("grim", &output);
        assert!(
            error.contains("7") && error.contains("capture denied"),
            "{error}"
        );
    }

    #[test]
    fn capture_command_drains_a_pipe_larger_than_its_capacity() {
        let output = capture_command(
            Command::new("sh").args([
                "-c",
                "head -c 262144 /dev/zero; head -c 262144 /dev/zero >&2",
            ]),
            None,
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), 262144);
        assert_eq!(output.stderr.len(), 262144);
    }

    #[test]
    fn cancellation_kills_and_reaps_an_active_capture_process() {
        let running = Arc::new(AtomicBool::new(true));
        let worker_running = running.clone();
        let (ready, received) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            ready.send(()).unwrap();
            capture_command(Command::new("sleep").arg("10"), Some(&worker_running))
        });
        received.recv().unwrap();
        thread::sleep(Duration::from_millis(25));
        let start = std::time::Instant::now();
        running.store(false, Ordering::Relaxed);
        worker.thread().unpark();
        assert!(worker.join().unwrap().is_err());
        assert!(start.elapsed() < Duration::from_millis(250));
    }

    #[test]
    fn a_failed_selected_output_cannot_fall_back_to_a_desktop_capture() {
        let result = choose_backend(
            Some("missing-DP"),
            Err("grim: unknown output".into()),
            || {
                panic!("GNOME cannot capture the selected output");
            },
        );
        let error = result
            .err()
            .expect("the selected output failure must be returned");
        assert!(
            error.contains("missing-DP") && error.contains("unknown output"),
            "{error}"
        );
    }

    #[test]
    fn failed_backend_probes_report_both_initial_errors() {
        let result = choose_backend(None, Err("grim: compositor denied capture".into()), || {
            Err("gnome-screenshot: command missing".into())
        });
        let error = result.err().unwrap();
        assert!(
            error.contains("compositor denied capture") && error.contains("command missing"),
            "{error}"
        );
    }

    #[test]
    fn png_capture_cleans_partial_files_on_command_and_decode_errors() {
        let paths = CapturePaths::new().unwrap();
        let running = AtomicBool::new(true);
        std::fs::write(&paths.screenshot, b"partial screenshot").unwrap();
        let result = capture_png(
            Command::new("sh").args(["-c", "printf 'capture denied' >&2; exit 7"]),
            &paths,
            0.35,
            &running,
        );
        let error = result.err().unwrap();
        assert!(error.contains("capture denied"), "{error}");
        assert!(!paths.screenshot.exists());
        std::fs::write(&paths.screenshot, b"invalid PNG").unwrap();
        assert!(capture_png(
            Command::new("sh").args(["-c", "exit 0"]),
            &paths,
            0.35,
            &running
        )
        .is_err());
        assert!(!paths.screenshot.exists());
    }

    #[test]
    fn png_rgb_and_rgba_capture_preserve_channels_and_remove_successful_files() {
        for color_type in [png::ColorType::Rgb, png::ColorType::Rgba] {
            let paths = CapturePaths::new().unwrap();
            {
                let file = std::fs::File::create(&paths.screenshot).unwrap();
                let mut encoder = png::Encoder::new(file, 1, 1);
                encoder.set_color(color_type);
                encoder.set_depth(png::BitDepth::Eight);
                let mut writer = encoder.write_header().unwrap();
                let bytes = if color_type == png::ColorType::Rgb {
                    &[12, 34, 56][..]
                } else {
                    &[12, 34, 56, 255][..]
                };
                writer.write_image_data(bytes).unwrap();
            }
            let frame = capture_png(
                Command::new("sh").args(["-c", "exit 0"]),
                &paths,
                0.35,
                &AtomicBool::new(true),
            )
            .unwrap();
            assert_eq!((frame.width, frame.height), (1, 1));
            assert_eq!(frame.pixels(), [12, 34, 56]);
            assert!(!paths.screenshot.exists());
            let colors = sample_border_colors(frame.pixels(), 1, 1, &[1, 1, 1], 80);
            assert_eq!(rgb(&colors), [[12, 34, 56]; 3]);
        }
    }

    #[test]
    fn rgb_edges_and_solid_frames_meet_live_expectations_at_all_ui_scales() {
        let sections = [15, 41, 15];
        for scale in [0.20_f64, 0.35, 0.50, 1.0] {
            let width = (320.0 * scale).round() as usize;
            let height = (180.0 * scale).round() as usize;
            let band = (16.0 * scale).round() as usize;
            let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
            for y in 0..height {
                for x in 0..width {
                    let color = if y < band {
                        [0, 240, 0]
                    } else if x < band {
                        [240, 0, 0]
                    } else if x >= width - band {
                        [0, 0, 240]
                    } else {
                        [0; 3]
                    };
                    ppm.extend_from_slice(&color);
                }
            }
            let (w, h, offset) = ppm_header(&ppm).unwrap();
            let mut colors = vec![LedColor::default(); 71];
            let pointer = colors.as_ptr();
            let depth = scaled_depth(8, w, h, scale, Some((320, 180)));
            sample_border_colors_into(&ppm[offset..], w, h, &sections, depth, &mut colors);
            let left = average_color(&colors[..15]);
            let top = average_color(&colors[15..56]);
            let right = average_color(&colors[56..]);
            assert!(left[0] > 170 && left[1] < 80, "scale={scale}: {left:?}");
            assert!(top[1] > 170, "scale={scale}: {top:?}");
            assert!(right[2] > 170 && right[1] < 80, "scale={scale}: {right:?}");
            for color in [[240, 0, 0], [0, 240, 0], [0, 0, 240]] {
                for pixel in ppm[offset..].chunks_exact_mut(3) {
                    pixel.copy_from_slice(&color);
                }
                sample_border_colors_into(&ppm[offset..], w, h, &sections, depth, &mut colors);
                assert_eq!(average_color(&colors), color);
                assert_eq!(colors.as_ptr(), pointer);
            }
        }
    }

    #[test]
    fn screen_metrics_serialize_original_and_new_measurements() {
        let metrics = ScreenMetrics {
            capture_scale: 0.35,
            capture_width: 672,
            capture_height: 378,
            capture_ms: 3.0,
            processing_ms: 0.2,
            write_ms: 1.0,
            frames: 1,
            average_rgb: [12, 34, 56],
            achieved_fps: 30.0,
            frame_ms: 4.2,
            ..Default::default()
        };
        let json = serde_json::to_value(metrics).unwrap();
        assert_eq!(json["capture_scale"], 0.35);
        assert_eq!(json["capture_width"], 672);
        assert_eq!(json["capture_height"], 378);
        assert_eq!(json["capture_ms"], 3.0);
        assert_eq!(json["processing_ms"], 0.2);
        assert_eq!(json["write_ms"], 1.0);
        assert_eq!(json["frames"], 1);
        assert_eq!(json["average_rgb"], serde_json::json!([12, 34, 56]));
        assert_eq!(json["achieved_fps"], 30.0);
        assert_eq!(json["frame_ms"], 4.2);
        assert_eq!(
            json["section_rgb"],
            serde_json::json!([[0, 0, 0], [0, 0, 0], [0, 0, 0]])
        );
        assert!(json.get("output").is_some());
    }

    #[test]
    fn startup_validation_retains_its_error_without_starting_capture() {
        let ambilight = Ambilight::new();
        for capture_scale in [0.09, 1.01, f64::NAN, f64::INFINITY] {
            let error = ambilight
                .start(
                    Arc::new(HidController::new()),
                    [1, 1, 1],
                    30,
                    CaptureOptions {
                        output: None,
                        smoothing: 0.0,
                        depth: 8,
                        reverse: false,
                        capture_scale,
                    },
                )
                .unwrap_err();
            assert_eq!(ambilight.error().as_deref(), Some(error.as_str()));
            assert!(!ambilight.is_running());
        }
    }
    #[test]
    fn ppm_preserves_whitespace_pixel_bytes() {
        let (w, h, pixels) = parse_ppm(b"P6\n1 1\n255\n\n\r ").unwrap();
        assert_eq!((w, h), (1, 1));
        assert_eq!(pixels, vec![10, 13, 32]);
    }
    #[test]
    fn ppm_rejects_truncated_and_unsupported_input() {
        assert!(parse_ppm(b"P6\n1 1\n255\n\x00").is_err());
        assert!(parse_ppm(b"P6\n1 1\n65535\n\x00\x00\x00").is_err());
        assert!(parse_ppm(b"P6\n999999999999999999999999999999 1\n255\n").is_err());
    }
    #[test]
    fn border_follows_left_top_right_order() {
        let mut pixels = vec![0; 8 * 8 * 3];
        for y in 0..8 {
            for x in 0..8 {
                let i = (y * 8 + x) * 3;
                pixels[i] = if x == 0 { 255 } else { 0 };
                pixels[i + 1] = if y == 0 { 255 } else { 0 };
                pixels[i + 2] = if x == 7 { 255 } else { 0 };
            }
        }
        let colors = sample_border_colors(&pixels, 8, 8, &[1, 1, 1], 1);
        assert_eq!(colors.len(), 3);
        assert_eq!(colors[0].r, 255);
        assert_eq!(colors[1].g, 255);
        assert_eq!(colors[2].b, 255);
    }
    #[test]
    fn uneven_top_segments_include_the_last_pixel_column() {
        let mut pixels = vec![0; 5 * 8 * 3];
        pixels[4 * 3] = 250;
        let colors = sample_border_colors(&pixels, 5, 8, &[0, 2, 0], 1);
        assert_eq!(colors[0].r, 0);
        assert_eq!(
            colors[1].r, 83,
            "the final segment must cover columns 2, 3 and 4"
        );
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
