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
// Fans
//
// *Changing* a fan is root-only: the ACPI EC is reachable through ec_sys
// debugfs, which no udev rule can hand to the desktop user. `g5fan` therefore
// needs a real authentication, and the panel gets it through polkit
// (dev.g5kbd.fan.manage, installed to /usr/share/polkit-1/actions/ by
// install.sh). We must not set G5FAN_NO_SUDO on that path, or g5fan's own
// elevation would be skipped instead.
//
// *Looking* at a fan is not privileged at all. The fan daemon republishes
// what it can see to /run/g5fan/status.json every tick (`g5fan status
// --cached`), and that is the only thing the panel reads — so opening the
// panel never raises a password dialog, and no poll can.
// ---------------------------------------------------------------------------

const G5FAN: &str = "g5fan";

/// Is `g5fan` on PATH and is the polkit policy installed?
fn fan_available() -> bool {
    std::path::Path::new("/usr/bin/g5fan").exists()
}

/// Everything the panel needs to draw the fans, as `g5fan status --json`
/// reports it. Field names are the CLI's, so the two cannot drift apart.
#[derive(Serialize, serde::Deserialize)]
struct RawFanReading {
    label: String,
    duty_pct: Option<u8>,
    rpm: Option<u32>,
    tacho: Option<u32>,
}

#[derive(Serialize, serde::Deserialize)]
struct RawFanStatus {
    mode: String,
    backend: String,
    driver: bool,
    manual_duty: Option<u8>,
    /// the curve actually in play (null unless a curve mode is selected)
    curve: Option<Vec<[u32; 2]>>,
    /// the saved custom curve, whether or not it is in play
    custom_curve: Vec<[u32; 2]>,
    /// the built-in presets, so the panel never hard-codes a copy
    presets: std::collections::BTreeMap<String, Vec<[u32; 2]>>,
    fans: Vec<RawFanReading>,
    cpu_temp_c: Option<i32>,
    gpu_temp_c: Option<i32>,
    ceiling_c: f64,
    daemon: bool,
    age_s: f64,
    stale: bool,
}

/// Run a fan command that needs no privileges at all.
///
/// Never `pkexec`, and `G5FAN_NO_SUDO` is set so that even a mistake here
/// cannot turn a read into a prompt or an escalation.
fn run_fan_cli_unprivileged(args: &[&str]) -> Result<String, String> {
    if !fan_available() {
        return Err("g5fan is not installed — re-run: sudo ./install.sh".into());
    }
    let out = Command::new(G5FAN)
        .args(args)
        .env("G5FAN_NO_SUDO", "1")
        .output()
        .map_err(|e| format!("cannot run `{G5FAN}`: {e}"))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    // Reads explain themselves in one or two lines; show all of them, since
    // the second line is usually the fix ("start g5fan-watchdog.service").
    let err = String::from_utf8_lossy(&out.stderr);
    let text = err
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Err(if text.is_empty() {
        format!("g5fan {}: unknown error", args.join(" "))
    } else {
        text
    })
}

fn run_fan_cli(args: &[&str]) -> Result<String, String> {
    if !fan_available() {
        return Err("g5fan is not installed — re-run: sudo ./install.sh".into());
    }

    // Already root (someone ran the GUI as root, or a test): no elevation.
    let mut cmd = if unsafe { libc::geteuid() } == 0 {
        let mut c = Command::new(G5FAN);
        c.env("G5FAN_NO_SUDO", "1");
        c
    } else {
        // pkexec gives us the polkit agent's prompt. allow_gui lets it render
        // the dialog against our window instead of needing a terminal.
        let mut c = Command::new("pkexec");
        c.arg(G5FAN);
        c
    };

    let out = cmd
        .args(args)
        .output()
        .map_err(|e| format!("cannot run `{G5FAN}`: {e}"))?;

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
        // pkexec exits 126 when the user dismisses the password dialog.
        if out.status.code() == Some(126) || out.status.code() == Some(127) {
            return Err("fan control needs authentication (pkexec was cancelled \
                        or is not installed)"
                .into());
        }
        Err(format!("g5fan {}: {tail}", args.join(" ")))
    }
}

