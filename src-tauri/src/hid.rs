use hidapi::{HidApi, HidDevice};
use std::sync::Mutex;
use std::time::Duration;
use std::thread;

const REPORT_SIZE: usize = 64;

// Supported light bar devices (same RB protocol)
const SUPPORTED_DEVICES: &[(u16, u16, &str)] = &[
    (0x1a86, 0xfe07, "SyncLight Bar"),
    (0x1a86, 0xfe0c, "SyncLight Bar (CDC)"),
];

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LedColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Default for LedColor {
    fn default() -> Self {
        LedColor { r: 0, g: 0, b: 0 }
    }
}

pub struct HidController {
    device: Mutex<Option<HidDevice>>,
    id_counter: Mutex<u8>,
    pub total_leds: Mutex<u16>,
    pub last_color_payload: Mutex<Option<Vec<u8>>>,
    pub current_brightness: Mutex<u8>,
    pub last_nonzero_brightness: Mutex<u8>,
    pub per_led_state: Mutex<Vec<LedColor>>,
    pub sections: Mutex<[u16; 3]>,
    pub global_color: Mutex<LedColor>,
}

impl HidController {
    pub fn new() -> Self {
        HidController {
            device: Mutex::new(None),
            id_counter: Mutex::new(0),
            total_leds: Mutex::new(71),
            last_color_payload: Mutex::new(None),
            current_brightness: Mutex::new(0x77),
            last_nonzero_brightness: Mutex::new(0x77),
            per_led_state: Mutex::new(Vec::new()),
            sections: Mutex::new([15, 41, 15]),
            global_color: Mutex::new(LedColor { r: 255, g: 0, b: 0 }),
        }
    }

    pub fn set_total_leds(&self, n: u16) {
        let clamped = n.max(1).min(254);
        *self.total_leds.lock().unwrap() = clamped;
    }

    pub fn get_total_leds(&self) -> u16 {
        *self.total_leds.lock().unwrap()
    }

    pub fn get_global_color(&self) -> LedColor {
        self.global_color.lock().unwrap().clone()
    }

    pub fn set_global_color(&self, color: LedColor) {
        *self.global_color.lock().unwrap() = color;
    }

    #[allow(dead_code)]
    pub fn get_brightness(&self) -> u8 {
        *self.current_brightness.lock().unwrap()
    }

    fn next_id(&self) -> u8 {
        let mut id = self.id_counter.lock().unwrap();
        *id = id.wrapping_add(1);
        if *id == 0 {
            *id = 1;
        }
        *id
    }

    fn checksum(buf: &[u8]) -> u8 {
        let mut s: u8 = 0;
        for &b in buf {
            s = s.wrapping_add(b);
        }
        s
    }

    #[allow(dead_code)]
    fn sc_crc(data: &[u8], len: usize) -> u16 {
        let table: [u16; 2] = [0, 21315];
        let mut r: u16 = 0;
        for i in 0..len {
            let mut o = data[len - 1 - i] as u16;
            for _ in 0..8 {
                r = (r >> 1) ^ table[((r ^ o) & 1) as usize];
                o >>= 1;
            }
        }
        r & 0xffff
    }

