//! Everything the product persists: the fan state, the keyboard state,
//! profiles, the published fan snapshot, and the EC lock that keeps two writers
//! from interleaving.
//!
//! Two rules learned the hard way and encoded here:
//!
//! * **Writes are atomic.** Every file is written to `path.tmp` and renamed, so
//!   a reader never sees a half-written JSON document. The snapshot is *also*
//!   world-readable: the panel has no business authenticating just to look at a
//!   fan, so `chmod 0644` is part of publishing it, not an afterthought.
//! * **Loading is lenient, using is strict.** A state file written by an older
//!   version (a flat `[T2, D2, T3, D3]` curve, say) must not crash a CLI or,
//!   worse, be driven onto the fans: it is dropped in favour of the built-in
//!   preset. [`as_curve`] is that gate, and it is the daemon's own rule.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::fan::{self, Point, RawFanStatus};

/// A snapshot older than this means the daemon died, not that the machine is
/// quiet.
pub const SNAPSHOT_MAX_AGE: f64 = 45.0;

// --------------------------------------------------------------------------
// Files
// --------------------------------------------------------------------------
/// Write `bytes` to `path` atomically, with an optional mode.
///
/// The mode matters for the files a non-root reader needs: Python did the same
/// with an explicit `os.chmod` before the rename.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: Option<u32>) -> io::Result<()> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        fs::create_dir_all(dir)?;
    }
    let tmp = tmp_path(path);
    {
        let mut file = File::create(&tmp)?;
        use std::io::Write as _;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    if let Some(mode) = mode {
        set_mode(&tmp, mode)?;
    }
    fs::rename(&tmp, path)
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

/// Hold an exclusive `flock` on `path` for as long as the returned file is open.
///
/// The lock file is not the state: it exists only to serialise writers, exactly
/// as `g5fan._lock()` does.
pub fn lock_exclusive(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        fs::create_dir_all(dir)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    // SAFETY: `flock` on an owned, open file descriptor; nothing outlives it.
    let rc = unsafe { libc::flock(std::os::unix::io::AsRawFd::as_raw_fd(&file), libc::LOCK_EX) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

/// Pretty JSON with a trailing newline and sorted keys — the shape Python's
/// `json.dump(..., indent=2, sort_keys=True)` writes, so the state files stay
/// byte-identical across the migration.
pub fn to_pretty_json(value: &Value) -> Result<Vec<u8>, serde_json::Error> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    Ok(text.into_bytes())
}

fn read_json(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

// --------------------------------------------------------------------------
// Fan state
// --------------------------------------------------------------------------
/// `/var/lib/g5fan/state.json`.
///
/// `curve` stays a raw [`Value`] on purpose: the daemon keeps whatever is in the
/// file and only coerces it where it is used (see [`as_curve`]), so a malformed
/// curve is *dropped*, never repaired into something the fans would then run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FanState {
    pub mode: String,
    pub duty: Option<u8>,
    pub curve: Option<Value>,
}

impl Default for FanState {
    fn default() -> Self {
        Self {
            mode: "auto".to_string(),
            duty: None,
            curve: None,
        }
    }
}

impl FanState {
    pub fn load(path: &Path) -> Self {
        let Some(value) = read_json(path) else {
            return Self::default();
        };
        let Some(object) = value.as_object() else {
            return Self::default();
        };
        Self {
            mode: object
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("auto")
                .to_string(),
            duty: object
                .get("duty")
                .and_then(Value::as_u64)
                .filter(|d| *d <= 100)
                .map(|d| d as u8),
            curve: object.get("curve").cloned(),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let value = serde_json::to_value(self).map_err(io::Error::other)?;
        // World-readable: which mode is selected is not a secret, and the panel
        // has no business authenticating to find it out. Only root can write it.
        write_atomic(
            path,
            &to_pretty_json(&value).map_err(io::Error::other)?,
            Some(0o644),
        )
    }

    /// The custom curve to drive, or `None` when it is missing or malformed.
    pub fn custom_curve(&self) -> Option<Vec<Point>> {
        as_curve(self.curve.as_ref())
    }

    /// The points a curve mode is driven with: the saved custom curve for
    /// `custom`, else the built-in preset.
    pub fn curve_for_mode(&self, mode: &str) -> Option<Vec<Point>> {
        if mode == "custom"
            && let Some(saved) = self.custom_curve()
        {
            return Some(saved);
        }
        fan::preset(mode).map(<[Point]>::to_vec)
    }
}

/// Coerce a stored curve into a validated point list, or `None`.
///
/// This is the gate the daemon's `as_curve` is: anything malformed — including a
/// curve written by an older version in a different shape — is dropped in favour
/// of the built-in preset rather than driven onto the fans.
pub fn as_curve(value: Option<&Value>) -> Option<Vec<Point>> {
    let items = value?.as_array()?;
    let mut points = Vec::with_capacity(items.len());
    for item in items {
        let pair = item.as_array()?;
        if pair.len() != 2 {
            return None;
        }
        let temp = pair[0].as_i64()?;
        let duty = pair[1].as_i64()?;
        points.push([temp as i32, duty as i32]);
    }
    fan::validate_curve(&points).ok()?;
    Some(points)
}

// --------------------------------------------------------------------------
// Keyboard state
// --------------------------------------------------------------------------
/// `/var/lib/g5kbd/state.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KbState {
    pub enabled: bool,
    pub rgb: [u8; 3],
    pub brightness: u8,
}

impl Default for KbState {
    fn default() -> Self {
        Self {
            enabled: true,
            // The firmware's own default: blue.
            rgb: [0, 0, 200],
            brightness: 100,
        }
    }
}

impl KbState {
    /// Lenient: a missing or damaged file means the defaults, never an error.
    pub fn load(path: &Path) -> Option<Self> {
        let value = read_json(path)?;
        let object = value.as_object()?;
        let mut state = Self::default();
        if let Some(enabled) = object.get("enabled").and_then(Value::as_bool) {
            state.enabled = enabled;
        }
        if let Some(rgb) = object.get("rgb").and_then(Value::as_array) {
            for (slot, channel) in state.rgb.iter_mut().zip(rgb) {
                if let Some(value) = channel.as_u64() {
                    *slot = value.min(255) as u8;
                }
            }
        }
        if let Some(brightness) = object.get("brightness").and_then(Value::as_u64) {
            state.brightness = brightness.min(100) as u8;
        }
        Some(state)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let value = serde_json::to_value(self).map_err(io::Error::other)?;
        write_atomic(
            path,
            &to_pretty_json(&value).map_err(io::Error::other)?,
            None,
        )
    }
}

// --------------------------------------------------------------------------
// Profiles
// --------------------------------------------------------------------------
/// One JSON object of named presets, sorted on disk so a diff of two installs
/// is readable.
pub fn load_profiles(path: &Path) -> BTreeMap<String, Value> {
    match read_json(path).and_then(|v| v.as_object().cloned()) {
        Some(object) => object.into_iter().collect(),
        None => BTreeMap::new(),
    }
}

pub fn save_profiles(path: &Path, profiles: &BTreeMap<String, Value>) -> io::Result<()> {
    let value = Value::Object(
        profiles
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    );
    write_atomic(
        path,
        &to_pretty_json(&value).map_err(io::Error::other)?,
        Some(0o644),
    )
}

/// A profile name the CLI accepts, or the reason it does not.
pub fn valid_profile_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 24 {
        return Err("profile name must be 1..24 characters".to_string());
    }
    let allowed = |c: char| c.is_alphanumeric() || " _-.Ññ".contains(c);
    if !name.chars().all(allowed) {
        return Err("profile name may only contain letters, digits, space, _ - .".to_string());
    }
    Ok(name.to_string())
}

// --------------------------------------------------------------------------
// The published snapshot
// --------------------------------------------------------------------------
/// Hand a reading to whoever can read the file.
///
/// Best effort by design: a failure here must never break a tick or a command,
/// and an unreadable snapshot only costs the panel its numbers.
pub fn publish_snapshot(path: &Path, data: &RawFanStatus) -> io::Result<()> {
    let value = serde_json::to_value(data).map_err(io::Error::other)?;
    write_atomic(
        path,
        &to_pretty_json(&value).map_err(io::Error::other)?,
        Some(0o644),
    )
}

/// The last reading the daemon published, with its age stamped on.
///
/// Returns the raw JSON object (the panel parses it into its own type) with
/// `age_s` and `stale` filled in, or `None` when there is nothing usable.
pub fn read_snapshot(path: &Path) -> Option<Value> {
    let mut value = read_json(path)?;
    let object = value.as_object_mut()?;
    if object
        .get("fans")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return None;
    }
    let generated = object.get("generated").and_then(Value::as_f64)?;
    let age = now_seconds() - generated;
    object.insert("age_s".to_string(), Value::from(round1(age)));
    object.insert("stale".to_string(), Value::from(age > SNAPSHOT_MAX_AGE));
    Some(value)
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Seconds since the epoch, as a float — what the daemon stamps into a snapshot.
pub fn now_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default()
}

