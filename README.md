# g5kbd — Gigabyte G5 keyboard backlight & fan control for Linux

Colour / brightness / effects for the single-zone RGB keyboard of the
**Gigabyte G5** (Clevo-ODM) gaming laptop, plus CPU/GPU fan control, on Linux:

```
┌──────────────────────────────────────────────────────────┐
│ Panel g5kbd-gui  (native GPUI: one Rust binary, no root)│
├──────────────────────────────────────────────────────────┤
│ CLI  g5kbd  (Python — keyboard; no GUI needed)           │
│ CLI  g5fan  (Python — fans, profiles, thermal watchdog)  │
├──────────────────────────────────────────────────────────┤
│ Standard API: /sys/class/leds/rgb:kbd  (LED multicolor)  │
├──────────────────────────────────────────────────────────┤
│ Kernel driver  g5kbd.ko  (ACPI platform driver on        │
│ CLV0001 -> evaluates the firmware _DSM: cmd 0x67 for the  │
│ keyboard, 0x68/0x69 to set the fan duty / hand the fans  │
│ back to the firmware curve)                              │
└──────────────────────────────────────────────────────────┘
```

Verified on a **Gigabyte G5 KC** (Insyde BIOS FB08, CachyOS / kernel 7.2):
the keyboard was confirmed working on 2026-09-06 (red/green/blue/white,
brightness steps, on/off, host-driven effects). The fan commands were decoded
from this machine's own firmware on 2026-09-28, and the EC was then exercised
for real: a duty write lands on the EC's read-back, the fans change speed,
`0x69` releases them again, and the curve modes move the duty with the
temperature. `sudo g5fan doctor --write` re-checks the write path in one
command. See [docs/FAN-RESEARCH.md](docs/FAN-RESEARCH.md) §8 for what is still
open.

> These laptops ship no Linux software for the backlight or the fans. Windows
> controls both with Clevo's Control Center (which is why, in Linux, the light
> stays stuck on the firmware default: blue, full brightness, and the fans sit
> on whatever curve the BIOS chose). The full story of what Windows actually
> uses, and how each EC protocol was found, is in
> [docs/WINDOWS-RESEARCH.md](docs/WINDOWS-RESEARCH.md) and
> [docs/FAN-RESEARCH.md](docs/FAN-RESEARCH.md).

## Install

```bash
sudo ./install.sh              # everything: kernel module, CLIs, GUI, watchdog
sudo ./install.sh --no-gui     # CLI + kernel module only
sudo ./install.sh --no-fan     # skip the fan tool and its watchdog
sudo ./install.sh --uninstall  # remove all of it
```

What it installs:

| Path | Purpose |
|---|---|
| `/usr/lib/modules/<kern>/extra/g5kbd.ko` | kernel driver: ACPI device `CLV0001` → LED device `rgb:kbd` |
| `/sys/class/leds/rgb:kbd/{brightness,red,green,blue,color}` | the standard LED API (once the module is loaded) |
| `/etc/udev/rules.d/99-g5kbd.rules` | let the `wheel` group write the LED — no root needed |
| `/usr/bin/g5kbd` | CLI (falls back to the raw-EC path when the module isn't loaded) |
| `/usr/bin/g5fan` | fan CLI: modes, duty, curves, profiles, daemon, diagnostics |
| `/usr/bin/g5kbd-gui` | native GPUI panel (skipped if `cargo` is missing) |
| `/usr/lib/systemd/system/g5kbd.service` + `system-sleep/g5kbd` | restore the saved colour at boot and after suspend |
| `/usr/lib/systemd/system/g5fan-watchdog.service` | fan daemon: drives the duty curves and hands the fans back to the firmware if the CPU gets too hot (skip with `--no-fan`) |
| `/usr/share/polkit-1/actions/dev.g5kbd.fan.policy` | lets the GUI change the fans after an auth prompt |
| `/var/lib/g5kbd/state.json` | your saved keyboard state |
| `/var/lib/g5fan/state.json` | your saved fan state (written by root, readable by anyone) |

The kernel module is built against **your running kernel's headers**, so you
must re-run `sudo ./install.sh` after a kernel update (or use the DKMS flow:
`cd kernel && sudo dkms add -v 1.0.0 . && sudo dkms install -m g5kbd -v 1.0.0`).

## GUI

```bash
g5kbd-gui
```

A dark control panel with a live keyboard preview that glows in the current
colour, colour presets + a native picker + RGB sliders + hex entry,
brightness/power, saved colour profiles, and the breathe/cycle effects with
speed. The **Fans** page has the five fan modes, live per-fan duty and RPM
gauges, CPU/GPU temperature gauges, a manual-duty slider and a five-point
curve editor. Built with **[GPUI](https://gpui.rs)** (Zed's GPU-accelerated UI
framework) and the `gpui-component` widget set: it ships as one Rust binary,
with no web view and no Node toolchain. Every change goes through the
`g5kbd`/`g5fan` CLIs, so the panel never touches the hardware itself. The
keyboard needs no privileges (the udev rule grants the LED node to the `wheel`
group), and neither does *looking* at the fans — the panel draws the reading
the fan daemon last published, so polling it never raises a password dialog.
Only fan **changes** authenticate, through polkit, because the EC is
root-only.

To hack on the panel alone (no driver needed): install the CLIs with
`sudo ./install.sh --no-gui`, then `cd gui && cargo run` — the panel drives
whatever `g5kbd`/`g5fan` are on `$PATH`. The first build compiles the GPUI tree
and takes a few minutes; after that it is incremental.

## CLI

```bash
g5kbd color ff6600        # any RRGGBB colour
g5kbd color red           # or a name (red green blue white orange yellow
                          #                  cyan purple pink black)
g5kbd brightness 60       # 0..100 %
g5kbd on / off            # master enable / disable
g5kbd state               # show what is saved
g5kbd probe               # cycle red/green/blue/white to verify (watch!)
```

When the kernel module is loaded, `g5kbd` writes to `/sys/class/leds/rgb:kbd`
(no sudo needed for members of `wheel`); otherwise it falls back to driving
the EC mailbox directly and auto-elevates via sudo.

## Effects
```bash
g5kbd effect breathe --color ff0066     # pulsing glow in that colour
g5kbd effect cycle --speed 8            # rainbow colour cycling (speed 1..10)
g5kbd effect breathe --color green --bg # run in the background
g5kbd effect stop                       # back to the saved colour
```

Effects are **host-driven animations**, not firmware modes: reverse
engineering (DSDT decode + live probing, see `docs/WINDOWS-RESEARCH.md`)
showed this EC implements only master-enable, colour and brightness — there
are no "breathe/cycle/wave" firmware commands on the G5's single-zone RGB
keyboard (that's also why Windows keeps the keyboard lit only while its own
program animates). The effect engine streams colour/brightness at ~30 fps
through whichever backend is active — the kernel LED node or the EC mailbox.

- `breathe` pulses the *brightness* of a colour; default colour = saved one.
- `cycle` sweeps the full rainbow hue at full brightness.
- Run foreground and press Ctrl-C, or use `--bg` + `g5kbd effect stop`.
- Any `g5kbd color/brightness/on/off` stops a running effect first.
- Effects are not persisted — the keyboard is back to static colour after a
  reboot (the EC forgets everything anyway).

## Fans

```bash
g5fan status                      # mode, per-fan duty, RPM, temperatures
g5fan auto                        # hand both fans back to the firmware curve
g5fan turbo                       # full speed
g5fan manual 60                   # pin both fans at 60 % duty
g5fan silent                      # quiet duty curve
g5fan maxq                        # quietest duty curve
g5fan custom                      # your own duty curve
g5fan curve                       # show the curve and the presets
g5fan curve set 50:25 65:40 75:60 85:80 95:100   # T:D points, °C and %
g5fan supervise                   # the curve engine + thermal watchdog
                                  # (this is what the systemd unit runs)
g5fan probe                       # step through the modes; watch the fans
g5fan doctor [--write]            # check every layer, incl. a duty round-trip
g5fan profile save office         # named presets, like the keyboard's
```

Changing a fan needs root; *looking* at one does not. The daemon republishes
what it can see to `/run/g5fan/status.json` every tick, so an unprivileged
reader gets the same numbers with no password:

```bash
g5fan status --cached             # last reading the daemon published
g5fan status --cached --json      # the same, machine-readable (the GUI's path)
```

That snapshot is only as old as the daemon's interval; if the daemon stops,
the reading is refused rather than presented as live.

`g5kbd fan <anything>` is a shortcut for `g5fan <anything>`, if you prefer one
command.

The five modes are the ones the Windows Control Center offers (from its own
`oem.ini`: `0:Auto 1:Max 3:Silent 5:MAXQ 6:Custom`; "Max" is labelled *Turbo*
in its UI). Unlike the keyboard, the fan commands are **documented in the
firmware itself** — they were read straight out of this machine's DSDT, so
nothing had to be guessed. Full write-up, including what is still inference,
in [docs/FAN-RESEARCH.md](docs/FAN-RESEARCH.md).

### Direct modes vs. curves

`auto`, `turbo` and `manual N` are **direct**: the duty is written to the EC
once and stays there. That is all the firmware itself offers.

`silent`, `maxq` and `custom` are **curves**: `g5fan-watchdog.service` samples
the temperature every few seconds and turns the curve into a duty for each fan
(piecewise linear, 4 °C of hysteresis, and a floor that never lets a fan drop
below 30 % once anything is at 85 °C or above).

Curves live in userspace on purpose. The firmware *does* have a settable curve
table, but:

1. only **two of its four points** are writable from the OS — the other two
   belong to the firmware;
2. each fan also carries three RPM set-point words that the matching read
   command never returns, so writing the table means clobbering values we
   cannot see;
3. nobody has been able to confirm that this EC honours the table at all.

Driving the duty directly has none of those problems, is fully verifiable by
ear, and gives a complete five-point curve instead of a two-point nudge. The
trade-off is that the fans follow the curve only while the daemon runs; if you
stop it they go back to the firmware curve, which is also the default.

### Safety

Fan control is the one feature here that can hurt you, so it is fenced in:

- `auto` is the default and the resting state — the kernel driver hands the
  fans back to the firmware curve when the module is unloaded, and
  `install.sh --uninstall` does the same before removing anything.
- Every write takes an `flock`, so two writers cannot interleave.
- **Both duties are always named.** The firmware's `0x68` command assigns all
  four fans on every call — it cannot be told about one fan, and a byte left
  at zero stops that fan. So the driver's `fan_duty` attribute takes the pair
  (`"cpu gpu"`) and nothing is read on the write path; setting the CPU fan
  cannot stop the GPU fan. (Found the hard way: `g5fan doctor --write` is what
  caught it, twice.)
- Curves are validated with the same rules in the CLI, the GUI and the kernel:
  temperatures must rise and duty must never fall as it gets hotter.
- `g5fan-watchdog.service` enforces a thermal ceiling: above the CPU's **own**
  high-temp limit from hwmon (90 °C if hwmon has none) the fans go back to the
  firmware curve, which is the one thing here that cannot be misconfigured.
- Fan writes need root, so the GUI authenticates through polkit
  (`dev.g5kbd.fan.manage`, scoped to just `g5fan`).
- The EC's firmware thermal protection runs regardless of any of this, which
  is the real reason this is safe to experiment with.

Stop the daemon with
`sudo systemctl disable --now g5fan-watchdog.service`; whatever it was driving
goes back to the firmware curve when it exits.

## Effects

## How it works

Two equally-valid routes to the same EC mailbox:

1. **Kernel driver** (`kernel/g5kbd.c`) — an ACPI platform driver that binds to
   the `CLV0001` device (`\_SB_.DCHU` — the very device Windows' AcpiBridge
   driver and the TUXEDO/Clevo drivers use) and drives the backlight by
   evaluating the firmware's own `_DSM` (Clevo UUID, command `0x67`). It
   registers a standard **multicolor LED** (`rgb:kbd`) — kernel brightness
   semantics, sysfs, and udev access control for free. This is the
   mainline-style design; no raw EC pokes in kernel code.
2. **EC fallback** (when the module isn't loaded) — the CLI writes the
   mailbox directly through the kernel's `ec_sys` debugfs interface.

The mailbox itself (EC RAM, behind the ACPI EC interface):

| Offset | Name | Role |
|---|---|---|
| `0xF8` | FCMD | doorbell — written **last**, triggers execution |
| `0xF9` | FDAT | sub-command |
| `0xFA` | FBUF | parameter 1 |
| `0xFB` | FBF1 | parameter 2 |
| `0xFC` | FBF2 | parameter 3 |
| `0xFD` | FBF3 | parameter 4 |

Keyboard commands:

| Action | FDAT | FBUF | FBF1 | FBF2 | doorbell |
|---|---|---|---|---|---|
| master enable | `0x0C` | `0x3F` | – | – | `0xC4` |
| master disable | `0x0C` | `0x20` | – | – | `0xC4` |
| colour (zone 0) | `0x03` | **Blue** | **Red** | **Green** | `0xCA` |
| brightness 0–255 | `0x06` | level | – | – | `0xCA` |

Fan commands (fan 1 = CPU, fan 2 = GPU):

| Action | FDAT | FBUF | doorbell |
|---|---|---|---|
| set duty 0–255 | fan number | level | `0xC1` |
| back to auto | `0xFF` | fan number | `0xC1` |

Note the argument order **flips** between the two. Duty is read back straight
from EC RAM (`0xCE` = CPU, `0xCF` = GPU), and so is the fan speed — but the
tacho registers (`0xD0/0xD1`, `0xD2/0xD3`) hold a **16-bit big-endian period**,
not a speed: `rpm = 2156220 / period`. Temperatures sit at `0x07` (CPU) and
`0x0A` (GPU).

The firmware also has a fan *curve* table (function `0x0E`), which `g5fan`
deliberately does not use — see [Fans](#fans) for why.

Three gotchas that cost real debugging time (details in the research docs):

1. the EC **silently ignores** every LED command until it has received the
   master enable (`0xC4 / 0x0C / 0x3F`);
2. colour components go in **B, R, G** order — not R, G, B;
3. the fan duty and auto commands swap `FDAT`/`FBUF` relative to each other.

Everything is forgotten on reboot and suspend, hence the systemd hooks.

## Troubleshooting

- **No `/sys/class/leds/rgb:kbd`** — the kernel module isn't loaded. Check
  `dmesg | grep g5kbd` and `lsmod | grep g5kbd`; re-run `sudo ./install.sh`
  after a kernel update (module is built for the running kernel). The CLI
  still works without it via the EC path.
- **`/sys/kernel/debug/ec/ec0/io` is read-only / writes are refused** (EC
  fallback path only) — the `ec_sys` module was loaded without
  `write_support`. Fix for the session:
  ```bash
  sudo modprobe -r ec_sys && sudo modprobe ec_sys write_support=1
  ```
- **No `/sys/kernel/debug/ec` at all** — `ec_sys` isn't loaded, or your kernel
  lacks `CONFIG_ACPI_EC_DEBUGFS`. Check `ls /sys/module/ec_sys`; most distro
  kernels (including CachyOS) ship it as a module.
- **The light never changed in `g5kbd probe`** — stop: the EC protocol below is
  model-specific. The tool and driver refuse to run on non-Gigabyte G5/G6/G7
  hardware (`G5KBD_UNSAFE=1` / `force=1` override, at your own risk).
- **Fan control does not seem to do anything** — run `sudo g5fan doctor`
  first: it reports the machine, whether the kernel module and its attributes
  are present, whether `ec_sys` is usable, and what the EC says about each
  fan. Then `sudo g5fan doctor --write` writes a duty, reads it back and tells
  you plainly whether the EC took it. Note the driver puts its attributes on
  the ACPI device (`/sys/bus/acpi/devices/CLV0001:00/fan_{mode,duty}`); the
  platform device of the same name carries none.
- **`g5fan silent` / `maxq` / `custom` do nothing** — those are curves, and a
  curve is driven by the daemon, not written to the hardware once. Check
  `systemctl status g5fan-watchdog.service` and start it with
  `sudo systemctl start g5fan-watchdog.service`.
- **The GUI asks for a password when changing the fan mode** — that is
  polkit, and it is expected: the EC is root-only. The keyboard controls never
  ask. If the prompt does not appear, the policy did not install; check
  `/usr/share/polkit-1/actions/dev.g5kbd.fan.policy`.
- **`g5fan status` shows 0 RPM for the GPU fan** — a stopped fan reports a
  tacho period of 0, and the GPU fan genuinely is stopped while the dGPU is
  idle. A spinning fan always reports a period; if you doubt the number,
  believe your ears.

## Safety

The kernel driver only evaluates the firmware's own `_DSM` commands (the same
ones Windows' AcpiBridge executes) — no hand-rolled EC writes. The CLI
fallback writes only the keyboard-backlight and fan mailboxes. A reboot always
returns the EC to firmware defaults, so no experiment can leave the machine
stuck.

The fans get extra care: `auto` is the resting state and what the driver
restores on unload, every write is serialised with a `flock`, curves are
rejected unless they rise with temperature, and the watchdog service restores
the firmware curve on overheat. See [Fans → Safety](#fans) above and
[docs/FAN-RESEARCH.md](docs/FAN-RESEARCH.md) §6.

**Verified on hardware.** The fan commands were read straight off the
firmware rather than guessed, and the EC was then driven for real: a duty
write lands on the read-back, the fans change speed, `0x69` releases them
again, and the curve modes move the duty as the temperature moves.
`sudo g5fan doctor --write` re-checks the write path in one command, and
`sudo ./probe-fan.sh` walks the modes by ear. What is still open is listed in
[docs/FAN-RESEARCH.md](docs/FAN-RESEARCH.md) §8.

## Roadmap

Known gaps and the things worth doing next — the firmware questions nobody has
answered yet, moving the fan telemetry into the kernel driver so `ec_sys` and
root stop being needed for a *reading*, naming the fans after the hardware
that is actually present, and the packaging/CI traps — are written down in
[docs/ROADMAP.md](docs/ROADMAP.md), with priorities and a "done when" for
each.

## Credits / prior art

- **smairio/gigactl** — the first project to reverse-engineer and document the
  Clevo EC mailbox on Gigabyte G5/G6 laptops; its protocol claims were
  re-verified live on this G5 KC before this tool was written.
- **wessel-novacustom/clevo-keyboard** (TUXEDO fork of t-8ch's driver) — the
  `CLV0001` binding, `_DSM`-via-ACPI design and B·R·G byte order originate
  there; the kernel driver in this repo is modelled on it.
- **nbfc-linux** — `ec_probe` / ec_sys usage pattern.

## Repo layout

```
src/g5kbd.py                the CLI (LED-node or EC backend, effects engine)
src/g5fan.py                fan CLI: modes, curve, telemetry, profiles, watchdog
gui/                        native GPUI panel (one Rust binary)
gui/src/views/PerformanceView.tsx   the Fans page
kernel/g5kbd.c              ACPI driver: CLV0001 -> rgb:kbd + fan control
kernel/Makefile, dkms.conf  build + DKMS packaging
kernel/99-g5kbd.rules       udev rule (wheel group owns the LED node)
systemd/                    boot/suspend restore units, fan watchdog, modprobe
polkit/                     lets the GUI authenticate for fan changes
install.sh                  one-shot installer / uninstaller
tools/ec.py                 raw EC probe utility (dump/read/write mailbox)
probe-kb.sh                 guided hardware verification used during RE
probe-fan.sh                the same, for the fan protocol
docs/WINDOWS-RESEARCH.md    keyboard reverse-engineering write-up
docs/FAN-RESEARCH.md        fan reverse-engineering write-up
docs/ROADMAP.md             what is missing or unverified, with priorities
win/, reference/            extracted Windows modules + prior art
logs/, dsdt.dat             probe captures + ACPI DSDT from this machine
```
