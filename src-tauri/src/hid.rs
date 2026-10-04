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
    control_lock: Mutex<Option<std::fs::File>>,
    screen_sc_supported: Mutex<bool>,
    pub firmware_info: Mutex<Option<serde_json::Value>>,
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
            control_lock: Mutex::new(None),
            screen_sc_supported: Mutex::new(false),
            firmware_info: Mutex::new(None),
            id_counter: Mutex::new(1),
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
        if *id == 0 || *id >= 255 {
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

    fn to_report(buf: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(buf.len() + 1);
        out.push(0); // HID report ID, exactly as the stock USB adapter.
        out.extend_from_slice(buf);
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

    fn claim_control(path: &std::path::Path) -> Result<std::fs::File, String> {
        let owner = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .open(path).map_err(|e|format!("Cannot open controller ownership lock: {e}"))?;
        owner.try_lock().map_err(|_| "Another controller owns the backlight. Quit its GUI before running a hardware test.".to_string())?;
        Ok(owner)
    }

    pub fn open_device(&self) -> Result<(), String> {
        let mut dev = self.device.lock().unwrap();
        if dev.is_some() {
            return Ok(());
        }
        let owner = Self::claim_control(&crate::state::dirs_config().join("usb-owner.lock"))?;
        let api = HidApi::new().map_err(|e| format!("HidApi init failed: {}", e))?;
        let info = api
            .device_list()
            .find(|d| Self::is_supported(d.vendor_id(), d.product_id()) && d.usage_page() >= 0xff00)
            .ok_or_else(|| "Supported SyncLight vendor HID interface not found. Reconnect your USB backlight.".to_string())?;
        let d = info
            .open_device(&api)
            .map_err(|e| format!("Failed to open device: {}", e))?;
        d.set_blocking_mode(false)
            .map_err(|e| format!("Failed to set non-blocking: {}", e))?;
        *dev = Some(d);
        *self.control_lock.lock().unwrap() = Some(owner);
        log::info!("HID device opened");
        Ok(())
    }

    pub fn close_device(&self) {
        let mut dev = self.device.lock().unwrap();
        if dev.is_some() {
            drop(dev.take());
            self.control_lock.lock().unwrap().take();
            *self.screen_sc_supported.lock().unwrap() = false;
            *self.firmware_info.lock().unwrap() = None;
            log::info!("HID device closed");
        }
    }

    pub fn is_open(&self) -> bool {
        self.device.lock().unwrap().is_some()
    }

    fn write_report(&self, buf: &[u8]) -> Result<(), String> {
        let dev = self.device.lock().unwrap();
        let d = dev.as_ref().ok_or("Device not open")?;
        // Keep the device lock across all chunks: frames must never interleave.
        for chunk in buf.chunks(REPORT_SIZE) {
            let report = Self::to_report(chunk);
            let written = d.write(&report).map_err(|e| format!("HID write failed: {}", e))?;
            if written != report.len() { return Err(format!("Incomplete HID write: {} of {} bytes.", written, report.len())); }
        }
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
        if total_len > 255 { return Err("RB packet exceeds its 8-bit length field".into()); }
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

    fn screen_packet(id: u8, action: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
        let total_len = 7 + payload.len();
        if total_len > u16::MAX as usize { return Err("SC packet too large".into()); }
        let mut buf = vec![b'S', b'C'];
        buf.extend_from_slice(&(total_len as u16).to_be_bytes());
        buf.extend_from_slice(&[id, action]);
        buf.extend_from_slice(payload);
        buf.push(Self::checksum(&buf));
        Ok(buf)
    }

    pub fn send_screen_colors(&self, colors: &[LedColor]) -> Result<(), String> {
        // Stock software uses RB segments for old firmware. Unknown firmware
        // takes that compatible route until a validated version enables SC.
        if !*self.screen_sc_supported.lock().unwrap() { return self.send_per_led_colors(colors); }
        let payload = self.build_segment_data(colors);
        let packet = Self::screen_packet(self.next_id(), 0x80, &payload)?;
        self.write_with_retry(&packet, 4)
    }

    pub fn build_section_payload(&self, section: u8, r: u8, g: u8, b: u8) -> Vec<u8> {
        let total = self.get_total_leds() as u8;
        let mut payload = vec![section, r, g, b, total];
        if total < 254 { payload.extend_from_slice(&[total + 1, 0, 0, 0, 254]); }
        payload
    }

    pub fn read_device_info(&self) -> Result<serde_json::Value, String> {
        let request = self.send_rb(0x82, &[])?;
        let dev = self.device.lock().unwrap();
        let d = dev.as_ref().ok_or("Device not open")?;
        let deadline = std::time::Instant::now() + Duration::from_millis(600);
        let mut response = [0u8; 256];
        let mut last_response = Vec::new();
        while std::time::Instant::now() < deadline {
            let n = d.read_timeout(&mut response, 100).map_err(|e| format!("Device info read failed: {e}"))?;
            if n > 0 { last_response = response[..n].to_vec(); }
            if n < 25 || &response[..2] != b"RB" || response[3] != request[3] { continue; }
            let len = response[2] as usize;
            if len < 25 || len > n { continue; }
            if let Some(info) = Self::parse_device_info(&response[..len], request[3]) {
                *self.screen_sc_supported.lock().unwrap() = [response[21], response[22], response[23]] > [1, 0, 2];
                *self.firmware_info.lock().unwrap() = Some(info.clone());
                return Ok(info);
            }
        }
        Err(format!("No valid firmware reply within 600ms; received bytes: {:02x?}", last_response))
    }

    fn parse_device_info(response: &[u8], id: u8) -> Option<serde_json::Value> {
        // Firmware 1.9.4 sends a 25-byte info reply with a zero trailing byte,
        // not the checksum used by requests. Stock parses fields directly.
        if response.len() < 25 || &response[..2] != b"RB" || response[3] != id || response[4] != 0x82 { return None; }
        if response[2] as usize > response.len() || response[2] < 25 || !(1..=254).contains(&response[11]) { return None; }
        Some(serde_json::json!({
            "modelId": format!("{:02x}{:02x}{:02x}", response[5], response[6], response[7]),
            "ledCount": response[11],
            "firmware": format!("{}.{}.{}", response[21], response[22], response[23])
        }))
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

    pub fn refresh_static_brightness(&self, color: &LedColor, value: u8) -> Result<(), String> {
        // Stock static-mode brightness path: 0x87, 20ms, then 0x86.
        // Do not replace the user's brightness ceiling with an audio sample.
        self.send_rb(0x87, &[value])?;
        thread::sleep(Duration::from_millis(20));
        let payload = self.build_section_payload(1, color.r, color.g, color.b);
        self.send_rb(0x86, &payload)?;
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
            self.send_rb(0x87, &[bv])?;
            thread::sleep(Duration::from_millis(20));
            if let Some(ref p) = payload {
                self.send_rb(0x86, p)?;
            }
            let remaining = step_delay.saturating_sub(Duration::from_millis(20));
            if !remaining.is_zero() {
                thread::sleep(remaining);
            }
        }
        *self.current_brightness.lock().unwrap() = 0;
        let off_payload = self.build_section_payload(1, 0, 0, 0);
        self.send_rb(0x86, &off_payload)?;
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

#[cfg(test)]
mod protocol_tests {
    use super::*;

    #[test]
    fn stock_screen_frame_uses_big_endian_length_and_sum() {
        // Stock module 60895 setSyncScreen, ID=2, one cyan segment.
        let packet = HidController::screen_packet(2, 0x80, &[1, 40, 150, 200, 71]).unwrap();
        assert_eq!(packet, vec![83, 67, 0, 12, 2, 128, 1, 40, 150, 200, 71, 242]);
        let long = HidController::screen_packet(2, 0x80, &vec![0; 355]).unwrap();
        assert_eq!(&long[2..4], &[1, 106]);
        let reports: Vec<_> = long.chunks(64).map(HidController::to_report).collect();
        assert_eq!(reports.len(), 6);
        assert_eq!(reports.last().unwrap().len(), 43);
        assert!(reports.iter().all(|r| r[0] == 0));
        let recovered: Vec<_> = reports.iter().flat_map(|r| r[1..].iter().copied()).collect();
        assert_eq!(recovered, long);
    }

    #[test]
    fn static_range_tracks_configured_layout() {
        let hid = HidController::new();
        hid.set_total_leds(100);
        assert_eq!(hid.build_section_payload(1, 10, 20, 30), vec![1,10,20,30,100,101,0,0,0,254]);
        hid.set_total_leds(254);
        assert_eq!(hid.build_section_payload(1, 10, 20, 30), vec![1,10,20,30,254]);
    }

    #[test]
    fn ids_match_stock_range_and_reports_are_not_padded() {
        let hid = HidController::new();
        assert_eq!(hid.next_id(), 2);
        for _ in 0..1000 { assert!((1..=254).contains(&hid.next_id())); }
        assert_eq!(HidController::to_report(&[82,66,6,2,130,30]), vec![0,82,66,6,2,130,30]);
    }

    #[test]
    fn no_background_turn_off_commands() {
        assert!(!include_str!("main.rs").contains("send_rb(0x97"));
        assert!(!include_str!("effects.rs").contains("send_rb(0x97"));
    }
}

#[cfg(test)]
mod firmware_reply_tests {
    #[test]
    fn real_firmware_info_reply_has_no_request_checksum() {
        let bytes = [0x52,0x42,0x19,0x02,0x82,0x00,0x05,0x01,0x18,0x01,0x00,0x36,0xcd,0xab,0x83,0x45,0x9e,0xbd,0xee,0xae,0xe5,0x01,0x09,0x04,0x00];
        let info = super::HidController::parse_device_info(&bytes, 2).unwrap();
        assert_eq!(info["ledCount"], 54);
        assert_eq!(info["firmware"], "1.9.4");
        assert!(super::HidController::parse_device_info(&bytes, 3).is_none());
        assert!(super::HidController::parse_device_info(&bytes[..20], 2).is_none());
    }
}

#[cfg(test)]
mod ownership_tests {
    #[test]
    fn second_writer_is_rejected_until_first_releases_device() {
        let path=std::env::temp_dir().join(format!("synclight-owner-test-{}",std::process::id()));
        let first=super::HidController::claim_control(&path).unwrap();
        assert!(super::HidController::claim_control(&path).is_err());
        drop(first);
        let second=super::HidController::claim_control(&path).unwrap();
        drop(second);
        std::fs::remove_file(path).unwrap();
    }
}
