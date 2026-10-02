#!/usr/bin/env python3
"""
g5fan — CPU/GPU fan control for the Gigabyte G5 (Clevo-ODM) laptop.

Everything here was decoded from this machine's own DSDT and cross-checked
against the Windows Control Center module; see ../docs/FAN-RESEARCH.md.

  Command mailbox (EC RAM 0xF8..0xFA, the same one the keyboard uses)

    set duty   FDAT=fan  FBUF=duty(0-255)  FCMD=0xC1
    auto       FDAT=0xFF FBUF=fan          FCMD=0xC1

  fan 1 = CPU, fan 2 = GPU (the AML addresses four; this chassis has two).

  Read-back (plain EC RAM, no mailbox)

    duty   0xCE (CPU)   0xCF (GPU)
    tacho  0xD0/0xD1 (CPU) 0xD2/0xD3 (GPU) — a 16-bit BIG-endian period, not a
           speed; the fan's speed is 2156220 / period.
    temp   0x07 (CPU)   0x0A (GPU), degrees Celsius

The same two commands are reachable through the firmware's `_DSM` (functions
0x68 and 0x69), which is what the kernel driver exposes as
`fan{1,2}_duty` and `fan_mode`. g5fan uses the driver when it is loaded and
the raw mailbox when it is not; both end in identical EC writes.

WHY THE CURVES LIVE IN USERSPACE
The firmware *does* have settable fan-curve points (function 0x0E), but only
two of the four points are writable from the OS, and each fan's three RPM
set-point words cannot be read back at all — so writing that table means
clobbering values we cannot see, with an effect nobody has been able to
confirm. Instead `g5fan supervise` (the daemon behind
g5fan-watchdog.service) samples the temperatures and drives the duty directly,
which is the same mechanism the Windows app's own "fan speed" slider ends up
using and the only one we can verify by ear.

Modes:
  auto            hand both fans back to the firmware curve (the resting state)
  turbo           pin both fans to full speed
  manual N        pin both fans to N %
  silent, maxq, custom
                  a duty curve evaluated by the daemon
  plus `curve` to read/edit the custom curve and `profile` for named presets.

LOOKING AT THE FANS IS NOT A PRIVILEGED OPERATION
The EC is root-only, so `status` reads it directly only when it has to. The
daemon republishes what it sees to /run/g5fan/status.json on every tick, and
`status --cached` reports that instead — no root, no password. `status --json`
is the same reading in a machine-readable form, and that is what the desktop
panel consumes (gui/src/backend/fan.rs). Everything that *changes* a fan
still goes through sudo/pkexec.

SAFETY. The fans are the only thing keeping this CPU alive, so:
  * `auto` is the default and the state the driver returns to on unload;
  * every write takes an flock, so two writers cannot interleave;
  * the daemon enforces a ceiling: above the CPU's own hwmon limit (90 °C if
    that is unavailable) the fans go back to the firmware curve, and a 30 %
    floor applies from 85 °C up;
  * a curve is refused unless its temperatures rise and its duties never fall;
  * the EC's own firmware thermal protection runs regardless of all of this.
G5FAN_FAKE=1 dry-runs without touching hardware. G5FAN_UNSAFE=1 skips the
DMI model guard.
"""
from __future__ import annotations

import argparse
import fcntl
import glob
import json
import os
import shutil
import signal
import subprocess
import sys
import time

PROG = "g5fan"
SELF = os.path.abspath(__file__)

STATE_PATH = os.environ.get("G5FAN_STATE", "/var/lib/g5fan/state.json")
LOCK_PATH = os.environ.get("G5FAN_LOCK", "/run/lock/g5fan-ec.lock")
EC_SYS_GLOB = "/sys/kernel/debug/ec/ec*/io"
UNIT = "g5fan-watchdog.service"

# What the daemon can see, published every tick so that reading the fans never
# needs root. Only the EC is root-only, and the daemon already samples
# everything anyway; the desktop panel has no business authenticating just to
# *look* at a fan. See `g5fan status --cached`. Kept under /run because it is
# telemetry, not state: it must not survive a reboot (it would describe a
# machine that no longer exists).
SNAPSHOT_PATH = os.environ.get("G5FAN_SNAPSHOT", "/run/g5fan/status.json")
SNAPSHOT_MAX_AGE = 45.0          # seconds before a snapshot is "the daemon died"

# The driver registers its fan attributes on the ACPI device itself
# (/sys/bus/acpi/devices/CLV0001:00). The platform device the ACPI core
# creates for it (/sys/bus/platform/devices/CLV0001:00) carries none — the
# two are linked by a `physical_node` symlink, but only the ACPI one has the
# attributes. Look in both rather than hard-coding one.
SYSFS_DIRS = (
    "/sys/bus/acpi/devices/CLV0001:*",
    "/sys/bus/platform/devices/CLV0001:*",
)

# --------------------------------------------------------------------------
# EC mailbox registers and constants
# --------------------------------------------------------------------------
FCMD, FDAT = 0xF8, 0xF9
FBUF = 0xFA
DOORBELL_FAN = 0xC1
FAN_AUTO = 0xFF

# telemetry, straight out of the DSDT's EC field declarations
REG_CPU_TEMP = 0x07
REG_GPU_TEMP = 0x0A
REG_DUTY = {1: 0xCE, 2: 0xCF}
REG_TACH = {1: 0xD0, 2: 0xD2}   # 16-bit big-endian period

# Standard Clevo formula: the tacho register is a period, not a speed.
RPM_CONST = 2_156_220
TACH_STOPPED = 0xFF00           # a stopped fan reports 0 or an 0xFFxx sentinel

FANS = (1, 2)
FAN_LABELS = {1: "CPU", 2: "GPU"}


def pct_to_byte(pct: int) -> int:
    return round(max(0, min(100, int(pct))) * 255 / 100)


def byte_to_pct(raw: int | None) -> int | None:
    return None if raw is None else round(raw * 100 / 255)


def rpm_from_tach(period: int | None) -> int | None:
    if period is None:
        return None
    if period <= 0 or period >= TACH_STOPPED:
        return 0
    return RPM_CONST // period


# --------------------------------------------------------------------------
# Curves
# --------------------------------------------------------------------------
# A curve is a list of monotonic (temperature °C, duty %) points.
CURVE_POINTS = 5

