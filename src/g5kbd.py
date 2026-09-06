#!/usr/bin/env python3
"""
g5kbd — keyboard backlight control for the Gigabyte G5 (Clevo-ODM) laptop.

Drives the single-zone RGB keyboard backlight through one of two backends:

  1. The kernel driver (preferred): /sys/class/leds/rgb:kbd/  (brightness,
     red/green/blue, colour convenience file). See ../kernel/.
  2. Direct EC mailbox (fallback, no kernel module): the ACPI EC command
     mailbox at RAM offsets 0xF8..0xFD — the protocol reverse-engineered in
     this repo (see ../docs/WINDOWS-RESEARCH.md):

       F8 FCMD  doorbell  — written LAST, triggers execution
       F9 FDAT  sub-command
       FA FBUF  parameter 1     FB FBF1  parameter 2
       FC FBF2  parameter 3     FD FBF3  parameter 4

     master enable  : FDAT=0x0C FBUF=0x3F doorbell 0xC4   (disable: FBUF=0x20)
     set color zone0: FDAT=0x03 FBUF=B FBF1=R FBF2=G doorbell 0xCA   (B,R,G!)
     brightness     : FDAT=0x06 FBUF=level(0-255)         doorbell 0xCA

     The EC silently ignores colour/brightness until master-enabled, and
     forgets everything at reboot/suspend.

Usage examples:
  g5kbd color ff4500         set static colour (RRGGBB hex or a name)
  g5kbd brightness 60        set brightness 0..100 %
  g5kbd on / off / state / restore
  g5kbd probe                cycle red/green/blue/white (watch the keyboard)
  g5kbd effect breathe --color ff0066   host-driven animated effects
  g5kbd effect cycle --speed 8
  g5kbd effect stop

Effects are host-driven animation (the EC has no firmware modes — verified in
the DSDT and live on hardware): breathe pulses the brightness of a colour,
cycle sweeps the hue. They stop at reboot.

Runs as root (auto-elevates via sudo unless G5KBD_NO_SUDO); G5KBD_FAKE=1
dry-runs without hardware; G5KBD_BACKEND=ec|led forces a backend.
"""
from __future__ import annotations

import argparse
import fcntl
import glob
import json
import math
import os
import signal
import subprocess
import sys
import time

PROG = "g5kbd"
STATE_PATH = os.environ.get("G5KBD_STATE", "/var/lib/g5kbd/state.json")
LOCK_PATH = os.environ.get("G5KBD_LOCK", "/run/lock/g5kbd-ec.lock")
EC_SYS_GLOB = "/sys/kernel/debug/ec/ec*/io"
LED_PATH = os.environ.get("G5KBD_LED_PATH", "/sys/class/leds/rgb:kbd")

# --------------------------------------------------------------------------
# EC mailbox registers and commands
# --------------------------------------------------------------------------
FCMD, FDAT = 0xF8, 0xF9
FBUF, FBF1, FBF2, FBF3 = 0xFA, 0xFB, 0xFC, 0xFD

DOORBELL_KB = 0xCA
DOORBELL_EN = 0xC4

SUB_EN = 0x0C
EN_ON = 0x3F
EN_OFF = 0x20

SUB_COLOR = 0x03
SUB_BRIGHT = 0x06

DEFAULT_RGB = (0, 0, 200)   # firmware default: blue
DEFAULT_BRIGHTNESS = 100

COLOR_NAMES = {
    "red": (255, 0, 0),
    "green": (0, 255, 0),
    "blue": (0, 0, 255),
    "white": (255, 255, 255),
    "orange": (255, 128, 0),
    "yellow": (255, 255, 0),
    "cyan": (0, 255, 255),
    "purple": (128, 0, 255),
    "pink": (255, 0, 128),
    "black": (0, 0, 0),
}


def parse_color(s: str):
    s = s.strip().lower().lstrip("#")
    if s in COLOR_NAMES:
        return COLOR_NAMES[s]
    if len(s) == 3:
        s = "".join(ch * 2 for ch in s)
    if len(s) == 6:
        try:
            return tuple(int(s[i:i + 2], 16) for i in (0, 2, 4))
        except ValueError:
            pass
    raise ValueError("unknown colour %r (use RRGGBB hex or a name)" % s)


