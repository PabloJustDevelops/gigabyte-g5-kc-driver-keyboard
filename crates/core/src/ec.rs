//! The EC register map, the two ways in, and the DMI model guard.
//!
//! Both CLIs drive the same 256-byte EC window and the same command mailbox;
//! they differ only in which registers they touch. The map lives here so the
//! offsets, the doorbells and the mailbox order are written down once:
//!
//! ```text
//! F8 FCMD  doorbell  — written LAST, triggers execution
//! F9 FDAT  sub-command
//! FA FBUF  parameter 1     FB FBF1  parameter 2
//! FC FBF2  parameter 3     FD FBF3  parameter 4
//! ```
//!
//! Two ways in, in order of preference:
//!
//! 1. the kernel driver's sysfs attributes on the `CLV0001` ACPI device (the
//!    firmware's own `_DSM` path, which is what Windows uses) — the fan side of
//!    that lives in the `g5fan` crate;
//! 2. `ec_sys`'s debugfs window, which is what `DebugFsEc` here is.
//!
//! `FakeEc` runs the mailbox for real without hardware, which is what makes the
//! fake backend's readings (and the CI smoke tests) mean something.

use std::fmt;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

// --------------------------------------------------------------------------
// Registers
// --------------------------------------------------------------------------
pub const FCMD: u8 = 0xF8;
pub const FDAT: u8 = 0xF9;
pub const FBUF: u8 = 0xFA;
pub const FBF1: u8 = 0xFB;
pub const FBF2: u8 = 0xFC;
pub const FBF3: u8 = 0xFD;

/// `FDAT=0x0C  FBUF=0x3F` master-enables the keyboard backlight.
pub const DOORBELL_EN: u8 = 0xC4;
/// Colour, brightness and anything else the keyboard answers to.
pub const DOORBELL_KB: u8 = 0xCA;
/// `FDAT=fan  FBUF=duty` / `FDAT=0xFF FBUF=fan`.
pub const DOORBELL_FAN: u8 = 0xC1;

pub const SUB_EN: u8 = 0x0C;
pub const EN_ON: u8 = 0x3F;
pub const EN_OFF: u8 = 0x20;
pub const SUB_COLOR: u8 = 0x03;
pub const SUB_BRIGHT: u8 = 0x06;

/// `FDAT=0xFF` hands a fan back to the firmware curve.
pub const FAN_AUTO: u8 = 0xFF;

// Telemetry, straight out of the DSDT's EC field declarations.
pub const REG_CPU_TEMP: u8 = 0x07;
pub const REG_GPU_TEMP: u8 = 0x0A;
pub const REG_DUTY_CPU: u8 = 0xCE;
pub const REG_DUTY_GPU: u8 = 0xCF;
/// 16-bit big-endian tacho *period* (`REG_TACH_CPU + 1` holds the low byte).
pub const REG_TACH_CPU: u8 = 0xD0;
pub const REG_TACH_GPU: u8 = 0xD2;

pub const EC_SYS_ROOT: &str = "/sys/kernel/debug/ec";

// --------------------------------------------------------------------------
// The window
// --------------------------------------------------------------------------
/// Something the EC refused to do, or that no window could be opened for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EcError {
    message: String,
}

impl EcError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for EcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EcError {}

/// A 256-byte EC window: one byte at a time, by register offset.
pub trait EcWindow {
    /// `"fake"`, `"ec_sys"`… what `status` and `doctor` print.
    fn name(&self) -> &'static str;
    fn write(&mut self, off: u8, val: u8) -> Result<(), EcError>;
    fn read(&mut self, off: u8) -> Result<u8, EcError>;
}

/// Dry-run window: echoes every write, and runs the real mailbox so that the
/// effects of a command can be read back.
///
/// It is not a stub that returns zeroes. `write` records the byte and, on a fan
/// doorbell, computes the duty and tacho registers the way the hardware does —
/// which is what lets `g5fan doctor` and the CI smoke tests exercise the real
/// code path with no EC in sight.
pub struct FakeEc {
    ram: [u8; 256],
    /// Print `[fake] EC 0xNN <- 0xNN` on every write, like the Python CLIs.
    pub echo: bool,
}