PROFILES: dict[str, tuple[tuple[int, int], ...]] = {
    "silent": ((45, 15), (60, 25), (72, 40), (82, 65), (92, 100)),
    "maxq":   ((48, 12), (62, 20), (75, 35), (85, 60), (92, 100)),
    "custom": ((50, 25), (65, 40), (75, 60), (85, 80), (95, 100)),
}
CURVE_MODES = tuple(PROFILES)

# Thermal safety, used by the daemon.
CEILING_FALLBACK_C = 90.0
FLOOR_TEMP_C = 85
FLOOR_PCT = 30
HYSTERESIS_C = 4


def duty_for(points, temp: int) -> int:
    """Piecewise-linear duty % for `temp`, clamped at the curve's ends."""
    if temp <= points[0][0]:
        return points[0][1]
    if temp >= points[-1][0]:
        return points[-1][1]
    for (t0, d0), (t1, d1) in zip(points, points[1:]):
        if t0 <= temp <= t1:
            if t1 == t0:
                return d1
            return round(d0 + (d1 - d0) * (temp - t0) / (t1 - t0))
    return points[-1][1]


def parse_point(text: str) -> tuple[int, int]:
    """'55:30' -> (55, 30): a temperature in °C and a duty in %."""
    temp, sep, duty = text.partition(":")
    if not sep:
        raise ValueError("%r is not a T:D point (for example 65:40)" % text)
    try:
        return int(temp), int(duty)
    except ValueError:
        raise ValueError("%r is not a T:D point (for example 65:40)" % text)


def validate_curve(points) -> tuple[tuple[int, int], ...]:
    """Coerce to a safe curve: 2..5 points, rising temperatures, and duties
    that never fall as the temperature climbs. Anything else is refused
    rather than silently reordered — a fan curve that dips is how a CPU
    cooks."""
    pts = tuple(points)
    if not 2 <= len(pts) <= CURVE_POINTS:
        raise ValueError("a curve needs 2..%d points (got %d)"
                         % (CURVE_POINTS, len(pts)))
    for temp, duty in pts:
        if not 0 <= temp <= 120:
            raise ValueError("temperature %d is outside 0..120 °C" % temp)
        if not 0 <= duty <= 100:
            raise ValueError("duty %d is outside 0..100 %%" % duty)
    for (t0, d0), (t1, d1) in zip(pts, pts[1:]):
        if t1 <= t0:
            raise ValueError("temperatures must rise: %d °C then %d °C"
                             % (t0, t1))
        if d1 < d0:
            raise ValueError("duty falls from %d %% at %d °C to %d %% at "
                             "%d °C — a fan must not slow down as it gets "
                             "hotter" % (d0, t0, d1, t1))
    return pts


# --------------------------------------------------------------------------
# Model guard (safety): only Gigabyte G5/G6/G7 laptops
# --------------------------------------------------------------------------
def dmi_read(name: str) -> str:
    try:
        with open("/sys/class/dmi/id/" + name) as f:
            return f.read().strip()
    except OSError:
        return ""


def model_check() -> str | None:
    vendor = dmi_read("sys_vendor")
    product = dmi_read("product_name")
    if not vendor and not product:
        return "cannot read DMI to confirm this is a Gigabyte G5/G6/G7"
    if vendor != "GIGABYTE" or not product.startswith(("G5", "G6", "G7")):
        return "this machine is %r %r — g5fan only drives Gigabyte G5/G6/G7" % (
            vendor, product)
    return None


# --------------------------------------------------------------------------
# EC access
# --------------------------------------------------------------------------
class EcUnavailable(Exception):
    """No usable EC window: ec_sys is not loaded, or debugfs is not mounted."""


class _FakeEc:
    """Dry-run EC backend (G5FAN_FAKE=1).

    It does not merely echo what would be written: it runs the same mailbox
    the real EC does, so a duty command actually moves the duty and tacho
    registers that everything else reads back. That is what makes `g5fan
    doctor` and the CI smoke tests meaningful without hardware.
    """

    name = "fake"

    # The live values a G5 KC reports at idle: both fans on the firmware curve
    # at 35 %, the CPU fan at ~2218 rpm, the GPU fan a touch slower.
    def __init__(self):
        self._ram = {
            REG_CPU_TEMP: 52, REG_GPU_TEMP: 44,
            REG_DUTY[1]: 0x59, REG_DUTY[2]: 0x59,
            REG_TACH[1]: 0x03, REG_TACH[1] + 1: 0xCC,
            REG_TACH[2]: 0x04, REG_TACH[2] + 1: 0x0F,
        }

    def write(self, off: int, val: int) -> None:
        val &= 0xFF
        print("  [fake] EC 0x%02x <- 0x%02x" % (off, val))
        self._ram[off] = val
        if off == FCMD and val == DOORBELL_FAN:
            self._mailbox()

    def _mailbox(self) -> None:
        sub = self._ram.get(FDAT, 0)
        arg = self._ram.get(FBUF, 0)
        if sub == FAN_AUTO:
            # the firmware curve takes over again
            for fan in FANS:
                self._ram[REG_DUTY[fan]] = 0x59
                self._ram[REG_TACH[fan] + 1] = 0xCC
        elif sub in REG_DUTY:
            self._ram[REG_DUTY[sub]] = arg
            # a higher duty means a shorter tacho period
            self._ram[REG_TACH[sub] + 1] = max(0x10, 0xFF - arg)

    def read(self, off: int) -> int:
        return self._ram.get(off, 0)


class _DebugFsEc:
    """The EC's 256-byte window through ec_sys."""

    name = "ec_sys"

    def __init__(self, path: str):
        self.path = path
        self.writable = os.access(path, os.W_OK)

    def write(self, off: int, val: int) -> None:
        if not self.writable:
            raise EcUnavailable(
                "%s is read-only — load ec_sys with write support:\n"
                "    modprobe -r ec_sys && modprobe ec_sys write_support=1"
                % self.path)
        with open(self.path, "r+b", buffering=0) as f:
            f.seek(off)
            f.write(bytes([val & 0xFF]))

    def read(self, off: int) -> int:
        with open(self.path, "rb", buffering=0) as f:
            f.seek(off)
            b = f.read(1)
        if len(b) != 1:
            raise IOError("short read at 0x%02x" % off)
        return b[0]


