// g5kbd-gui Rust core.
//
// Every hardware change is delegated to the `g5kbd` CLI, which is the single
// source of truth for the protocol, state persistence and effect daemons:
//
//   GUI (webview)  --invoke-->  this crate  --spawn-->  g5kbd CLI
//                                                    (kernel LED node or EC)
//
// The GUI therefore never touches the hardware itself. Children are spawned
// with G5KBD_NO_SUDO=1 — the kernel node is group-writable via the udev rule,
// and per-user effect bookkeeping lives under $XDG_RUNTIME_DIR, so no root is
// needed when the kernel module is loaded.

use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

const G5KBD: &str = "g5kbd";

fn default_state_file() -> PathBuf {
    PathBuf::from(
        std::env::var_os("G5KBD_STATE").unwrap_or_else(|| "/var/lib/g5kbd/state.json".into()),
    )
}

fn default_pid_file() -> PathBuf {
    PathBuf::from(std::env::var_os("G5KBD_PID").unwrap_or_else(|| "/run/g5kbd-effect.pid".into()))
}

/// Per-user pid file under $XDG_RUNTIME_DIR so a non-root GUI can register
/// and stop effects (the root service keeps the /run default).
fn runtime_pid_file() -> PathBuf {
    if let Ok(rd) = std::env::var("XDG_RUNTIME_DIR") {
        let dir = PathBuf::from(&rd);
        if dir.exists() {
            return dir.join("g5kbd-effect.pid");
        }
    }
    default_pid_file()
}

/// Run `g5kbd` with the args a normal user needs; returns stdout on success
/// or the last stderr line on failure.
fn run_cli(args: &[&str]) -> Result<String, String> {
    let out = Command::new(G5KBD)
        .args(args)
        .env("G5KBD_NO_SUDO", "1")
        .env("G5KBD_PID", runtime_pid_file())
        .output()
        .map_err(|e| format!("cannot run `g5kbd`: {e} (is it installed? run sudo ./install.sh)"))?;

    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail = err
            .trim()
            .lines()
            .last()
            .unwrap_or("unknown error")
            .to_string();
        Err(format!("g5kbd {}: {tail}", args.join(" ")))
    }
}

/// Run a CLI call off the async executor thread.
async fn run_cli_async(args: Vec<String>) -> Result<String, String> {
    let handle = tauri::async_runtime::spawn_blocking(move || {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        run_cli(&refs)
    });
    handle.await.map_err(|e| format!("task join error: {e}"))?
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
struct KbState {
    enabled: bool,
    red: u8,
    green: u8,
    blue: u8,
    brightness: u8,
    effect_running: bool,
    backend: String,
}

#[derive(serde::Deserialize)]
struct SavedState {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    rgb: Option<Vec<u16>>,
    #[serde(default)]
    brightness: Option<u16>,
}

fn effect_is_running() -> bool {
    if let Ok(text) = std::fs::read_to_string(runtime_pid_file()) {
        if let Ok(pid) = text.trim().parse::<i32>() {
            // kill(pid, 0) returns 0 if the process is alive, -1 otherwise
            return unsafe { libc::kill(pid, 0) } == 0;
        }
    }
    false
}

fn backend_name() -> String {
    if std::path::Path::new("/sys/class/leds/rgb:kbd").exists() {
        "led".into()
    } else {
        "ec".into()
    }
}

fn current_state() -> KbState {
    let mut st = KbState {
        enabled: true,
        red: 0,
        green: 0,
        blue: 200,
        brightness: 100,
        effect_running: false,
        backend: backend_name(),
    };

    if let Ok(text) = std::fs::read_to_string(default_state_file()) {
        if let Ok(saved) = serde_json::from_str::<SavedState>(&text) {
            st.enabled = saved.enabled.unwrap_or(true);
            if let Some(rgb) = saved.rgb {
                if rgb.len() >= 3 {
                    st.red = (rgb[0] & 0xff) as u8;
                    st.green = (rgb[1] & 0xff) as u8;
                    st.blue = (rgb[2] & 0xff) as u8;
                }
            }
            st.brightness = (saved.brightness.unwrap_or(100).min(100)) as u8;
        }
    }
    st.effect_running = effect_is_running();
    st
}

// ---------------------------------------------------------------------------
// Commands (frontend calls these via invoke)
// ---------------------------------------------------------------------------

#[tauri::command]
fn backend() -> String {
    backend_name()
}

#[tauri::command]
async fn get_state() -> Result<KbState, String> {
    // quick file reads; fine to do inline on the async pool
    Ok(current_state())
}

#[tauri::command]
async fn set_color(red: u8, green: u8, blue: u8) -> Result<String, String> {
    let hex = format!("{red:02x}{green:02x}{blue:02x}");
    run_cli_async(vec!["color".into(), hex]).await
}

#[tauri::command]
async fn set_brightness(pct: u8) -> Result<String, String> {
    let pct = pct.min(100);
    run_cli_async(vec!["brightness".into(), pct.to_string()]).await
}

/// Live brightness write used while the slider is being dragged: writes the
/// LED sysfs node directly (a single file write, world-writable via the udev
/// rule) instead of spawning the CLI per tick. State is only persisted by the
/// final `set_brightness` call on release.
#[tauri::command]
async fn brightness_raw(level: u8) -> Result<(), String> {
    let path = std::path::PathBuf::from("/sys/class/leds/rgb:kbd/brightness");
    let handle = tauri::async_runtime::spawn_blocking(move || {
        std::fs::write(&path, level.to_string())
            .map_err(|e| format!("cannot write brightness: {e}"))
    });
    handle.await.map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn set_power(on: bool) -> Result<String, String> {
    run_cli_async(vec![if on { "on".into() } else { "off".into() }]).await
}

#[tauri::command]
async fn effect_start(mode: String, speed: u8) -> Result<String, String> {
    let speed = speed.clamp(1, 10);
    run_cli_async(vec![
        "effect".into(),
        mode,
        "--speed".into(),
        speed.to_string(),
        "--bg".into(),
    ])
    .await
}

/// Tab-separated profile list from the CLI: name \t RRGGBB \t brightness \t on|off
#[tauri::command]
async fn profiles_list() -> Result<Vec<String>, String> {
    let out = run_cli_async(vec!["profile".into(), "list".into()]).await?;
    Ok(out.lines().map(str::to_string).collect())
}

#[tauri::command]
async fn profile_save(name: String) -> Result<String, String> {
    run_cli_async(vec!["profile".into(), "save".into(), name]).await
}

#[tauri::command]
async fn profile_apply(name: String) -> Result<String, String> {
    run_cli_async(vec!["profile".into(), "apply".into(), name]).await
}

#[tauri::command]
async fn profile_delete(name: String) -> Result<String, String> {
    run_cli_async(vec!["profile".into(), "delete".into(), name]).await
}

#[tauri::command]
async fn effect_stop() -> Result<String, String> {
    run_cli_async(vec!["effect".into(), "stop".into()]).await
}

// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            backend,
            get_state,
            set_color,
            set_brightness,
            brightness_raw,
            set_power,
            effect_start,
            effect_stop,
            profiles_list,
            profile_save,
            profile_apply,
            profile_delete,
        ])
        .run(tauri::generate_context!())
        .expect("error while running g5kbd-gui");
}