impl Default for FakeEc {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeEc {
    /// The live values a G5 KC reports at idle: both fans on the firmware curve
    /// at 35 %, the CPU fan at ~2218 rpm, the GPU fan a touch slower.
    pub fn new() -> Self {
        let mut ram = [0u8; 256];
        ram[REG_CPU_TEMP as usize] = 52;
        ram[REG_GPU_TEMP as usize] = 44;
        ram[REG_DUTY_CPU as usize] = 0x59;
        ram[REG_DUTY_GPU as usize] = 0x59;
        ram[REG_TACH_CPU as usize] = 0x03;
        ram[REG_TACH_CPU as usize + 1] = 0xCC;
        ram[REG_TACH_GPU as usize] = 0x04;
        ram[REG_TACH_GPU as usize + 1] = 0x0F;
        Self { ram, echo: true }
    }

    /// Write without printing, for tests that do not want stdout noise.
    pub fn quiet() -> Self {
        Self {
            echo: false,
            ..Self::new()
        }
    }

    fn run_mailbox(&mut self) {
        let sub = self.ram[FDAT as usize];
        let arg = self.ram[FBUF as usize];
        if sub == FAN_AUTO {
            // The firmware curve takes over again.
            self.ram[REG_DUTY_CPU as usize] = 0x59;
            self.ram[REG_DUTY_GPU as usize] = 0x59;
            self.ram[REG_TACH_CPU as usize + 1] = 0xCC;
            self.ram[REG_TACH_GPU as usize + 1] = 0x0F;
        } else if sub == 1 || sub == 2 {
            let duty_reg = if sub == 1 { REG_DUTY_CPU } else { REG_DUTY_GPU };
            let tach_reg = if sub == 1 { REG_TACH_CPU } else { REG_TACH_GPU };
            self.ram[duty_reg as usize] = arg;
            // A higher duty means a shorter tacho period.
            self.ram[tach_reg as usize + 1] = 0x10.max(0xFFu8.saturating_sub(arg));
        }
    }
}

impl EcWindow for FakeEc {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn write(&mut self, off: u8, val: u8) -> Result<(), EcError> {
        if self.echo {
            println!("  [fake] EC 0x{off:02x} <- 0x{val:02x}");
        }
        self.ram[off as usize] = val;
        if off == FCMD && val == DOORBELL_FAN {
            self.run_mailbox();
        }
        Ok(())
    }

    fn read(&mut self, off: u8) -> Result<u8, EcError> {
        Ok(self.ram[off as usize])
    }
}

/// The EC's window through `ec_sys`.
pub struct DebugFsEc {
    path: PathBuf,
    writable: bool,
}

impl DebugFsEc {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EcError> {
        let path = path.as_ref().to_path_buf();
        // Opening read-only is fine: `g5fan doctor` and `status` only read, and
        // ec_sys is very often loaded without write support.
        let writable = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .is_ok();
        if !writable && fs::File::open(&path).is_err() {
            return Err(EcError::new(format!(
                "cannot open {} — is ec_sys loaded?\n    modprobe ec_sys write_support=1",
                path.display()
            )));
        }
        Ok(Self { path, writable })
    }

    pub fn is_writable(&self) -> bool {
        self.writable
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl EcWindow for DebugFsEc {
    fn name(&self) -> &'static str {
        "ec_sys"
    }

    fn write(&mut self, off: u8, val: u8) -> Result<(), EcError> {
        if !self.writable {
            return Err(EcError::new(format!(
                "{} is read-only — load ec_sys with write support:\n    \
                 modprobe -r ec_sys && modprobe ec_sys write_support=1",
                self.path.display()
            )));
        }
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .map_err(|e| EcError::new(format!("writing {}: {e}", self.path.display())))?;
        file.seek(SeekFrom::Start(u64::from(off)))
            .map_err(|e| EcError::new(format!("seeking {}: {e}", self.path.display())))?;
        file.write_all(&[val])
            .map_err(|e| EcError::new(format!("writing {}: {e}", self.path.display())))?;
        Ok(())
    }

    fn read(&mut self, off: u8) -> Result<u8, EcError> {
        let mut file = fs::File::open(&self.path)
            .map_err(|e| EcError::new(format!("reading {}: {e}", self.path.display())))?;
        file.seek(SeekFrom::Start(u64::from(off)))
            .map_err(|e| EcError::new(format!("seeking {}: {e}", self.path.display())))?;
        let mut buf = [0u8; 1];
        file.read_exact(&mut buf)
            .map_err(|_| EcError::new(format!("short read at 0x{off:02x}")))?;
        Ok(buf[0])
    }
}

/// The `ec*/io` windows under `ec_sys`'s debugfs root, sorted — the same set
/// Python's `glob.glob("/sys/kernel/debug/ec/ec*/io")` returns.
pub fn ec_sys_candidates_in(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("ec"))
                && p.join("io").is_file()
        })
        .map(|p| p.join("io"))
        .collect();
    found.sort();
    found
}