def open_raw_ec():
    """Open the EC window, or raise EcUnavailable."""
    if os.environ.get("G5FAN_FAKE"):
        return _FakeEc()
    cands = sorted(glob.glob(EC_SYS_GLOB))
    if not cands:
        raise EcUnavailable(
            "no %s found — is ec_sys loaded?\n"
            "    modprobe ec_sys write_support=1" % EC_SYS_GLOB)
    return _DebugFsEc(cands[0])


# --------------------------------------------------------------------------
# Kernel-driver backend (sysfs on the CLV0001 ACPI device)
# --------------------------------------------------------------------------
def kernel_path() -> str | None:
    """Directory holding the g5kbd driver's fan attributes, if loaded."""
    override = os.environ.get("G5FAN_SYSFS")
    roots = [override] if override else [
        p for pattern in SYSFS_DIRS for p in sorted(glob.glob(pattern))]
    for root in roots:
        if root and os.path.exists(os.path.join(root, "fan_mode")):
            return root
    return None


def _kwrite(root: str, attr: str, value) -> None:
    path = os.path.join(root, attr)
    try:
        with open(path, "w") as f:
            f.write(str(value))
    except OSError as e:
        raise RuntimeError("writing %s: %s" % (path, e))


def _kread_ints(root: str, attr: str) -> list[int] | None:
    try:
        with open(os.path.join(root, attr)) as f:
            return [int(tok) for tok in f.read().split()]
    except (OSError, ValueError):
        return None


# --------------------------------------------------------------------------
# Fan backend
# --------------------------------------------------------------------------
class FanBackend:
    """Fan control and telemetry.

    Writes go through the kernel driver when it is loaded (the firmware's own
    `_DSM` path, which is what Windows uses) and through the raw EC mailbox
    otherwise. Reads always come from the EC, because that is the only place
    the duty, the tacho and the EC's own temperatures actually live.
    """

    def __init__(self):
        # G5FAN_FAKE is a dry run: never touch sysfs or the real EC, even when
        # the module happens to be loaded.
        self.fake = bool(os.environ.get("G5FAN_FAKE"))
        self.root = None if self.fake else kernel_path()
        self.name = ("fake" if self.fake
                     else "kernel" if self.root else "ec")
        self._ec = None

    # -- EC window, opened on demand ------------------------------------
    def _open_ec(self):
        if self._ec is None:
            self._ec = open_raw_ec()
        return self._ec

    def _try_read(self, off: int) -> int | None:
        try:
            return self._open_ec().read(off)
        except (EcUnavailable, IOError, OSError):
            return None

    def _read(self, off: int) -> int:
        return self._open_ec().read(off)

    # -- writes ---------------------------------------------------------
    def set_duties(self, cpu_pct: int, gpu_pct: int) -> None:
        """Pin both fans.

        They are set together because that is the shape of the firmware
        command: `_DSM 0x68` assigns all four fans on every call, so a per-fan
        write would stop the fan it did not name (see
        docs/FAN-RESEARCH.md §3.1). The raw-EC mailbox can address one fan on
        its own, but both are sent either way so the two paths behave
        identically.
        """
        if self.root:
            _kwrite(self.root, "fan_duty", "%d %d"
                    % (pct_to_byte(cpu_pct), pct_to_byte(gpu_pct)))
            return
        ec = self._open_ec()
        for fan, pct in ((1, cpu_pct), (2, gpu_pct)):
            ec.write(FDAT, fan)
            ec.write(FBUF, pct_to_byte(pct) & 0xFF)
            ec.write(FCMD, DOORBELL_FAN)

    def auto(self) -> None:
        if self.root:
            _kwrite(self.root, "fan_mode", "auto")
            return
        ec = self._open_ec()
        for fan in FANS:
            ec.write(FDAT, FAN_AUTO)
            ec.write(FBUF, fan)
            ec.write(FCMD, DOORBELL_FAN)

    # -- telemetry ------------------------------------------------------
    def mode(self) -> str | None:
        """What the driver thinks it is doing, or None without the module."""
        if not self.root:
            return None
        try:
            with open(os.path.join(self.root, "fan_mode")) as f:
                return f.read().strip()
        except OSError:
            return None

    def duty(self, fan: int) -> int | None:
        """The duty the fan is actually at, 0..255.

        The EC is the source of truth. With the driver loaded its `fan_duty`
        attribute reads the same registers, so duty still shows up on a system
        without ec_sys — that attribute is world-readable and needs no root.
        """
        v = self._try_read(REG_DUTY[fan])
        if v is not None:
            return v
        if self.root:
            vals = _kread_ints(self.root, "fan_duty")
            if vals and len(vals) >= fan:
                return vals[fan - 1]
        return None

    def tach(self, fan: int) -> int | None:
        """Tacho period, big-endian 16-bit."""
        hi = self._try_read(REG_TACH[fan])
        lo = self._try_read(REG_TACH[fan] + 1)
        if hi is None or lo is None:
            return None
        return (hi << 8) | lo

    def rpm(self, fan: int) -> int | None:
        return rpm_from_tach(self.tach(fan))

    def ec_temp(self, kind: str) -> int | None:
        return self._try_read(REG_CPU_TEMP if kind == "cpu" else REG_GPU_TEMP)

    def cpu_temp(self) -> float | None:
        """CPU temperature: the EC's own reading, else hwmon."""
        raw = self.ec_temp("cpu")
        if raw is not None and 0 < raw < 125:
            return float(raw)
        return hwmon_temp_c(("coretemp", "k10temp", "zenpower"))

    def gpu_temp(self) -> float | None:
        raw = self.ec_temp("gpu")
        if raw is not None and 0 < raw < 125:
            return float(raw)
        return hwmon_temp_c(("amdgpu", "nouveau", "nvidia"))


def open_backend() -> FanBackend:
    return FanBackend()


def hwmon_temp_c(names: tuple[str, ...]) -> float | None:
    """Highest temp1_input among the hwmon chips whose name is in `names`."""
    best = None
    for hw in sorted(glob.glob("/sys/class/hwmon/hwmon*")):
        try:
            with open(os.path.join(hw, "name")) as f:
                if f.read().strip() not in names:
                    continue
            with open(os.path.join(hw, "temp1_input")) as f:
                val = int(f.read().strip()) / 1000.0
        except (OSError, ValueError):
            continue
        best = val if best is None else max(best, val)
    return best


