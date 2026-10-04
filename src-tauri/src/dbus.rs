use crate::hid::HidController;
use std::sync::Arc;
use zbus::{interface, Connection};
use tauri::{Emitter, Manager};

pub struct OpenLightsSyncDBus {
    hid: Arc<HidController>,
    app: tauri::AppHandle,
}

#[interface(name = "com.snzhy.opensycnlights")]
impl OpenLightsSyncDBus {
    async fn toggle_power(&self) -> bool {
        self.control("toggle".into()).await
    }

    async fn set_brightness(&self, value: u8) {
        let _=self.app.emit("controller-action",format!("brightness:{value}"));
    }

    async fn get_status(&self) -> String {
        let open = self.hid.is_open();
        let found = HidController::find_device();
        let brightness = *self.hid.current_brightness.lock().unwrap();
        serde_json::json!({
            "open": open,
            "found": found,
            "brightness": brightness,
            "controller": self.app.state::<crate::AppShellStatus>().0.lock().unwrap().clone(),
            "audioRunning": self.app.state::<crate::AppAudio>().0.is_running(),
            "screenRunning": self.app.state::<crate::AppAmbilight>().0.is_running(),
            "effectsRunning": self.app.state::<crate::AppEffects>().0.is_running(),
        })
        .to_string()
    }

    async fn show_window(&self) {
        if let Some(w)=self.app.get_webview_window("main") {
            let _=w.show();let _=w.unminimize();let _=w.set_focus();
        }
    }

    async fn control(&self, command: String) -> bool {
        if !valid_command(&command) { return false; }
        if self.app.state::<crate::AppShellStatus>().0.lock().unwrap()["ready"]!=true { return false; }
        self.app.emit("controller-action",command).is_ok()
    }
}

pub async fn start_dbus_server(hid: Arc<HidController>, app: tauri::AppHandle) -> Result<(), String> {
    let service = OpenLightsSyncDBus { hid, app };
    let conn = Connection::session()
        .await
        .map_err(|e| format!("D-Bus connection failed: {}", e))?;
    conn.object_server()
        .at("/com/snzhy/opensycnlights", service)
        .await
        .map_err(|e| format!("D-Bus object failed: {}", e))?;
    conn.request_name("com.snzhy.opensycnlights")
        .await
        .map_err(|e| format!("D-Bus name failed: {}", e))?;

    log::info!("D-Bus server running on com.snzhy.opensycnlights");

    // Keep connection alive
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }
}

fn valid_command(command: &str) -> bool {
    if ["lighting","audio","screen","stop","off","on","toggle","brightness-up","brightness-down","resume-toggle"].contains(&command) { return true; }
    if let Some(value)=command.strip_prefix("brightness:") { return value.parse::<u8>().is_ok(); }
    for prefix in ["preset:","display:"] {
        if let Some(value)=command.strip_prefix(prefix) {
            return !value.is_empty() && value.len()<=64 && value.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'-'||c==b'_');
        }
    }
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn shell_commands_are_bounded_and_allowlisted() {
        for c in ["audio","screen","stop","display:DP-1","preset:screen-gaming","brightness:255"] { assert!(super::valid_command(c)); }
        for c in ["exec:rm", "display:../../", "brightness:256", "preset:","display:a;foo"] { assert!(!super::valid_command(c)); }
    }
}