/// Open the EC window: the fake when asked for, `ec_sys` otherwise.
pub fn open_raw_ec(fake: bool) -> Result<Box<dyn EcWindow>, EcError> {
    if fake {
        return Ok(Box::new(FakeEc::new()));
    }
    let candidates = ec_sys_candidates_in(Path::new(EC_SYS_ROOT));
    let Some(path) = candidates.first() else {
        return Err(EcError::new(format!(
            "no {EC_SYS_ROOT}/ec*/io found — is ec_sys loaded?\n    \
             modprobe ec_sys write_support=1"
        )));
    };
    Ok(Box::new(DebugFsEc::open(path)?))
}

// --------------------------------------------------------------------------
// Mailbox helpers
// --------------------------------------------------------------------------
/// Write the command mailbox: parameters first, doorbell last.
pub fn mailbox(ec: &mut dyn EcWindow, doorbell: u8, sub: u8, params: &[u8]) -> Result<(), EcError> {
    let regs = [FBUF, FBF1, FBF2, FBF3];
    if params.len() > regs.len() {
        return Err(EcError::new("too many mailbox parameters"));
    }
    ec.write(FDAT, sub)?;
    for (reg, val) in regs.iter().zip(params) {
        ec.write(*reg, *val)?;
    }
    ec.write(FCMD, doorbell)?;
    Ok(())
}

/// Master-enable or disable the keyboard backlight. The EC silently ignores
/// colour and brightness until this has been sent once.
pub fn kb_enable(ec: &mut dyn EcWindow, on: bool) -> Result<(), EcError> {
    mailbox(ec, DOORBELL_EN, SUB_EN, &[if on { EN_ON } else { EN_OFF }])
}

/// Set the keyboard colour. The EC wants the channels as **B, R, G** — not RGB.
pub fn kb_set_color(ec: &mut dyn EcWindow, rgb: [u8; 3]) -> Result<(), EcError> {
    let [r, g, b] = rgb;
    mailbox(ec, DOORBELL_KB, SUB_COLOR, &[b, r, g])
}

/// Set the keyboard brightness as a raw 0..255 level.
pub fn kb_set_level(ec: &mut dyn EcWindow, level: u8) -> Result<(), EcError> {
    mailbox(ec, DOORBELL_KB, SUB_BRIGHT, &[level])
}

/// Pin one fan (`fan` 1 = CPU, 2 = GPU) to a duty byte.
pub fn fan_set_duty(ec: &mut dyn EcWindow, fan: u8, duty: u8) -> Result<(), EcError> {
    mailbox(ec, DOORBELL_FAN, fan, &[duty])
}

/// Hand one fan back to the firmware curve.
pub fn fan_set_auto(ec: &mut dyn EcWindow, fan: u8) -> Result<(), EcError> {
    mailbox(ec, DOORBELL_FAN, FAN_AUTO, &[fan])
}

// --------------------------------------------------------------------------
// Model guard (safety): only Gigabyte G5/G6/G7 laptops, please
// --------------------------------------------------------------------------
/// The reason this machine is not supported, or `None` when it is.
///
/// Pure, so both the message and the rule are testable without a laptop.
pub fn model_problem(vendor: &str, product: &str, prog: &str) -> Option<String> {
    if vendor.is_empty() && product.is_empty() {
        return Some("cannot read DMI to confirm this is a Gigabyte G5/G6/G7".to_string());
    }
    let starts = |prefixes: [&str; 3]| prefixes.iter().any(|p| product.starts_with(p));
    if vendor != "GIGABYTE" || !starts(["G5", "G6", "G7"]) {
        return Some(format!(
            "this machine is '{vendor}' '{product}' — {prog} only drives Gigabyte G5/G6/G7"
        ));
    }
    None
}