def cpu_ceiling_c() -> float | None:
    """The CPU's own high-temperature threshold, if hwmon reports one."""
    for hw in sorted(glob.glob("/sys/class/hwmon/hwmon*")):
        try:
            with open(os.path.join(hw, "name")) as f:
                if f.read().strip() != "coretemp":
                    continue
            with open(os.path.join(hw, "temp1_max")) as f:
                return int(f.read().strip()) / 1000.0
        except (OSError, ValueError):
            continue
    return None


# --------------------------------------------------------------------------
# State
# --------------------------------------------------------------------------
def default_state() -> dict:
    return {"mode": "auto", "duty": None, "curve": None}


def load_state() -> dict:
    try:
        with open(STATE_PATH) as f:
            st = json.load(f)
    except (OSError, ValueError):
        return default_state()
    if not isinstance(st, dict):
        return default_state()
    st.setdefault("mode", "auto")
    st.setdefault("duty", None)
    st.setdefault("curve", None)
    return st


def save_state(st: dict) -> None:
    os.makedirs(os.path.dirname(STATE_PATH) or "/", exist_ok=True)
    tmp = STATE_PATH + ".tmp"
    with open(tmp, "w") as f:
        json.dump(st, f, indent=2, sort_keys=True)
        f.write("\n")
    # World-readable: which mode is selected is not a secret, and the panel
    # has no business authenticating to find it out. Only root can write it.
    os.chmod(tmp, 0o644)
    os.replace(tmp, STATE_PATH)


def as_curve(value):
    """Coerce a stored curve into a validated point list, or None.

    Anything malformed — including a curve written by an older version in a
    different shape — is dropped in favour of the built-in preset rather than
    driven onto the fans.
    """
    if not value:
        return None
    try:
        pts = tuple((int(p[0]), int(p[1])) for p in value)
    except (TypeError, ValueError, IndexError, KeyError):
        return None
    try:
        return validate_curve(pts)
    except ValueError:
        return None


def curve_for_mode(st: dict, mode: str) -> tuple[tuple[int, int], ...]:
    """The points to drive `mode` with: the saved custom curve if the user
    wrote one, else the built-in preset."""
    if mode == "custom":
        saved = as_curve(st.get("curve"))
        if saved:
            return saved
    return PROFILES[mode]


def _lock():
    os.makedirs(os.path.dirname(LOCK_PATH) or "/", exist_ok=True)
    f = open(LOCK_PATH, "w")
    fcntl.flock(f, fcntl.LOCK_EX)
    return f


# --------------------------------------------------------------------------
# Applying a mode
# --------------------------------------------------------------------------
def apply_mode(b: FanBackend, mode: str, duty: int | None = None) -> None:
    """Drive the backend to `mode` under an exclusive lock.

    Only the direct modes touch the hardware here. A curve mode is applied by
    the daemon, which evaluates the curve against the current temperature; all
    this does is record it in the state file.
    """
    lock = _lock()
    try:
        if mode == "auto":
            b.auto()
        elif mode == "turbo":
            b.set_duties(100, 100)
        elif mode == "manual":
            if duty is None:
                raise ValueError("manual mode needs a duty percentage")
            b.set_duties(duty, duty)
        elif mode not in CURVE_MODES:
            raise ValueError("unknown mode %r" % mode)
    finally:
        lock.close()


def mode_label(mode: str, duty: int | None = None,
               curve=None) -> str:
    if mode == "manual" and duty is not None:
        return "manual %d%%" % duty
    if mode == "turbo":
        return "turbo (100%)"
    if mode in CURVE_MODES:
        pts = curve or PROFILES[mode]
        return "%s (curve %s)" % (
            mode, " ".join("%d:%d" % p for p in pts))
    return mode


def parse_mode(text: str) -> tuple[str, int | None]:
    """Accept 'auto', 'turbo'/'max', 'silent', 'maxq', 'custom',
    'manual:60' and 'manual 60'."""
    t = text.strip().lower()
    if t in ("auto", "default"):
        return "auto", None
    if t in ("turbo", "max"):
        return "turbo", None
    if t in CURVE_MODES:
        return t, None
    if t.startswith("manual"):
        _, _, rest = t.partition(":")
        rest = rest.strip() or t[6:].strip()
        if not rest:
            return "manual", None
        try:
            pct = int(rest)
        except ValueError:
            raise ValueError("manual mode needs a percentage, e.g. manual:60")
        if not 0 <= pct <= 100:
            raise ValueError("manual duty must be 0..100 (got %d)" % pct)
        return "manual", pct
    raise ValueError(
        "unknown mode %r (try: auto, turbo, silent, maxq, custom, manual:60)"
        % text)


def _save_mode(mode: str, duty: int | None = None) -> dict:
    st = load_state()
    st["mode"] = mode
    st["duty"] = duty if mode == "manual" else None
    save_state(st)
    return st


def _apply_and_save(b: FanBackend, mode: str, duty: int | None = None) -> dict:
    apply_mode(b, mode, duty)
    return _save_mode(mode, duty)


# --------------------------------------------------------------------------
# The daemon: curve engine + thermal watchdog
# --------------------------------------------------------------------------
def unit_active() -> bool:
    if not os.path.isdir("/run/systemd/system") or not shutil.which("systemctl"):
        return False
    try:
        out = subprocess.run(["systemctl", "is-active", UNIT],
                             capture_output=True, text=True, timeout=5)
    except (OSError, subprocess.SubprocessError):
        return False
    return out.stdout.strip() == "active"


def start_unit() -> None:
    """Best effort: make sure the daemon is running so a curve mode is
    actually driven. Called by the CLI after it saves a curve mode."""
    if os.environ.get("G5FAN_FAKE") or not os.path.isdir("/run/systemd/system"):
        return
    systemctl = shutil.which("systemctl")
    if not systemctl:
        return
    try:
        subprocess.run([systemctl, "start", "--no-block", UNIT],
                       capture_output=True, timeout=10)
    except (OSError, subprocess.SubprocessError):
        pass