def pct_to_byte(pct: int) -> int:
    return round(max(0, min(100, int(pct))) * 255 / 100)


# --------------------------------------------------------------------------
# Model guard (safety): only Gigabyte G5/G6/G7 laptops, please
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
        return "this machine is %r %r — g5kbd only drives Gigabyte G5/G6/G7" % (vendor, product)
    return None


# --------------------------------------------------------------------------
# Raw EC mailbox primitives
# --------------------------------------------------------------------------
class _FakeEc:
    """Dry-run EC backend (G5KBD_FAKE=1): print what would be written."""
    name = "fake"

    def write(self, off: int, val: int) -> None:
        print("  [fake] EC 0x%02x <- 0x%02x" % (off, val))


class _DebugFsEc:
    name = "ec_sys"

    def __init__(self, path: str):
        self.path = path
        if not os.access(path, os.W_OK):
            sys.exit(
                "error: %s is read-only — load ec_sys with write support:\n"
                "    modprobe -r ec_sys && modprobe ec_sys write_support=1"
                % path)

    def write(self, off: int, val: int) -> None:
        with open(self.path, "r+b", buffering=0) as f:
            f.seek(off)
            f.write(bytes([val & 0xFF]))


def _open_raw_ec():
    if os.environ.get("G5KBD_FAKE"):
        return _FakeEc()
    cands = sorted(glob.glob(EC_SYS_GLOB))
    if not cands:
        sys.exit("error: no %s found — is ec_sys loaded?\n"
                 "    modprobe ec_sys write_support=1" % EC_SYS_GLOB)
    return _DebugFsEc(cands[0])


def _mailbox(ec, doorbell: int, sub: int, params=()) -> None:
    regs = (FBUF, FBF1, FBF2, FBF3)
    if len(params) > len(regs):
        raise ValueError("too many mailbox parameters")
    ec.write(FDAT, sub)
    for reg, val in zip(regs, params):
        ec.write(reg, val & 0xFF)
    ec.write(FCMD, doorbell)


def _ec_enable(ec, on: bool) -> None:
    _mailbox(ec, DOORBELL_EN, SUB_EN, (EN_ON if on else EN_OFF,))


def _ec_set_color(ec, rgb) -> None:
    r, g, b = rgb
    _mailbox(ec, DOORBELL_KB, SUB_COLOR, (b, r, g))      # EC wants B,R,G


def _ec_set_brightness(ec, pct: int) -> None:
    _mailbox(ec, DOORBELL_KB, SUB_BRIGHT, (pct_to_byte(pct),))


def _ec_set_level(ec, level: int) -> None:
    _mailbox(ec, DOORBELL_KB, SUB_BRIGHT, (max(0, min(255, int(level))),))


# --------------------------------------------------------------------------
# Backends: kernel LED sysfs node (preferred) or raw EC
# --------------------------------------------------------------------------
class EcBackend:
    """Direct EC mailbox backend."""
    is_ec = True
    name = "ec"

    def __init__(self):
        self.ec = _open_raw_ec()

    def enable(self, on: bool) -> None:
        _ec_enable(self.ec, on)

    def set_color(self, rgb) -> None:
        _ec_set_color(self.ec, rgb)

    def set_brightness_pct(self, pct: int) -> None:
        _ec_set_brightness(self.ec, pct)

    def set_level(self, level: int) -> None:
        _ec_set_level(self.ec, level)