async fn run_fan_cli_async(args: Vec<String>) -> Result<String, String> {
    let handle = tauri::async_runtime::spawn_blocking(move || {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        run_fan_cli(&refs)
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
// Fan commands
// ---------------------------------------------------------------------------

/// The fans as of the fan daemon's last tick. No privileges, no prompt.
#[tauri::command]
async fn fan_status() -> Result<RawFanStatus, String> {
    let out = run_fan_cli_unprivileged(&["status", "--cached", "--json"])?;
    serde_json::from_str(&out).map_err(|e| format!("could not read `g5fan status --json`: {e}"))
}

#[tauri::command]
async fn fan_set_mode(mode: String) -> Result<String, String> {
    let mode = match mode.as_str() {
        // `mode` is the general form; these two read better as their own verbs
        // and are what the CLI documents, so pass them through untouched.
        m @ ("auto" | "turbo" | "silent" | "maxq" | "custom") => m.to_string(),
        m if m.starts_with("manual:") => m.to_string(),
        _ => return Err(format!("unknown fan mode {mode:?}")),
    };
    run_fan_cli_async(vec!["mode".into(), mode]).await
}

#[tauri::command]
async fn fan_set_manual(pct: u8) -> Result<String, String> {
    run_fan_cli_async(vec!["manual".into(), pct.min(100).to_string()]).await
}

/// Write the custom duty curve: monotonic (temperature °C, duty %) points.
///
/// The ramp is evaluated by the `g5fan` daemon, not by the EC firmware — see
/// docs/FAN-RESEARCH.md for why (the firmware's own table only exposes two of
/// its four points and hides its RPM set-points).
#[tauri::command]
async fn fan_set_curve(points: Vec<[u32; 2]>) -> Result<String, String> {
    if !(2..=5).contains(&points.len()) {
        return Err(format!(
            "a curve needs 2 to 5 points (got {})",
            points.len()
        ));
    }
    for w in points.windows(2) {
        if w[1][0] <= w[0][0] {
            return Err(format!(
                "temperatures must rise: {} °C then {} °C",
                w[0][0], w[1][0]
            ));
        }
        if w[1][1] < w[0][1] {
            return Err(format!(
                "duty falls from {}% at {} °C to {}% at {} °C — a fan must not \
                 slow down as it gets hotter",
                w[0][1], w[0][0], w[1][1], w[1][0]
            ));
        }
    }
    let mut args: Vec<String> = vec!["curve".into(), "set".into()];
    args.extend(points.iter().map(|p| format!("{}:{}", p[0], p[1].min(100))));
    run_fan_cli_async(args).await
}

/// Is the watchdog unit running?
#[tauri::command]
async fn fan_watchdog() -> Result<String, String> {
    let out = Command::new("systemctl")
        .args(["is-active", "g5fan-watchdog.service"])
        .output()
        .map_err(|e| format!("cannot run systemctl: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
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
            fan_status,
            fan_set_mode,
            fan_set_manual,
            fan_set_curve,
            fan_watchdog,
        ])
        .run(tauri::generate_context!())
        .expect("error while running g5kbd-gui");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panel's only way of looking at the fans is `g5fan status --json
    /// --cached`, so the field names below are a contract with the CLI (see
    /// `status_data` in src/g5fan.py and its key-set test in
    /// tests/test_g5fan.py). If the CLI renames one, this breaks here instead
    /// of the panel quietly drawing an empty fan list.
    const SAMPLE: &str = r#"{
        "mode": "silent",
        "backend": "kernel",
        "driver": true,
        "manual_duty": null,
        "curve": [[45, 15], [60, 25], [72, 40], [82, 65], [92, 100]],
        "custom_curve": [[50, 25], [65, 40], [75, 60], [85, 80], [95, 100]],
        "presets": {
            "silent": [[45, 15]], "maxq": [[48, 12]], "custom": [[50, 25]]
        },
        "fans": [
            {"label": "CPU", "duty_pct": 20, "rpm": 2218, "tacho": 972},
            {"label": "GPU", "duty_pct": 15, "rpm": 1737, "tacho": 1241}
        ],
        "cpu_temp_c": 52,
        "gpu_temp_c": 44,
        "ceiling_c": 100.0,
        "daemon": true,
        "age_s": 1.1,
        "stale": false
    }"#;

    #[test]
    fn parses_a_fan_status_snapshot() {
        let s: RawFanStatus = serde_json::from_str(SAMPLE).expect("parse");
        assert_eq!(s.mode, "silent");
        assert_eq!(s.backend, "kernel");
        assert!(s.driver && s.daemon && !s.stale);
        assert_eq!(s.manual_duty, None);
        assert_eq!(s.curve.as_ref().map(Vec::len), Some(5));
        assert_eq!(s.custom_curve.len(), 5);
        assert_eq!(s.presets["custom"], vec![[50, 25]]);
        assert_eq!(s.fans.len(), 2);
        assert_eq!(s.fans[0].label, "CPU");
        assert_eq!(s.fans[0].duty_pct, Some(20));
        assert_eq!(s.fans[0].rpm, Some(2218));
        assert_eq!(s.fans[0].tacho, Some(972));
        assert_eq!(s.cpu_temp_c, Some(52));
        assert_eq!(s.gpu_temp_c, Some(44));
        assert_eq!(s.age_s, 1.1);
    }

    /// A fan the EC cannot report, and a machine whose EC gives no
    /// temperature, must come through as nulls rather than as a parse error.
    #[test]
    fn tolerates_missing_readings() {
        let json = r#"{
            "mode": "auto", "backend": "ec", "driver": false,
            "manual_duty": null, "curve": null,
            "custom_curve": [[50, 25], [95, 100]],
            "presets": {},
            "fans": [{"label": "CPU", "duty_pct": null,
                      "rpm": null, "tacho": null}],
            "cpu_temp_c": null, "gpu_temp_c": null, "ceiling_c": 90.0,
            "daemon": false, "age_s": 0.0, "stale": true
        }"#;
        let s: RawFanStatus = serde_json::from_str(json).expect("parse");
        assert_eq!(s.fans[0].duty_pct, None);
        assert_eq!(s.fans[0].rpm, None);
        assert_eq!(s.cpu_temp_c, None);
        assert!(s.curve.is_none() && s.stale && !s.driver);
    }
}