class Supervisor:
    """Samples the temperatures and drives the fans.

    Two jobs, in this order of importance:

      1. the thermal ceiling — above the CPU's own hwmon limit (90 °C if
         hwmon has none) the fans go back to the firmware curve, which is the
         one thing here that cannot be misconfigured;
      2. the selected curve profile — while a curve mode is active, turn the
         temperature into a duty for each fan.
    """

    def __init__(self, interval: int, ceiling: float | None):
        self.interval = max(2, int(interval))
        self.ceiling = ceiling
        self.stop = False
        self.tripped = False
        # The mode we last acted on, so a switch (silent -> auto -> silent)
        # invalidates the memory below instead of trusting it.
        self._mode: str | None = None
        # last temperature a target was computed for (hysteresis)
        self._last_temp: dict[int, int] = {}
        # last target chosen per fan — what the hysteresis holds on to
        self._last_duty: dict[int, int] = {}
        # the pair actually handed to the EC, so a tick with no change writes
        # nothing at all
        self._written: dict[int, int] = {}

    def _on_signal(self, signum, frame):
        self.stop = True

    def limit(self) -> float:
        if self.ceiling is not None:
            return self.ceiling
        return cpu_ceiling_c() or CEILING_FALLBACK_C

    def _decide(self, fan: int, points, temp: int,
                cpu_temp: int | None) -> int | None:
        """Duty % for `fan`, or None to leave it alone."""
        hot = max(temp, cpu_temp if cpu_temp is not None else temp)
        floor = FLOOR_PCT if hot >= FLOOR_TEMP_C else 0
        last_t = self._last_temp.get(fan)
        if last_t is None or abs(temp - last_t) >= HYSTERESIS_C:
            target = duty_for(points, temp)
            self._last_temp[fan] = temp
        else:
            target = self._last_duty.get(fan)
            if target is None:
                target = duty_for(points, temp)
        return max(min(target, 100), floor)

    def tick(self, b: FanBackend, limit: float) -> None:
        """One sample: act on it, then publish what we saw.

        Publishing happens on every tick, including the ones that changed
        nothing, because the snapshot's age is how the desktop panel knows
        this daemon is alive. It must never be the thing that kills a tick.
        """
        try:
            self._tick(b, limit)
        finally:
            try:
                publish_snapshot(status_data(b, daemon=True))
            except Exception:       # noqa: BLE001 - a panel is not worth a crash
                pass

    def _tick(self, b: FanBackend, limit: float) -> None:
        st = load_state()
        mode = st.get("mode", "auto")

        if mode != self._mode:
            # A mode change invalidates everything we remember: `auto`, turbo
            # and manual all rewrite the duty behind our back, and the
            # hysteresis floor belonged to the old curve. Without this,
            # `silent -> auto -> silent` at an unchanged temperature would
            # find the new targets equal to the last ones written and skip the
            # write entirely, leaving the firmware curve driving a mode the
            # user asked for and `g5fan status` reporting as active.
            self._mode = mode
            self._last_temp.clear()
            self._last_duty.clear()
            self._written.clear()

        cpu = b.cpu_temp()
        cpu_int = int(round(cpu)) if cpu is not None else None
        gpu = b.gpu_temp()
        gpu_int = int(round(gpu)) if gpu is not None else None

        # 1. the ceiling wins over everything else
        if cpu_int is not None and cpu_int >= limit:
            if not self.tripped:
                print("  %d °C >= %.0f °C — releasing the fans to the "
                      "firmware curve" % (cpu_int, limit), flush=True)
                b.auto()
                self.tripped = True
            return
        if self.tripped:
            if cpu_int is not None and cpu_int < limit - 5:
                print("  %d °C — under the ceiling again, profile re-armed"
                      % cpu_int, flush=True)
                self.tripped = False
                self._last_temp.clear()
                self._last_duty.clear()
                self._written.clear()
            else:
                return

        # 2. a curve mode is driven here; a direct mode was pinned by the CLI
        if mode not in CURVE_MODES:
            return
        points = curve_for_mode(st, mode)
        temps = {1: cpu_int, 2: gpu_int}
        fallback = cpu_int if cpu_int is not None else gpu_int
        if fallback is None:
            return

        # Both fans are decided every tick and written as one pair, because
        # the firmware command sets them together (see set_duties).
        targets = {}
        for fan in FANS:
            temp = temps[fan] if temps[fan] is not None else fallback
            targets[fan] = self._decide(fan, points, temp, cpu_int)

        if targets == self._written:
            return
        try:
            b.set_duties(targets[1], targets[2])
        except (EcUnavailable, RuntimeError) as e:
            print("  fan duty: %s" % e, flush=True)
            return
        for fan in FANS:
            if self._written.get(fan) != targets[fan]:
                print("  %s %s °C -> %d %%" % (
                    FAN_LABELS[fan],
                    "-" if temps[fan] is None else str(temps[fan]),
                    targets[fan]), flush=True)
        self._written = dict(targets)

    def run(self) -> int:
        signal.signal(signal.SIGTERM, self._on_signal)
        signal.signal(signal.SIGINT, self._on_signal)
        b = open_backend()
        limit = self.limit()
        print("g5fan daemon: every %ds, ceiling %.0f °C, backend %s"
              % (self.interval, limit, b.name), flush=True)
        print("  curve profiles: " + ", ".join(CURVE_MODES), flush=True)
        try:
            while not self.stop:
                self.tick(b, limit)
                time.sleep(self.interval)
        finally:
            self.shutdown(b)

    def shutdown(self, b: FanBackend) -> None:
        """Hand the fans back to the firmware curve if we were driving them.
        A pinned turbo/manual duty is left alone: that was an explicit choice
        by the user, and stopping the watchdog should not undo it."""
        mode = load_state().get("mode", "auto")
        if self.tripped:
            # The ceiling already released the fans; nothing left to undo.
            print("daemon stopped — fans still on the firmware curve",
                  flush=True)
            return
        if mode in CURVE_MODES:
            try:
                b.auto()
                print("daemon stopped — fans back on the firmware curve",
                      flush=True)
            except (EcUnavailable, RuntimeError) as e:
                print("warning: could not release the fans (%s)" % e,
                      flush=True)
        else:
            print("daemon stopped — %s left as it is" % mode, flush=True)


def cmd_supervise(args) -> int:
    return Supervisor(args.interval, args.ceiling).run()


# --------------------------------------------------------------------------
# Commands
# --------------------------------------------------------------------------
def _temp_line(label: str, value: float | None) -> str:
    return "%s %s" % (label, "-" if value is None else "%.0f °C" % value)