/// The paths the product uses, honouring the environment overrides the CLIs and
/// their tests have always had.
#[derive(Clone, Debug)]
pub struct Paths {
    pub fan_state: PathBuf,
    pub fan_lock: PathBuf,
    pub fan_snapshot: PathBuf,
    pub kb_state: PathBuf,
    pub kb_lock: PathBuf,
    pub kb_pid: PathBuf,
}

impl Paths {
    pub fn from_env() -> Self {
        let env = |name: &str, fallback: &str| {
            std::env::var_os(name).map_or_else(|| PathBuf::from(fallback), PathBuf::from)
        };
        Self {
            fan_state: env("G5FAN_STATE", "/var/lib/g5fan/state.json"),
            fan_lock: env("G5FAN_LOCK", "/run/lock/g5fan-ec.lock"),
            fan_snapshot: env("G5FAN_SNAPSHOT", "/run/g5fan/status.json"),
            kb_state: env("G5KBD_STATE", "/var/lib/g5kbd/state.json"),
            kb_lock: env("G5KBD_LOCK", "/run/lock/g5kbd-ec.lock"),
            kb_pid: env("G5KBD_PID", "/run/g5kbd-effect.pid"),
        }
    }

    /// The profiles file sits next to the state file, as it always has.
    pub fn fan_profiles(&self) -> PathBuf {
        self.fan_state.with_file_name("profiles.json")
    }

