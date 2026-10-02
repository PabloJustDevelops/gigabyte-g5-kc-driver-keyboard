// Backend access: spawn the `g5kbd` / `g5fan` CLIs and read their state.
//
// Every hardware change is delegated to the CLIs, exactly as in the previous
// Tauri core — they remain the single source of truth for the protocol, state
// persistence, the fan daemon and the polkit scope. The panel never touches
// the hardware itself, except for two cheap direct sysfs writes the udev rule
// already allows the user (brightness while dragging, colour while dragging).
//
//   GPUI app  --std::process::Command-->  g5kbd / g5fan CLI
//                (LED node / EC)          (root via pkexec only for fan writes)

pub mod fan;
pub mod kbd;

use std::path::PathBuf;
use std::process::Command;

const G5KBD: &str = "g5kbd";
const G5FAN: &str = "g5fan";

pub fn default_state_file() -> PathBuf {
    PathBuf::from(
        std::env::var_os("G5KBD_STATE").unwrap_or_else(|| "/var/lib/g5kbd/state.json".into()),
    )
}

fn default_pid_file() -> PathBuf {
    PathBuf::from(std::env::var_os("G5KBD_PID").unwrap_or_else(|| "/run/g5kbd-effect.pid".into()))
}

/// Per-user pid file under $XDG_RUNTIME_DIR so a non-root panel can register
/// and stop effects (the root service keeps the /run default).
pub fn runtime_pid_file() -> PathBuf {
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
pub fn run_cli(args: &[&str]) -> Result<String, String> {
    let out = Command::new(G5KBD)
        .args(args)
        .env("G5KBD_NO_SUDO", "1")
        .env("G5KBD_PID", runtime_pid_file())
        .output()
        .map_err(|e| {
            format!("cannot run `{G5KBD}`: {e} (is it installed? run sudo ./install.sh)")
        })?;

    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail = err.trim().lines().last().unwrap_or("unknown error");
        Err(format!("g5kbd {}: {tail}", args.join(" ")))
    }
}

/// Run a fan command that needs no privileges at all.
///
/// Never `pkexec`, and `G5FAN_NO_SUDO` is set so that even a mistake here
/// cannot turn a read into a prompt or an escalation.
pub fn run_fan_cli_unprivileged(args: &[&str]) -> Result<String, String> {
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

/// Is `g5fan` on PATH?
pub fn fan_available() -> bool {
    std::path::Path::new("/usr/bin/g5fan").exists()
}

/// Run a fan command that needs root, elevating through pkexec (the polkit
/// policy dev.g5kbd.fan.manage scopes it to `g5fan` and the `wheel` group).
pub fn run_fan_cli(args: &[&str]) -> Result<String, String> {
    if !fan_available() {
        return Err("g5fan is not installed — re-run: sudo ./install.sh".into());
    }

    // Already root (someone ran the panel as root): no elevation.
    let euid = unsafe { libc::geteuid() };
    let mut cmd = if euid == 0 {
        let mut c = Command::new(G5FAN);
        c.env("G5FAN_NO_SUDO", "1");
        c
    } else {
        // pkexec gives us the polkit agent's prompt; allow_gui lets it render
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
        // pkexec exits 126 when the user dismisses the password dialog.
        if out.status.code() == Some(126) || out.status.code() == Some(127) {
            return Err(
                "fan control needs authentication (pkexec was cancelled or is not installed)"
                    .into(),
            );
        }
        let err = String::from_utf8_lossy(&out.stderr);
        let tail = err.trim().lines().last().unwrap_or("unknown error");
        Err(format!("g5fan {}: {tail}", args.join(" ")))
    }
}

/// Is the watchdog unit running? (`systemctl is-active` answers in one line.)
pub fn fan_watchdog_active() -> bool {
    Command::new("systemctl")
        .args(["is-active", "g5fan-watchdog.service"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim() == "active")
        .unwrap_or(false)
}

/// Live brightness write used while the slider is being dragged: writes the
/// LED sysfs node directly (a single file write, world-writable via the udev
/// rule) instead of spawning the CLI per tick. State is persisted by the
/// final CLI `brightness` call on release.
pub fn brightness_raw(level: u8) -> Result<(), String> {
    std::fs::write("/sys/class/leds/rgb:kbd/brightness", level.to_string())
        .map_err(|e| format!("cannot write brightness: {e}"))
}

/// Live colour write while the colour sliders are being dragged. The udev
/// rule exposes the per-channel nodes, so no CLI spawn is needed mid-drag;
/// `g5kbd color` on release is what persists the value.
pub fn color_raw(red: u8, green: u8, blue: u8) -> Result<(), String> {
    let base = "/sys/class/leds/rgb:kbd";
    for (name, value) in [("red", red), ("green", green), ("blue", blue)] {
        std::fs::write(format!("{base}/{name}"), value.to_string())
            .map_err(|e| format!("cannot write {name}: {e}"))?;
    }
    Ok(())
}