# --------------------------------------------------------------------------
# Status: one description, three readers
#
# status_data() is the single source for the live reading, for the JSON the
# desktop panel consumes and for the snapshot the daemon publishes, so the
# three can never disagree about what a fan is doing.
# --------------------------------------------------------------------------
def status_data(b: FanBackend, daemon: bool | None = None) -> dict:
    """Everything `status` reports, as plain JSON-able values."""
    st = load_state()
    mode = st.get("mode", "auto")
    data = {
        "mode": mode,
        "backend": b.name,
        "driver": b.root is not None,
        "manual_duty": st.get("duty") if mode == "manual" else None,
        "curve": ([list(p) for p in curve_for_mode(st, mode)]
                  if mode in CURVE_MODES else None),
        "custom_curve": [list(p) for p in (as_curve(st.get("curve"))
                                           or PROFILES["custom"])],
        "presets": {name: [list(p) for p in PROFILES[name]]
                    for name in CURVE_MODES},
        "fans": [],
        "cpu_temp_c": None,
        "gpu_temp_c": None,
        "ceiling_c": cpu_ceiling_c() or CEILING_FALLBACK_C,
        "daemon": unit_active() if daemon is None else daemon,
        "generated": time.time(),
    }
    for fan in FANS:
        measured = b.duty(fan)
        tach = b.tach(fan)
        data["fans"].append({
            "label": FAN_LABELS[fan],
            "duty_pct": byte_to_pct(measured),
            "rpm": rpm_from_tach(tach),
            "tacho": tach,
        })
    cpu = b.cpu_temp()
    if cpu is not None:
        data["cpu_temp_c"] = round(cpu)
    gpu = b.gpu_temp()
    if gpu is not None:
        data["gpu_temp_c"] = round(gpu)
    return data


def publish_snapshot(data: dict) -> None:
    """Hand a reading to whoever can read the file.

    Best effort by design: a failure here must never break a tick or a
    command, and an unreadable snapshot only costs the panel its numbers.
    """
    try:
        d = os.path.dirname(SNAPSHOT_PATH)
        if d:
            os.makedirs(d, exist_ok=True)
        tmp = SNAPSHOT_PATH + ".tmp"
        with open(tmp, "w") as f:
            json.dump(data, f, sort_keys=True)
        os.chmod(tmp, 0o644)
        os.replace(tmp, SNAPSHOT_PATH)
    except OSError:
        pass


def read_snapshot() -> dict | None:
    """The last reading the daemon published, with its age stamped on."""
    try:
        with open(SNAPSHOT_PATH) as f:
            data = json.load(f)
    except (OSError, ValueError):
        return None
    if not isinstance(data, dict) or not data.get("fans"):
        return None
    try:
        age = time.time() - float(data.get("generated") or 0)
    except (TypeError, ValueError):
        return None
    data["age_s"] = round(age, 1)
    data["stale"] = age > SNAPSHOT_MAX_AGE
    return data


def _render_status(data: dict, age: float | None = None) -> None:
    mode = data.get("mode", "auto")
    print("fans: %-8s [backend: %s]" % (mode, data.get("backend", "?")))
    if mode in CURVE_MODES and not data.get("daemon", True):
        print("  note: the daemon that drives curves is not running")
        print("        sudo systemctl start %s" % UNIT)
    if mode == "manual" and data.get("manual_duty") is not None:
        print("  duty  %d%%" % int(data["manual_duty"]))
    print()
    print("  %-4s %-7s %-7s %-8s" % ("fan", "duty", "rpm", "tacho"))
    for f in data.get("fans", []):
        print("  %-4s %-7s %-7s %-8s" % (
            f.get("label", "?"),
            "-" if f.get("duty_pct") is None else "%d%%" % f["duty_pct"],
            "-" if f.get("rpm") is None else str(f["rpm"]),
            "-" if f.get("tacho") is None else str(f["tacho"])))

    cpu = data.get("cpu_temp_c")
    if cpu is not None:
        ceiling = data.get("ceiling_c")
        print()
        print("  " + _temp_line("CPU", cpu)
              + ("   (ceiling %.0f °C)" % ceiling if ceiling else ""))
    gpu = data.get("gpu_temp_c")
    if gpu is not None:
        print("  " + _temp_line("GPU", gpu))

    if data.get("curve"):
        print("  curve  " + " ".join("%d:%d" % (p[0], p[1])
                                     for p in data["curve"]))
    if age is not None:
        print("  as of  %.0f s ago (published by the fan daemon)" % age)


def cmd_status(args) -> int:
    """Human-readable, JSON with --json.

    Without `--cached` the EC is read directly, which needs root. With it the
    daemon's snapshot is used instead, which does not — that is the path the
    desktop panel takes, so merely *looking* at the fans never asks for a
    password.
    """
    if args.cached:
        data = read_snapshot()
        if data is None:
            print("error: no fan snapshot at %s." % SNAPSHOT_PATH,
                  file=sys.stderr)
            print("       the fan daemon publishes it: sudo systemctl start %s"
                  % UNIT, file=sys.stderr)
            return 1
        if args.json:
            print(json.dumps(data))
            return 0
        if data["stale"]:
            print("error: the fan snapshot is %.0f s old — the fan daemon is "
                  "not running." % data["age_s"], file=sys.stderr)
            print("       sudo systemctl start %s" % UNIT, file=sys.stderr)
            return 1
        _render_status(data, age=data["age_s"])
        return 0

    b = open_backend()
    data = status_data(b)
    if args.json:
        data["age_s"] = 0.0
        data["stale"] = False
        print(json.dumps(data))
        return 0
    _render_status(data)
    return 0


def cmd_mode(args) -> int:
    mode, duty = parse_mode(args.mode)
    b = open_backend()
    st = _apply_and_save(b, mode, duty)
    print("fans: %s" % mode_label(st["mode"], st.get("duty"),
                                  curve_for_mode(st, mode)
                                  if mode in CURVE_MODES else None))
    if mode in CURVE_MODES:
        start_unit()
        if not unit_active():
            print("note: start the daemon to drive it: sudo systemctl start %s"
                  % UNIT, file=sys.stderr)
    return 0


def cmd_auto(args) -> int:
    b = open_backend()
    _apply_and_save(b, "auto")
    print("fans: auto (firmware curve)")
    return 0


def cmd_manual(args) -> int:
    pct = max(0, min(100, int(args.pct)))
    b = open_backend()
    st = _apply_and_save(b, "manual", pct)
    print("fans: %s" % mode_label(st["mode"], st.get("duty")))
    return 0