class LedBackend:
    """Kernel-driver backend: /sys/class/leds/rgb:kbd/."""
    is_ec = False
    name = "led"

    def __init__(self, root: str = LED_PATH):
        self.root = root
        if not os.path.isdir(root):
            sys.exit("error: LED device %s not found — is the g5kbd kernel "
                     "module loaded? (see kernel/) or force the EC backend "
                     "with G5KBD_BACKEND=ec" % root)

    def _p(self, name: str) -> str:
        return os.path.join(self.root, name)

    def _w(self, name: str, text: str) -> None:
        with open(self._p(name), "w") as f:
            f.write(text)

    def _read_level(self) -> int:
        try:
            with open(self._p("brightness")) as f:
                return int(f.read().strip())
        except (OSError, ValueError):
            return 0

    def enable(self, on: bool) -> None:
        if on:
            if self._read_level() == 0:
                self.set_level(255)
        else:
            self.set_level(0)

    @staticmethod
    def _multi_positions(root: str) -> dict | None:
        """Map colour names to positions in multi_intensity by reading
        multi_index (format varies: 'red green blue' or '0 red 1 green 2 blue')."""
        try:
            with open(os.path.join(root, "multi_index")) as f:
                toks = f.read().split()
        except OSError:
            return None
        pos: dict = {}
        # pair format: number then name, repeating
        if len(toks) >= 2 and all(t.isdigit() for t in toks[0::2]):
            for i in range(0, len(toks) - 1, 2):
                pos[toks[i + 1]] = int(toks[i])
        else:  # bare ordered list of names
            for i, t in enumerate(toks):
                pos[t] = i
        return pos or None

    def set_color(self, rgb) -> None:
        r, g, b = (max(0, min(255, int(c))) for c in rgb)
        hexcol = "%02x%02x%02x" % (r, g, b)
        if os.path.exists(self._p("color")):
            self._w("color", hexcol)
            return
        if all(os.path.exists(self._p(c)) for c in ("red", "green", "blue")):
            self._w("red", str(r))
            self._w("green", str(g))
            self._w("blue", str(b))
            return
        # kernel led-class-multicolor: write raw intensities per channel
        multi = self._p("multi_intensity")
        if os.path.exists(multi):
            pos = self._multi_positions(self.root)
            if pos is not None and {"red", "green", "blue"} <= set(pos):
                vals = [0] * (max(pos.values()) + 1)
                for name, v in (("red", r), ("green", g), ("blue", b)):
                    vals[pos[name]] = v
                self._w("multi_intensity", " ".join(str(v) for v in vals))
                return
        sys.exit("error: LED device %s exposes neither 'color', red/green/blue "
                 "nor a parseable multi_index" % self.root)

    def set_brightness_pct(self, pct: int) -> None:
        self.set_level(pct_to_byte(pct))

    def set_level(self, level: int) -> None:
        self._w("brightness", str(max(0, min(255, int(level)))))


def open_backend():
    """LED kernel node when present, else the raw EC. Override with
    G5KBD_BACKEND=led|ec."""
    choice = os.environ.get("G5KBD_BACKEND")
    if choice == "ec":
        return EcBackend()
    if choice == "led":
        return LedBackend()
    if os.path.isdir(LED_PATH):      # kernel driver wins when loaded
        return LedBackend()
    return EcBackend()


# --------------------------------------------------------------------------
# State
# --------------------------------------------------------------------------
def default_state() -> dict:
    return {"enabled": True, "rgb": list(DEFAULT_RGB), "brightness": DEFAULT_BRIGHTNESS}


def load_state() -> dict | None:
    try:
        with open(STATE_PATH) as f:
            st = json.load(f)
    except (OSError, ValueError):
        return None
    st.setdefault("enabled", True)
    st.setdefault("rgb", list(DEFAULT_RGB))
    st.setdefault("brightness", DEFAULT_BRIGHTNESS)
    return st


def save_state(st: dict) -> None:
    os.makedirs(os.path.dirname(STATE_PATH), exist_ok=True)
    tmp = STATE_PATH + ".tmp"
    with open(tmp, "w") as f:
        json.dump(st, f, indent=2)
        f.write("\n")
    os.replace(tmp, STATE_PATH)