    pub fn kb_profiles(&self) -> PathBuf {
        self.kb_state.with_file_name("profiles.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fan::{RawFanReading, preset};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("g5core-state-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn a_missing_state_file_is_the_default_one() {
        let dir = scratch("missing");
        let state = FanState::load(&dir.join("nope.json"));
        assert_eq!(state.mode, "auto");
        assert_eq!(state.duty, None);
        assert_eq!(state.curve, None);
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn the_fan_state_round_trips_and_stays_readable() {
        let dir = scratch("round-trip");
        let path = dir.join("state.json");
        let state = FanState {
            mode: "custom".to_string(),
            duty: None,
            curve: Some(serde_json::json!([[50, 25], [95, 100]])),
        };
        state.save(&path).expect("save");
        let loaded = FanState::load(&path);
        assert_eq!(loaded.mode, "custom");
        assert_eq!(loaded.custom_curve(), Some(vec![[50, 25], [95, 100]]));

        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(&path).expect("stat").permissions().mode();
        assert_eq!(mode & 0o777, 0o644, "the panel must be able to read it");
        assert!(fs::read_to_string(&path).expect("read").ends_with("}\n"));
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// The first cut of the CLI stored a flat `[T2, D2, T3, D3]`. It has to be
    /// dropped rather than driven onto the fans.
    #[test]
    fn an_older_curve_shape_is_dropped_not_repaired() {
        assert_eq!(as_curve(Some(&serde_json::json!([55, 25, 70, 50]))), None);
        assert_eq!(as_curve(Some(&serde_json::json!([]))), None);
        assert_eq!(as_curve(Some(&serde_json::json!(["x:y"]))), None);
        assert_eq!(as_curve(Some(&serde_json::json!([[50, 20]]))), None);
        // …and a curve that dips is refused like any other.
        assert_eq!(
            as_curve(Some(&serde_json::json!([[70, 20], [60, 40]]))),
            None
        );
        assert_eq!(as_curve(None), None);
        assert_eq!(
            as_curve(Some(&serde_json::json!([[45, 15], [60, 25], [92, 100]]))),
            Some(vec![[45, 15], [60, 25], [92, 100]])
        );
    }

    #[test]
    fn a_custom_mode_falls_back_to_the_preset_when_the_saved_curve_is_junk() {
        let mut state = FanState {
            mode: "custom".to_string(),
            duty: None,
            curve: Some(serde_json::json!([55, 25, 70, 50])),
        };
        assert_eq!(
            state.curve_for_mode("custom"),
            Some(preset("custom").expect("preset").to_vec())
        );
        state.curve = Some(serde_json::json!([[40, 10], [90, 100]]));
        assert_eq!(
            state.curve_for_mode("custom"),
            Some(vec![[40, 10], [90, 100]])
        );
        // A direct mode never reports a curve.
        assert_eq!(state.curve_for_mode("auto"), None);
    }

    #[test]
    fn the_keyboard_state_survives_a_damaged_file() {
        let dir = scratch("kb");
        let path = dir.join("state.json");
        assert_eq!(KbState::load(&path), None);
        fs::write(&path, "{ not json").expect("write");
        assert_eq!(KbState::load(&path), None);
        let state = KbState {
            enabled: false,
            rgb: [1, 2, 3],
            brightness: 42,
        };
        state.save(&path).expect("save");
        let loaded = KbState::load(&path).expect("load");
        assert!(!loaded.enabled);
        assert_eq!(loaded.rgb, [1, 2, 3]);
        assert_eq!(loaded.brightness, 42);
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn profile_names_are_checked_the_way_the_cli_advertises() {
        assert_eq!(valid_profile_name("  work "), Ok("work".to_string()));
        assert_eq!(
            valid_profile_name("ñ_and-so.1"),
            Ok("ñ_and-so.1".to_string())
        );
        assert!(valid_profile_name("").is_err());
        assert!(valid_profile_name("a".repeat(25).as_str()).is_err());
        assert!(valid_profile_name("bad/name").is_err());
    }

    fn status(mode: &str) -> RawFanStatus {
        RawFanStatus {
            mode: mode.to_string(),
            backend: "kernel".to_string(),
            driver: true,
            manual_duty: None,
            curve: Some(vec![[45, 15], [92, 100]]),
            custom_curve: vec![[50, 25], [95, 100]],
            presets: BTreeMap::new(),
            fans: vec![RawFanReading {
                label: "CPU".to_string(),
                duty_pct: Some(20),
                rpm: Some(2218),
                tacho: Some(972),
            }],
            cpu_temp_c: Some(52),
            gpu_temp_c: Some(44),
            ceiling_c: 100.0,
            daemon: true,
            generated: now_seconds(),
            age_s: 0.0,
            stale: false,
        }
    }

    #[test]
    fn a_published_snapshot_reads_back_with_its_age() {
        let dir = scratch("snapshot");
        let path = dir.join("status.json");
        publish_snapshot(&path, &status("silent")).expect("publish");
        let read = read_snapshot(&path).expect("read");
        assert_eq!(read["mode"], "silent");
        assert_eq!(read["fans"][0]["label"], "CPU");
        assert_eq!(read["fans"][0]["rpm"], 2218);
        assert_eq!(read["stale"], false);
        assert!(read["age_s"].as_f64().expect("age") < 5.0);

        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(&path).expect("stat").permissions().mode();
        assert_eq!(mode & 0o777, 0o644, "an unprivileged panel reads this");
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_stale_or_broken_snapshot_is_flagged_or_ignored() {
        let dir = scratch("stale");
        let path = dir.join("status.json");

        let mut old = status("auto");
        old.daemon = false;
        publish_snapshot(&path, &old).expect("publish");
        // Backdate it by rewriting the generation stamp.
        let mut value = read_json(&path).expect("parse");
        value["generated"] = Value::from(now_seconds() - SNAPSHOT_MAX_AGE - 1.0);
        write_atomic(
            &path,
            serde_json::to_string(&value).unwrap().as_bytes(),
            None,
        )
        .expect("write");
        assert_eq!(read_snapshot(&path).expect("read")["stale"], true);

        fs::write(&path, "{\"mode\": \"silent\"").expect("write");
        assert_eq!(read_snapshot(&path), None);

        fs::write(&path, "{\"fans\": [], \"generated\": 1.0}").expect("write");
        assert_eq!(read_snapshot(&path), None);

        fs::write(&path, "{\"fans\": [{}], \"generated\": \"eleven\"}").expect("write");
        assert_eq!(read_snapshot(&path), None);
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn publishing_creates_the_directory() {
        let dir = scratch("deep");
        let path = dir.join("a/deeper/status.json");
        publish_snapshot(&path, &status("auto")).expect("publish");
        assert!(read_snapshot(&path).is_some());
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn the_lock_is_exclusive_and_survives_a_missing_directory() {
        let dir = scratch("lock");
        let path = dir.join("nested/ec.lock");
        let first = lock_exclusive(&path).expect("first lock");
        drop(first);
        let second = lock_exclusive(&path).expect("second lock");
        drop(second);
        assert!(path.exists());
        fs::remove_dir_all(&dir).expect("cleanup");
    }
}