def cmd_curve(args) -> int:
    st = load_state()
    values = list(args.values)
    if values and values[0] == "set":
        values = values[1:]

    if not values:
        points = as_curve(st.get("curve")) or PROFILES["custom"]
        print("custom curve (temperature °C : duty %):")
        print("  " + " ".join("%d:%d" % p for p in points))
        print()
        print("presets:")
        for name in CURVE_MODES:
            print("  %-7s %s" % (name, " ".join(
                "%d:%d" % p for p in PROFILES[name])))
        print()
        print("set one with:  g5fan curve set 50:25 65:40 75:60 85:80 95:100")
        return 0

    if len(values) == 1 and values[0] in CURVE_MODES:
        points = PROFILES[values[0]]
    else:
        points = validate_curve(parse_point(v) for v in values)

    st["curve"] = [list(p) for p in points]
    save_state(st)
    print("custom curve set: " + " ".join("%d:%d" % p for p in points))
    if st.get("mode") == "custom":
        start_unit()
        print("fans: custom (curve applied by the daemon)")
    else:
        print("pick it up with:  g5fan custom")
    return 0


def cmd_profile(args) -> int:
    path = os.path.join(os.path.dirname(STATE_PATH) or "/", "profiles.json")
    try:
        with open(path) as f:
            profiles = json.load(f)
        if not isinstance(profiles, dict):
            profiles = {}
    except (OSError, ValueError):
        profiles = {}

    action, name = args.profile_action, args.name

    if action == "list":
        for n in sorted(profiles):
            p = profiles[n]
            label = mode_label(p.get("mode", "auto"), p.get("duty"),
                               as_curve(p.get("curve")))
            print("%s\t%s" % (n, label))
        return 0

    if action == "save":
        name = _valid_name(name or "")
        st = load_state()
        profiles[name] = {"mode": st.get("mode", "auto"),
                          "duty": st.get("duty"),
                          "curve": st.get("curve")}
        _save_profiles(path, profiles)
        print("profile saved: %s (%s)" % (name, st.get("mode", "auto")))
        return 0

    if action == "apply":
        name = _valid_name(name or "")
        if name not in profiles:
            raise ValueError("profile not found: %s" % name)
        p = profiles[name]
        curve = as_curve(p.get("curve"))
        if curve:
            st = load_state()
            st["curve"] = [list(x) for x in curve]
            save_state(st)
        b = open_backend()
        st = _apply_and_save(b, p.get("mode", "auto"),
                             p.get("duty"))
        print("applied profile %s: %s" % (
            name, mode_label(st["mode"], st.get("duty"), curve)))
        if st["mode"] in CURVE_MODES:
            start_unit()
        return 0

    if action == "delete":
        name = _valid_name(name or "")
        if name not in profiles:
            raise ValueError("profile not found: %s" % name)
        del profiles[name]
        _save_profiles(path, profiles)
        print("profile deleted: %s" % name)
        return 0

    raise ValueError("unknown profile action %r" % action)


def _save_profiles(path: str, profiles: dict) -> None:
    os.makedirs(os.path.dirname(path) or "/", exist_ok=True)
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        json.dump(profiles, f, indent=2, sort_keys=True)
        f.write("\n")
    os.chmod(tmp, 0o644)
    os.replace(tmp, path)


def _valid_name(name: str) -> str:
    name = name.strip()
    if not name or len(name) > 24:
        raise ValueError("profile name must be 1..24 characters")
    if not all(c.isalnum() or c in " _-.Ññ" for c in name):
        raise ValueError("profile name may only contain letters, digits, "
                         "space, _ - .")
    return name


def cmd_probe(args) -> int:
    """Step through the modes so the change is audible."""
    b = open_backend()
    steps = ["turbo", "manual:30", "manual:70", "auto"]
    if unit_active():
        steps = ["silent"] + steps
    print("watch the fans; Ctrl-C at any point to stop\n")
    for text in steps:
        mode, duty = parse_mode(text)
        try:
            apply_mode(b, mode, duty)
        except (RuntimeError, EcUnavailable) as e:
            print("  %-12s skipped: %s" % (text, e))
            continue
        _save_mode(mode, duty)
        print("  %-12s -> %s" % (text, mode_label(mode, duty)))
        time.sleep(args.hold)
    b.auto()
    _save_mode("auto")
    print("probe done — fans left on auto")
    return 0


def cmd_doctor(args) -> int:
    """Say which layer is broken, without guessing."""
    print("g5fan doctor")
    print()
    print("  machine        : %s / %s (BIOS %s)" % (
        dmi_read("sys_vendor") or "?", dmi_read("product_name") or "?",
        dmi_read("bios_version") or "?"))

    path = kernel_path()
    print("  kernel module  : %s" % (
        "loaded, attributes at %s" % path if path else
        "NOT loaded (or no fan_mode attribute) — falling back to the raw EC"))
    if not path:
        print("                   modprobe g5kbd, or sudo ./install.sh")

    io = sorted(glob.glob(EC_SYS_GLOB))
    if not io:
        print("  ec_sys         : NOT available — no %s" % EC_SYS_GLOB)
    else:
        print("  ec_sys         : %s (%s)" % (
            io[0], "writable" if os.access(io[0], os.W_OK) else "read-only"))

    b = open_backend()
    print("  backend        : %s" % b.name)

    cpu = b.cpu_temp()
    gpu = b.gpu_temp()
    print("  temperatures   : %s, %s" % (_temp_line("CPU", cpu),
                                         _temp_line("GPU", gpu)))
    ceiling = cpu_ceiling_c()
    print("  ceiling        : %.0f °C" % (ceiling or CEILING_FALLBACK_C))
    for fan in FANS:
        measured = b.duty(fan)
        tach = b.tach(fan)
        print("  fan %d (%s)    : duty %s, tacho %s -> %s rpm" % (
            fan, FAN_LABELS[fan],
            "%d%%" % byte_to_pct(measured) if measured is not None else "?",
            tach if tach is not None else "?",
            rpm_from_tach(tach) if tach is not None else "?"))

    st = load_state()
    print("  state          : mode %s, duty %s, custom curve %s" % (
        st.get("mode"), st.get("duty"), st.get("curve") or "-"))
    print("  daemon         : %s" % (
        "active" if unit_active() else "not running"))

    if not args.write:
        print()
        print("  add --write to test an actual duty round-trip (ends on auto)")
        return 0

    print()
    print("  duty round-trip: both fans to 60 %, then back to auto")
    probe_pct = 60
    before = {f: b.duty(f) for f in FANS}
    b.set_duties(probe_pct, probe_pct)
    time.sleep(3)
    after = {f: b.duty(f) for f in FANS}
    ok = False
    for fan in FANS:
        was, now = before[fan], after[fan]
        target = pct_to_byte(probe_pct)
        verdict = "!"
        if now is not None:
            if abs(now - target) <= 16:
                verdict = "accepted"
                ok = True
            elif was is not None and abs(now - was) <= 16:
                verdict = "IGNORED (still the firmware value)"
            else:
                verdict = "moved, but not to the target"
        print("    fan %d (%s): %s -> %s  %s" % (
            fan, FAN_LABELS[fan],
            "%d%%" % byte_to_pct(was) if was is not None else "?",
            "%d%%" % byte_to_pct(now) if now is not None else "?",
            verdict))
    print("    restoring auto ...")
    b.auto()
    _save_mode("auto")
    print()
    print("  %s" % ("SUCCESS: the EC takes our duty writes."
                    if ok else
                    "FAILURE: the EC ignored the duty write — the raw-EC "
                    "fallback cannot drive the fans here. Send this output."))
    return 0 if ok else 1