# --------------------------------------------------------------------------
# Apply
# --------------------------------------------------------------------------
def apply_state(b, st: dict) -> None:
    """Drive the backend to match st. The EC backend gets one flock around
    the whole sequence so two writers can never interleave enable/colour."""
    lock = open(LOCK_PATH, "w") if b.is_ec else None
    if lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
    try:
        if not st.get("enabled", True):
            b.enable(False)
            return
        b.enable(True)
        b.set_color(tuple(st["rgb"]))
        b.set_brightness_pct(int(st["brightness"]))
    finally:
        if lock:
            lock.close()


def describe(st: dict) -> str:
    rgb = tuple(st["rgb"])
    if not st.get("enabled", True):
        return "off"
    name = next((n for n, c in COLOR_NAMES.items() if c == rgb), None)
    col = name or "#%02x%02x%02x" % rgb
    return "%s, brightness %d%%" % (col, int(st["brightness"]))


# --------------------------------------------------------------------------
# Effects — host-driven animation (works on every backend)
# --------------------------------------------------------------------------
EFFECT_PID = os.environ.get("G5KBD_PID", "/run/g5kbd-effect.pid")
EFFECT_FPS = 30
EFFECT_BASES = {"breathe": 3.0, "cycle": 6.0}   # seconds per cycle at speed 5
_EFFECT_STOPPED = False


def _effect_pid() -> int | None:
    try:
        with open(EFFECT_PID) as f:
            return int(f.read().strip())
    except (OSError, ValueError):
        return None


def _pid_alive(pid: int | None) -> bool:
    if not pid:
        return False
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True


def _unregister_effect() -> None:
    try:
        os.unlink(EFFECT_PID)
    except OSError:
        pass


def _register_effect() -> None:
    os.makedirs(os.path.dirname(EFFECT_PID) or ".", exist_ok=True)
    with open(EFFECT_PID, "w") as f:
        f.write("%d\n" % os.getpid())


def stop_effect_process() -> None:
    pid = _effect_pid()
    if pid and _pid_alive(pid):
        try:
            os.kill(pid, signal.SIGTERM)
        except OSError:
            pass
        for _ in range(60):
            if not _pid_alive(pid):
                break
            time.sleep(0.05)
    if not _pid_alive(pid):
        _unregister_effect()


def _on_stop_signal(signum, frame) -> None:
    global _EFFECT_STOPPED
    _EFFECT_STOPPED = True


def hsv_to_rgb(h: float, s: float, v: float):
    i = int(h * 6.0)
    f = h * 6.0 - i
    p = v * (1.0 - s)
    q = v * (1.0 - s * f)
    t = v * (1.0 - s * (1.0 - f))
    table = ((v, t, p), (q, v, p), (p, v, t),
             (p, q, v), (t, p, v), (v, p, q))
    r, g, b = table[i % 6]
    return (int(round(r * 255)), int(round(g * 255)), int(round(b * 255)))


def _run_effect(args) -> int:
    kind = args.mode
    fake = bool(os.environ.get("G5KBD_FAKE"))
    stop_effect_process()

    saved = load_state() or default_state()
    rgb = tuple(parse_color(args.color)) if args.color else tuple(saved["rgb"])

    b = open_backend()
    period = EFFECT_BASES[kind] * 5.0 / max(1, min(10, int(args.speed)))

    global _EFFECT_STOPPED
    _EFFECT_STOPPED = False
    signal.signal(signal.SIGTERM, _on_stop_signal)
    signal.signal(signal.SIGINT, _on_stop_signal)
    _register_effect()

    print("effect %s running — Ctrl-C or 'g5kbd effect stop' to end "
          "(returns to the saved colour)" % kind)
    t0 = time.monotonic()
    frame = 0
    try:
        b.enable(True)
        b.set_color(rgb)
        b.set_brightness_pct(int(saved["brightness"]))
        step = 1.0 / EFFECT_FPS
        deadline = time.monotonic() + step
        while not _EFFECT_STOPPED and (not fake or frame < 8):
            t = time.monotonic() - t0
            if kind == "breathe":
                ph = (t % period) / period
                v = 0.5 * (1.0 - math.cos(2 * math.pi * ph))  # 0..1..0
                b.set_level(16 + int(round(239 * v)))
            else:  # cycle
                hue = (t % period) / period
                b.set_color(hsv_to_rgb(hue, 1.0, 1.0))
            frame += 1
            delay = deadline - time.monotonic()
            if delay > 0:
                time.sleep(delay)
            deadline += step
    finally:
        _unregister_effect()
        st = saved
        st["enabled"] = True
        apply_state(b, st)
    print("effect stopped — back to %s" % describe(saved))
    return 0