    fn to_report(buf: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; REPORT_SIZE + 1];
        out[0] = 0x00;
        let copy_len = buf.len().min(REPORT_SIZE);
        out[1..1 + copy_len].copy_from_slice(&buf[..copy_len]);
        out
    }

    fn is_supported(vid: u16, pid: u16) -> bool {
        SUPPORTED_DEVICES.iter().any(|&(v, p, _)| v == vid && p == pid)
    }

    pub fn find_device() -> bool {
        match HidApi::new() {
            Ok(api) => api
                .device_list()
                .any(|d| Self::is_supported(d.vendor_id(), d.product_id())),
            Err(_) => false,
        }
    }

    pub fn open_device(&self) -> Result<(), String> {
        let mut dev = self.device.lock().unwrap();
        if dev.is_some() {
            return Ok(());
        }
        let api = HidApi::new().map_err(|e| format!("HidApi init failed: {}", e))?;
        let info = api
            .device_list()
            .find(|d| Self::is_supported(d.vendor_id(), d.product_id()))
            .ok_or_else(|| "SyncLight device not found".to_string())?;
        let d = info
            .open_device(&api)
            .map_err(|e| format!("Failed to open device: {}", e))?;
        d.set_blocking_mode(false)
            .map_err(|e| format!("Failed to set non-blocking: {}", e))?;
        *dev = Some(d);
        log::info!("HID device opened");
        Ok(())
    }

    pub fn close_device(&self) {
        let mut dev = self.device.lock().unwrap();
        if dev.is_some() {
            drop(dev.take());
            log::info!("HID device closed");
        }
    }

    pub fn is_open(&self) -> bool {
        self.device.lock().unwrap().is_some()
    }

    fn write_report(&self, buf: &[u8]) -> Result<(), String> {
        let report = Self::to_report(buf);
        let dev = self.device.lock().unwrap();
        let d = dev.as_ref().ok_or("Device not open")?;
        d.write(&report)
            .map_err(|e| format!("HID write failed: {}", e))?;
        Ok(())
    }

    fn write_with_retry(&self, buf: &[u8], max_attempts: u32) -> Result<(), String> {
        let mut delay = Duration::from_millis(50);
        for attempt in 1..=max_attempts {
            match self.write_report(buf) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    log::warn!("write attempt {} failed: {}", attempt, e);
                    self.close_device();
                    if attempt < max_attempts {
                        thread::sleep(delay);
                        delay = delay.min(Duration::from_secs(1)) * 2;
                        let _ = self.open_device();
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        Err("all write attempts failed".into())
    }

    pub fn send_rb(&self, action: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
        let payload_len = payload.len();
        let total_len = 6 + payload_len;
        let mut buf = vec![0u8; total_len];
        buf[0] = b'R';
        buf[1] = b'B';
        buf[2] = (total_len & 0xff) as u8;
        buf[3] = self.next_id();
        buf[4] = action;
        if payload_len > 0 {
            buf[5..5 + payload_len].copy_from_slice(payload);
        }
        buf[total_len - 1] = Self::checksum(&buf[..total_len - 1]);
        self.write_with_retry(&buf, 4)?;
        Ok(buf)
    }

    #[allow(dead_code)]
    pub fn send_sc(&self, action: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
        let payload_len = payload.len();
        let total_len = 7 + payload_len;
        let mut buf = vec![0u8; total_len];
        buf[0] = b'S';
        buf[1] = b'C';
        buf[2] = total_len as u8;
        buf[3] = self.next_id();
        buf[4] = action;
        if payload_len > 0 {
            buf[5..5 + payload_len].copy_from_slice(payload);
        }
        let crc = Self::sc_crc(&buf, total_len - 2);
        buf[total_len - 2] = (crc >> 8) as u8;
        buf[total_len - 1] = (crc & 0xff) as u8;
        self.write_with_retry(&buf, 4)?;
        Ok(buf)
    }

    pub fn build_section_payload(&self, section: u8, r: u8, g: u8, b: u8) -> Vec<u8> {
        vec![section, r, g, b, 0x47, 0x48, 0x00, 0x00, 0x00, 0xfe]
    }

    pub fn build_segment_data(&self, colors: &[LedColor]) -> Vec<u8> {
        let total = self.get_total_leds() as usize;
        let num_leds = colors.len().min(total);
        let mut segments: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < num_leds {
            let c = &colors[i];
            let cr = c.r;
            let cg = c.g;
            let cb = c.b;
            let mut end = i;
            while end + 1 < num_leds {
                let nc = &colors[end + 1];
                if nc.r == cr && nc.g == cg && nc.b == cb {
                    end += 1;
                } else {
                    break;
                }
            }
            // 1-based indices
            segments.push((i + 1) as u8);
            segments.push(cr);
            segments.push(cg);
            segments.push(cb);
            segments.push((end + 1) as u8);
            i = end + 1;
        }
        // turn off remaining LEDs
        if num_leds < total {
            segments.push((num_leds + 1) as u8);
            segments.push(0);
            segments.push(0);
            segments.push(0);
            segments.push(total as u8);
        }
        segments
    }

    pub fn send_per_led_colors(&self, colors: &[LedColor]) -> Result<(), String> {
        let seg_data = self.build_segment_data(colors);
        let chunk_size = 55;
        let mut offset = 0;
        while offset < seg_data.len() {
            let end = (offset + chunk_size).min(seg_data.len());
            let chunk = &seg_data[offset..end];
            self.send_rb(0x86, chunk)?;
            offset = end;
            if offset < seg_data.len() {
                thread::sleep(Duration::from_millis(20));
            }
        }
        Ok(())
    }

    pub fn set_color(&self, section: u8, r: u8, g: u8, b: u8) -> Result<(), String> {
        // probe first
        let _ = self.send_rb(0x97, &[]);
        thread::sleep(Duration::from_millis(20));
        let payload = self.build_section_payload(section, r, g, b);
        *self.last_color_payload.lock().unwrap() = Some(payload.clone());
        self.send_rb(0x86, &payload)?;
        Ok(())
    }

    pub fn set_brightness(&self, value: u8) -> Result<(), String> {
        *self.current_brightness.lock().unwrap() = value;
        self.send_rb(0x87, &[value])?;
        if value > 0 {
            *self.last_nonzero_brightness.lock().unwrap() = value;
        }
        if let Some(ref payload) = *self.last_color_payload.lock().unwrap() {
            let _ = self.send_rb(0x86, payload);
        }
        Ok(())
    }

    pub fn set_brightness_only(&self, value: u8) -> Result<(), String> {
        *self.current_brightness.lock().unwrap() = value;
        self.send_rb(0x87, &[value])?;
        if value > 0 {
            *self.last_nonzero_brightness.lock().unwrap() = value;
        }
        Ok(())
    }

    pub fn set_color_and_brightness(
        &self,
        section: u8,
        r: u8,
        g: u8,
        b: u8,
        brightness: u8,
    ) -> Result<(), String> {
        *self.current_brightness.lock().unwrap() = brightness;
        let _ = self.send_rb(0x97, &[]);
        thread::sleep(Duration::from_millis(20));
        self.send_rb(0x87, &[brightness])?;
        thread::sleep(Duration::from_millis(30));
        let payload = self.build_section_payload(section, r, g, b);
        *self.last_color_payload.lock().unwrap() = Some(payload.clone());
        self.send_rb(0x86, &payload)?;
        Ok(())
    }

    pub fn set_single_led(&self, index: usize, r: u8, g: u8, b: u8) -> Result<(), String> {
        let total = self.get_total_leds() as usize;
        if index >= total {
            return Err(format!("invalid led index (0-{})", total - 1));
        }
        {
            let mut state = self.per_led_state.lock().unwrap();
            if state.len() < total {
                state.resize(total, LedColor::default());
            }
            state[index] = LedColor { r, g, b };
        }
        let state = self.per_led_state.lock().unwrap().clone();
        self.send_per_led_colors(&state)
    }

    pub fn fade_to_off(
        &self,
        start_brightness: Option<u8>,
        color_payload: Option<Vec<u8>>,
        duration_ms: u64,
        steps: u32,
    ) -> Result<(), String> {
        let start_b = start_brightness.unwrap_or(*self.current_brightness.lock().unwrap());
        let payload = color_payload.or_else(|| self.last_color_payload.lock().unwrap().clone());
        let step_delay = Duration::from_millis((duration_ms / steps as u64).max(10));
        for i in (1..=steps).rev() {
            let bv = ((start_b as u32 * i) / steps) as u8;
            let _ = self.send_rb(0x87, &[bv]);
            thread::sleep(Duration::from_millis(20));
            if let Some(ref p) = payload {
                let _ = self.send_rb(0x86, p);
            }
            let remaining = step_delay.saturating_sub(Duration::from_millis(20));
            if !remaining.is_zero() {
                thread::sleep(remaining);
            }
        }
        *self.current_brightness.lock().unwrap() = 0;
        let off_payload: Vec<u8> = vec![0x01, 0, 0, 0, 0x47, 0x48, 0, 0, 0, 0xfe];
        let _ = self.send_rb(0x86, &off_payload);
        Ok(())
    }

    pub fn power_on(
        &self,
        section: u8,
        r: u8,
        g: u8,
        b: u8,
        brightness: Option<u8>,
    ) -> Result<(), String> {
        let bri = brightness.unwrap_or(*self.last_nonzero_brightness.lock().unwrap());
        self.set_color_and_brightness(section, r, g, b, bri)
    }

    #[allow(dead_code)]
    pub fn reset_bar(&self) -> Result<(), String> {
        self.close_device();
        for attempt in 0..6 {
            thread::sleep(Duration::from_millis(500));
            if !Self::find_device() {
                continue;
            }
            if let Err(e) = self.open_device() {
                log::warn!("resetBar attempt {} open failed: {}", attempt, e);
                continue;
            }
            thread::sleep(Duration::from_millis(200));
            let bri = {
                let nb = *self.last_nonzero_brightness.lock().unwrap();
                let cb = *self.current_brightness.lock().unwrap();
                if nb > 0 { nb } else if cb > 0 { cb } else { 0xff }
            };
            let _ = self.send_rb(0x87, &[bri]);
            thread::sleep(Duration::from_millis(80));
            if let Some(ref payload) = *self.last_color_payload.lock().unwrap() {
                let section = payload[0];
                let r = payload[1];
                let g = payload[2];
                let b = payload[3];
                let _ = self.set_color_and_brightness(section, r, g, b, if bri > 0 { bri } else { 0x77 });
            }
            return Ok(());
        }
        Err("resetBar: all attempts failed".into())
    }
}
