// Keyboard state, read from the same files the CLI itself uses:
// the saved state under /var/lib/g5kbd (written by `g5kbd color` etc.) and
// the per-user effect pid file. Nothing here talks to the EC directly.

use serde::{Deserialize, Serialize};

use super::{default_state_file, runtime_pid_file};

#[derive(Clone, Serialize)]
pub struct KbState {
    pub enabled: bool,
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub brightness: u8,
    pub effect_running: bool,
    pub backend: String,
}

#[derive(Deserialize)]
struct SavedState {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    rgb: Option<Vec<u16>>,
    #[serde(default)]
    brightness: Option<u16>,
}

pub fn effect_is_running() -> bool {
    if let Ok(text) = std::fs::read_to_string(runtime_pid_file())
        && let Ok(pid) = text.trim().parse::<i32>()
    {
        // kill(pid, 0) returns 0 if the process is alive, -1 otherwise
        return unsafe { libc::kill(pid, 0) } == 0;
    }
    false
}

pub fn backend_name() -> String {
    if std::path::Path::new("/sys/class/leds/rgb:kbd").exists() {
        "led".into()
    } else {
        "ec".into()
    }
}

/// Read what the keyboard is doing right now. Blocking file reads, cheap —
/// but still call it from the background executor.
pub fn current_state() -> KbState {
    let mut st = KbState {
        enabled: true,
        red: 0,
        green: 0,
        blue: 200,
        brightness: 100,
        effect_running: false,
        backend: backend_name(),
    };

    if let Ok(text) = std::fs::read_to_string(default_state_file())
        && let Ok(saved) = serde_json::from_str::<SavedState>(&text)
    {
        st.enabled = saved.enabled.unwrap_or(true);
        if let Some(rgb) = saved.rgb
            && rgb.len() >= 3
        {
            st.red = (rgb[0] & 0xff) as u8;
            st.green = (rgb[1] & 0xff) as u8;
            st.blue = (rgb[2] & 0xff) as u8;
        }
        st.brightness = (saved.brightness.unwrap_or(100).min(100)) as u8;
    }
    st.effect_running = effect_is_running();
    st
}