def cmd_effect(args) -> int:
    mode = args.mode
    if not mode or mode == "list":
        print("effects are host-driven (works on the EC or the kernel LED "
              "node):")
        for name in EFFECT_BASES:
            print("  %-8s g5kbd effect %s [--color RRGGBB] [--speed 1..10] [--bg]"
                  % (name, name))
        print("  stop      g5kbd effect stop   (returns to the saved colour)")
        return 0
    if mode == "stop":
        had_effect = _effect_pid() is not None
        stop_effect_process()
        b = open_backend()
        st = load_state()
        if st is None:
            print("no saved state to restore" if had_effect else "no effect running")
            return 0
        apply_state(b, st)
        print("stopped — keyboard backlight: %s" % describe(st))
        return 0
    if mode not in EFFECT_BASES:
        raise ValueError("unknown effect %r (try: breathe, cycle, stop, list)" % mode)

    speed = max(1, min(10, int(args.speed)))
    if args.bg and not os.environ.get("G5KBD_FAKE"):
        stop_effect_process()
        argv = [sys.executable, os.path.abspath(__file__), "effect", mode,
                "--speed", str(speed)]
        if args.color:
            argv += ["--color", args.color]
        # Inherit the environment as-is: the child must run with exactly the
        # same privileges as this process (root if we are root; the desktop
        # user against the world-writable LED node if we are not). Trying to
        # re-elevate via 'sudo -n' from a detached child fails silently.
        subprocess.Popen(argv, stdin=subprocess.DEVNULL,
                         stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL, start_new_session=True)
        for _ in range(100):
            if _effect_pid():
                break
            time.sleep(0.05)
        pid = _effect_pid()
        if not pid:
            print("error: effect failed to start (no pid after spawn) — "
                  "check that the keyboard backend is reachable")
            return 1
        print("effect %s running in background (pid %d) — stop with: "
              "g5kbd effect stop" % (mode, pid))
        return 0
    return _run_effect(args)


# --------------------------------------------------------------------------
# Commands
# --------------------------------------------------------------------------
def cmd_color(args) -> int:
    stop_effect_process()
    b = open_backend()
    st = load_state() or default_state()
    st["rgb"] = list(parse_color(args.color))
    if args.brightness is not None:
        st["brightness"] = max(0, min(100, int(args.brightness)))
    st["enabled"] = True
    apply_state(b, st)
    save_state(st)
    print("keyboard backlight: %s" % describe(st))
    return 0


def cmd_brightness(args) -> int:
    stop_effect_process()
    b = open_backend()
    st = load_state() or default_state()
    st["brightness"] = max(0, min(100, int(args.pct)))
    st["enabled"] = True
    apply_state(b, st)
    save_state(st)
    print("keyboard backlight: %s" % describe(st))
    return 0


def cmd_on(args) -> int:
    stop_effect_process()
    b = open_backend()
    st = load_state() or default_state()
    st["enabled"] = True
    apply_state(b, st)
    save_state(st)
    print("keyboard backlight: %s" % describe(st))
    return 0


def cmd_off(args) -> int:
    stop_effect_process()
    b = open_backend()
    st = load_state() or default_state()
    st["enabled"] = False
    apply_state(b, st)
    save_state(st)
    print("keyboard backlight: off")
    return 0


def cmd_restore(args) -> int:
    b = open_backend()
    st = load_state()
    if st is None and not args.force:
        print("no saved state — leaving the backlight as-is "
              "(re-run after setting a colour/brightness once)")
        return 0
    if st is None:
        st = default_state()
    apply_state(b, st)
    save_state(st)
    print("keyboard backlight: %s" % describe(st))
    return 0