# --------------------------------------------------------------------------
def _ensure_root(argv) -> None:
    """Elevate before touching the EC.

    From a terminal we hand over to an interactive `sudo`, which can prompt.
    Without a terminal (the GUI) the caller is expected to have gone through
    `pkexec g5fan`, so all we can do is explain what is missing — which is why
    the panel only does that for writes and reads `status --cached`.
    """
    if os.geteuid() == 0 or os.environ.get("G5FAN_NO_SUDO") \
            or os.environ.get("G5FAN_FAKE"):
        return
    if sys.stdin.isatty() and shutil.which("sudo"):
        os.execvp("sudo", ["sudo", sys.executable, SELF] + list(argv))
    print("error: %s needs root to reach the EC." % PROG, file=sys.stderr)
    print("       run: sudo %s %s" % (PROG, " ".join(argv)), file=sys.stderr)
    sys.exit(1)


def main(argv) -> int:
    parser = argparse.ArgumentParser(
        prog=PROG,
        description="Gigabyte G5 fan control (auto / turbo / silent / maxq / "
                    "custom curves, plus manual duty).")
    sub = parser.add_subparsers(dest="cmd", metavar="COMMAND")

    p = sub.add_parser("status", help="fan mode, duty, rpm and temperatures")
    p.add_argument("--json", action="store_true",
                   help="machine-readable output (this is what the desktop "
                        "panel consumes)")
    p.add_argument("--cached", action="store_true",
                   help="report the snapshot the fan daemon publishes, "
                        "instead of reading the EC — no root needed")
    p.set_defaults(func=cmd_status)
    sub.add_parser("auto", help="hand both fans back to the firmware curve") \
        .set_defaults(func=cmd_auto)
    sub.add_parser("turbo", help="pin both fans to full speed") \
        .set_defaults(func=cmd_mode, mode="turbo")
    for name in CURVE_MODES:
        sub.add_parser(name, help="drive the %s curve in the daemon" % name) \
            .set_defaults(func=cmd_mode, mode=name)

    p = sub.add_parser("mode", help="switch mode")
    p.add_argument("mode", help="auto | turbo | silent | maxq | custom | "
                                "manual:60")
    p.set_defaults(func=cmd_mode)

    p = sub.add_parser("manual", help="pin the fans to a fixed duty")
    p.add_argument("pct", type=int, help="0..100 %%")
    p.set_defaults(func=cmd_manual)

    p = sub.add_parser("curve", help="show or set the custom duty curve")
    p.add_argument("values", nargs="*",
                   metavar="[set] T:D ...",
                   help="'curve' shows the curve and the presets; "
                        "'curve set 50:25 65:40 75:60 85:80 95:100' writes one")
    p.set_defaults(func=cmd_curve)

    p = sub.add_parser("supervise",
                       help="run the curve engine + thermal watchdog "
                            "(what the systemd unit runs)")
    p.add_argument("--interval", type=int, default=5,
                   help="seconds between samples (default 5)")
    p.add_argument("--ceiling", type=float, default=None,
                   help="release the fans to the firmware curve at this CPU "
                        "temperature (default: the CPU's own hwmon limit, "
                        "else 90)")
    p.set_defaults(func=cmd_supervise)
    p = sub.add_parser("watch", help="alias for supervise")
    p.add_argument("--interval", type=int, default=5)
    p.add_argument("--ceiling", type=float, default=None)
    p.set_defaults(func=cmd_supervise)

    p = sub.add_parser("profile", help="named fan-mode presets")
    p.add_argument("profile_action",
                   choices=["save", "list", "apply", "delete"])
    p.add_argument("name", nargs="?")
    p.set_defaults(func=cmd_profile)

    p = sub.add_parser("probe", help="step through the modes as a self-test")
    p.add_argument("--hold", type=float, default=5.0,
                   help="seconds per step (default 5)")
    p.set_defaults(func=cmd_probe)

    p = sub.add_parser("doctor", help="check every layer that fan control "
                                      "depends on")
    p.add_argument("--write", action="store_true",
                   help="also test a duty write and read it back (ends on "
                        "auto)")
    p.set_defaults(func=cmd_doctor)

    args = parser.parse_args(argv)
    if not getattr(args, "func", None):
        parser.print_help()
        return 1

    fake = bool(os.environ.get("G5FAN_FAKE"))
    # Reading is not a privileged operation: listing profiles, looking at the
    # fans and looking at the curve all only touch files that are already
    # world-readable. Only changing something needs the EC.
    readonly = (
        (args.cmd == "profile"
         and getattr(args, "profile_action", None) == "list")
        or (args.cmd == "status" and args.cached)
        or (args.cmd == "curve"
            and not (args.values and args.values[0] == "set"))
    )

    if not fake and not readonly:
        _ensure_root(argv)

    problem = model_check()
    if problem and not os.environ.get("G5FAN_UNSAFE"):
        print("refusing to run: %s" % problem)
        print("(override with G5FAN_UNSAFE=1 if you know what you are doing)")
        return 2

    try:
        return args.func(args)
    except ValueError as e:
        print("error: %s" % e)
        return 1
    except RuntimeError as e:
        print("error: %s" % e)
        return 1
    except EcUnavailable as e:
        print("error: %s" % e)
        return 1
    except KeyboardInterrupt:
        print()
        return 130


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