/// One DMI string, or `""` when it cannot be read (the `%r %r` of the Python
/// version, which printed an empty string rather than failing).
pub fn dmi_read_in(root: &Path, name: &str) -> String {
    fs::read_to_string(root.join(name))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

pub fn dmi_read(name: &str) -> String {
    dmi_read_in(Path::new("/sys/class/dmi/id"), name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fake_ec_starts_where_a_live_laptop_idles() {
        let mut ec = FakeEc::quiet();
        assert_eq!(ec.name(), "fake");
        assert_eq!(ec.read(REG_CPU_TEMP), Ok(52));
        assert_eq!(ec.read(REG_GPU_TEMP), Ok(44));
        assert_eq!(ec.read(REG_DUTY_CPU), Ok(0x59));
        let tach = (u32::from(ec.read(REG_TACH_CPU).unwrap()) << 8)
            | u32::from(ec.read(REG_TACH_CPU + 1).unwrap());
        assert_eq!(crate::units::rpm_from_tach(Some(tach)), Some(2218));
    }

    #[test]
    fn a_fake_duty_command_actually_moves_the_registers() {
        let mut ec = FakeEc::quiet();
        fan_set_duty(&mut ec, 1, 153).expect("write");
        assert_eq!(ec.read(REG_DUTY_CPU), Ok(153));
        // A higher duty is a shorter period, so the fan reads as faster.
        let busy = (u32::from(ec.read(REG_TACH_CPU).unwrap()) << 8)
            | u32::from(ec.read(REG_TACH_CPU + 1).unwrap());
        assert!(busy < 0x3CC, "{busy:#x}");
    }

    #[test]
    fn a_fake_auto_command_hands_the_fans_back() {
        let mut ec = FakeEc::quiet();
        fan_set_duty(&mut ec, 1, 0).expect("write");
        assert_eq!(ec.read(REG_DUTY_CPU), Ok(0));
        fan_set_auto(&mut ec, 1).expect("write");
        assert_eq!(ec.read(REG_DUTY_CPU), Ok(0x59));
    }

    #[test]
    fn the_keyboard_colour_goes_out_as_b_r_g() {
        let mut ec = FakeEc::quiet();
        kb_set_color(&mut ec, [0x11, 0x22, 0x33]).expect("write");
        // FDAT holds the sub-command, FBUF..FBF2 the parameters in EC order.
        assert_eq!(ec.read(FDAT), Ok(SUB_COLOR));
        assert_eq!(ec.read(FBUF), Ok(0x33));
        assert_eq!(ec.read(FBF1), Ok(0x11));
        assert_eq!(ec.read(FBF2), Ok(0x22));
        assert_eq!(ec.read(FCMD), Ok(DOORBELL_KB));
    }

    #[test]
    fn the_doorbell_is_written_last() {
        // A window that panics if the doorbell is not the final write.
        struct Watchdog {
            writes: Vec<(u8, u8)>,
        }
        impl EcWindow for Watchdog {
            fn name(&self) -> &'static str {
                "watchdog"
            }
            fn write(&mut self, off: u8, val: u8) -> Result<(), EcError> {
                self.writes.push((off, val));
                Ok(())
            }
            fn read(&mut self, _off: u8) -> Result<u8, EcError> {
                Ok(0)
            }
        }
        let mut w = Watchdog { writes: Vec::new() };
        fan_set_duty(&mut w, 2, 128).expect("write");
        assert_eq!(w.writes.last(), Some(&(FCMD, DOORBELL_FAN)));
        assert_eq!(w.writes.first(), Some(&(FDAT, 2)));
    }

    #[test]
    fn a_missing_ec_says_how_to_load_one() {
        let error = open_raw_ec(false).err();
        if let Some(error) = error {
            assert!(error.message().contains("ec_sys"), "{}", error.message());
        }
    }

    #[test]
    fn the_ec_window_is_found_by_name() {
        let dir = tempfile_dir();
        fs::create_dir_all(dir.join("ec0")).expect("mkdir");
        fs::write(dir.join("ec0/io"), b"").expect("touch");
        fs::create_dir_all(dir.join("notworthy")).expect("mkdir");
        fs::write(dir.join("notworthy/io"), b"").expect("touch");
        let found = ec_sys_candidates_in(&dir);
        assert_eq!(found, vec![dir.join("ec0/io")]);
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn the_model_guard_accepts_a_g5_and_refuses_the_rest() {
        assert_eq!(model_problem("GIGABYTE", "G5 KC", "g5fan"), None);
        assert_eq!(model_problem("GIGABYTE", "G6", "g5kbd"), None);
        let other = model_problem("LENOVO", "ThinkPad", "g5fan").expect("refused");
        assert!(other.contains("only drives Gigabyte G5/G6/G7"), "{other}");
        let unknown = model_problem("", "", "g5kbd").expect("refused");
        assert!(unknown.contains("cannot read DMI"), "{unknown}");
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "g5core-ec-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }
}
