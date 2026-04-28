use crate::hid::HidController;
use std::sync::Arc;
use zbus::{interface, Connection};

pub struct OpenLightsSyncDBus {
    hid: Arc<HidController>,
}

#[interface(name = "com.synclights.gui")]
impl OpenLightsSyncDBus {
    async fn toggle_power(&self) -> bool {
        if self.hid.is_open() {
            let payload = self.hid.build_section_payload(1, 0, 0, 0);
            let _ = self.hid.fade_to_off(None, Some(payload), 1000, 10);
            false
        } else {
            let _ = self.hid.open_device();
            let _ = self.hid.power_on(1, 255, 255, 255, None);
            true
        }
    }

    async fn set_brightness(&self, value: u8) {
        let _ = self.hid.set_brightness(value);
    }

    async fn get_status(&self) -> String {
        let open = self.hid.is_open();
        let found = HidController::find_device();
        let brightness = *self.hid.current_brightness.lock().unwrap();
        serde_json::json!({
            "open": open,
            "found": found,
            "brightness": brightness,
        })
        .to_string()
    }

    async fn show_window(&self) {
        // Handled by the Tauri window manager via event
    }
}

pub async fn start_dbus_server(hid: Arc<HidController>) -> Result<(), String> {
    let service = OpenLightsSyncDBus { hid };
    let conn = Connection::session()
        .await
        .map_err(|e| format!("D-Bus connection failed: {}", e))?;
    conn.object_server()
        .at("/com/synclights/gui", service)
        .await
        .map_err(|e| format!("D-Bus object failed: {}", e))?;
    conn.request_name("com.synclights.gui")
        .await
        .map_err(|e| format!("D-Bus name failed: {}", e))?;

    log::info!("D-Bus server running on com.synclights.gui");

    // Keep connection alive
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }
}
