# g5kbd — Gigabyte G5 keyboard backlight for Linux

Colour / brightness / effects for the single-zone RGB keyboard of the
**Gigabyte G5** (Clevo-ODM) gaming laptop on Linux, delivered as three layers:

```
┌──────────────────────────────────────────────────────────┐
│ GUI  g5kbd-gui  (Tauri v2: web UI + Rust core, no root)  │
├──────────────────────────────────────────────────────────┤
│ CLI  g5kbd  (Python — the same commands, no GUI needed)  │
├──────────────────────────────────────────────────────────┤
│ Standard API: /sys/class/leds/rgb:kbd  (LED multicolor)  │
├──────────────────────────────────────────────────────────┤
│ Kernel driver  g5kbd.ko  (ACPI platform driver on        │
│ CLV0001 -> evaluates the firmware _DSM, cmd 0x67)        │
└──────────────────────────────────────────────────────────┘
```

Verified on a **Gigabyte G5 KC** (Insyde BIOS FB08, CachyOS / kernel 7.2)
on 2026-09-06: red/green/blue/white, brightness steps and on/off all confirmed
working; host-driven `breathe`/`cycle` effects confirmed on screen.

> These laptops ship no Linux software for the backlight. Windows controls it
> with Clevo's Control Center (which is why, in Linux, the light stays stuck on
> the firmware default: blue, full brightness — see
> [docs/WINDOWS-RESEARCH.md](docs/WINDOWS-RESEARCH.md) for the full story of
> what Windows actually uses and how the EC protocol was found).

## Install

```bash
sudo ./install.sh              # everything: kernel module, CLI, GUI
sudo ./install.sh --no-gui     # CLI + kernel module only
sudo ./install.sh --uninstall  # remove all of it
```

What it installs:

| Path | Purpose |
|---|---|
| `/usr/lib/modules/<kern>/extra/g5kbd.ko` | kernel driver: ACPI device `CLV0001` → LED device `rgb:kbd` |
| `/sys/class/leds/rgb:kbd/{brightness,red,green,blue,color}` | the standard LED API (once the module is loaded) |
| `/etc/udev/rules.d/99-g5kbd.rules` | let the `wheel` group write the LED — no root needed |
| `/usr/bin/g5kbd` | CLI (falls back to the raw-EC path when the module isn't loaded) |
| `/usr/bin/g5kbd-gui` | Tauri v2 GUI (skipped if `npm`/`cargo` are missing) |
| `/usr/lib/systemd/system/g5kbd.service` + `system-sleep/g5kbd` | restore the saved colour at boot and after suspend |
| `/var/lib/g5kbd/state.json` | your saved state |

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
speed. Built with **Tauri v2** (Rust core) and a **React + TypeScript +
Tailwind + shadcn/ui** frontend; every change goes through the `g5kbd` CLI,
so the GUI never touches the hardware itself and needs no privileges — the
udev rule grants the LED node to the `wheel` group. The kernel node being
present also means KDE/GNOME keyboard-brightness controls and tools like
`brightnessctl` work for the brightness part.

To hack on the UI alone (no driver needed): `cd gui && npm run dev` and open
http://localhost:5173 — the panel falls back to an in-memory mock when it
isn't running inside Tauri. Full app in dev mode with hot reload:
`npm run tauri dev`.

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

Two gotchas that cost real debugging time (details in the research doc):

1. the EC **silently ignores** every LED command until it has received the
   master enable (`0xC4 / 0x0C / 0x3F`);
2. colour components go in **B, R, G** order — not R, G, B.

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

## Safety

The kernel driver only evaluates the firmware's own `_DSM` LED commands (the
same ones Windows' AcpiBridge executes) — no hand-rolled EC writes. The CLI
fallback writes only the keyboard-backlight mailbox. A reboot always returns
the EC to firmware defaults, so no experiment can leave the machine stuck.

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
gui/                        Tauri v2 GUI (web frontend + Rust core)
kernel/g5kbd.c              ACPI driver: CLV0001 -> led-class-multicolor rgb:kbd
kernel/Makefile, dkms.conf  build + DKMS packaging
kernel/99-g5kbd.rules       udev rule (wheel group owns the LED node)
systemd/                    boot/suspend restore units + module config
install.sh                  one-shot installer / uninstaller
tools/ec.py                 raw EC probe utility (dump/read/write mailbox)
probe-kb.sh                 guided hardware verification used during RE
docs/WINDOWS-RESEARCH.md    full reverse-engineering write-up
logs/, dsdt.dat             probe captures + ACPI DSDT from this machine
```