def cmd_state(args) -> int:
    st = load_state()
    if st is None:
        print("no saved state (backlight is at its default)")
    else:
        print("saved state: %s" % describe(st))
    return 0


def cmd_probe(args) -> int:
    """Quick self-test that cycles colours so you can watch the keyboard."""
    b = open_backend()
    st = load_state() or default_state()
    for name, rgb in (("red", (255, 0, 0)), ("green", (0, 255, 0)),
                      ("blue", (0, 0, 255)), ("white", (255, 255, 255))):
        st["rgb"] = list(rgb)
        st["enabled"] = True
        apply_state(b, st)
        print("  %s" % name)
        time.sleep(1)
    apply_state(b, st)
    save_state(st)
    print("done (back to %s)" % describe(st))
    return 0


# --------------------------------------------------------------------------
def main(argv) -> int:
    parser = argparse.ArgumentParser(
        prog=PROG,
        description="Gigabyte G5 keyboard backlight control "
                    "(kernel LED node or EC mailbox).")
    sub = parser.add_subparsers(dest="cmd", metavar="COMMAND")

    p = sub.add_parser("color", help="set the backlight colour (name or RRGGBB)")
    p.add_argument("color")
    p.add_argument("--brightness", "-b", type=int, default=None,
                   help="set brightness 0..100 %% at the same time")
    p.set_defaults(func=cmd_color)

    p = sub.add_parser("brightness", help="set brightness 0..100 %%")
    p.add_argument("pct", type=int)
    p.set_defaults(func=cmd_brightness)

    sub.add_parser("on", help="turn the backlight on").set_defaults(func=cmd_on)
    sub.add_parser("off", help="turn the backlight off").set_defaults(func=cmd_off)

    p = sub.add_parser("restore", help="re-apply the saved state (boot/resume)")
    p.add_argument("--force", action="store_true",
                   help="apply the default even if nothing is saved")
    p.set_defaults(func=cmd_restore)

    sub.add_parser("state", help="show the saved state").set_defaults(func=cmd_state)
    sub.add_parser("probe", help="cycle red/green/blue/white as a self-test") \
        .set_defaults(func=cmd_probe)

    p = sub.add_parser("effect", help="animated effects (host-driven)")
    p.add_argument("mode", nargs="?", help="breathe | cycle | stop | list")
    p.add_argument("--color", "-c", default=None,
                   help="base colour for 'breathe' (name or RRGGBB)")
    p.add_argument("--speed", type=int, default=5,
                   help="animation speed 1..10 (default 5)")
    p.add_argument("--bg", action="store_true",
                   help="run in the background (stop with 'g5kbd effect stop')")
    p.set_defaults(func=cmd_effect)

    args = parser.parse_args(argv)
    if not getattr(args, "func", None):
        parser.print_help()
        return 1

    # ``state`` only reads a file — no root needed. Everything else talks to
    # the hardware, so auto-elevate unless we are root / dry-running / opted out.
    needs_root = args.cmd != "state"
    if (needs_root and os.geteuid() != 0
            and not os.environ.get("G5KBD_FAKE")
            and not os.environ.get("G5KBD_NO_SUDO")):
        import shutil
        if shutil.which("sudo"):
            print("%s needs root to reach the hardware — elevating via sudo..."
                  % PROG)
            # -n: fail fast instead of hanging on a password prompt (e.g. a
            # background effect). Run 'sudo -v' once to cache credentials.
            os.execvp("sudo", ["sudo", "-n", sys.executable,
                               os.path.abspath(__file__)] + argv)
        print("error: needs root (no sudo found); run: sudo %s %s" % (PROG, " ".join(argv)))
        return 1

    if args.cmd == "state":
        return cmd_state(args)

    problem = model_check()
    if problem and not os.environ.get("G5KBD_UNSAFE"):
        print("refusing to run: %s" % problem)
        print("(override with G5KBD_UNSAFE=1 if you know what you are doing)")
        return 2

    try:
        return args.func(args)
    except ValueError as e:
        print("error: %s" % e)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
