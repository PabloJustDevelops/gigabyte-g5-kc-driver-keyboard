# Roadmap — what is worth doing next, and why

Written down so it does not have to be rediscovered. Everything here is
*known to be missing or unverified*, not speculation: each item says how to
tell it is done.

Priorities: **P0** = a real bug or a hole in the safety story, **P1** = the
thing that removes a whole class of friction, **P2** = polish.

---

## P0 — correctness and safety holes

### 1. A pinned duty is silently lost after suspend

The EC forgets its fan configuration across suspend and reboot (this is why
the keyboard has restore units). The daemon re-drives *curves* within one
tick, but `turbo` and `manual` are handed straight to the EC by the CLI and
the daemon deliberately leaves direct modes alone — so this happens:

```
g5fan turbo        # fans pinned to 100 %
systemctl suspend  # ...and resume
g5fan status       # says "turbo", the EC is back on its own curve
```

Fix, in order of robustness:

* self-healing, no hooks: while the mode is `turbo`/`manual`, the daemon
  compares the EC's duty with the commanded one and re-writes it *only* after
  it has been wrong for two consecutive ticks (the duty read-back lags a write
  by more than one sysfs call — see FAN-RESEARCH §3.1, so a single-tick
  mismatch means nothing);
* or a `system-sleep` hook that re-applies the state file on resume, plus
  `g5fan restore` for the boot path.

Done when: `turbo` → suspend → resume leaves the fan at 100 %, and
`g5fan manual 30` survives a reboot.

### 2. The daemon watching the daemon

`Restart=always` brings `g5fan` back after a crash, and the EC's own firmware
protection is always there, but a daemon that *hangs* (EC read blocking in
debugfs, a wedged `_DSM` call) is invisible: the panel would show stale
telemetry and nobody would know the curve stopped being driven.

* `WatchdogSec=` in the unit + `sd_notify` heartbeats from `g5fan supervise`;
* a check of the snapshot's age in the GUI is already there (it says "the fan
  daemon is not running") — a stuck daemon needs the same treatment.

Done when: `kill -STOP` on the daemon leads to a restart and an explanatory
line in the journal.

---

## P1 — the big wins

### 3. Put the telemetry in the driver (the one that removes `ec_sys`)

Today the driver exposes writes only (`fan_mode`, `fan_duty`) and every
reading — duty, tacho, temperatures — goes through `/dev`-less debugfs
(`ec_sys`, `write_support=1`). That has three costs:

* the CLI needs root to *read* (the panel avoids it only because the daemon
  republishes what it saw — a workaround, not a fix);
* `ec_sys` is a debugging interface: it is a raw window into EC RAM, and
  loading it with write support is a bigger power than any of this needs;
* on a system without `ec_sys` there are no temperatures at all.

The intended end state is a proper hwmon device on the ACPI device:

```
/sys/class/hwmon/hwmonN/  name=g5kbd
    fan1_input   fan2_input           # rpm   (tacho period -> 2156220 / period)
    pwm1         pwm2                 # 0-255, the duty actually applied
    temp1_input  temp2_input          # CPU, GPU (EC 0x07, 0x0A)
    temp1_max                         # the ceiling the daemon uses
```

with `devm_hwmon_device_register_with_info()` and the register reads already
proven by `g5fan doctor`. Bonus: `sensors` shows the fans, and `fancontrol`
users stop being surprised. Keep the LED side as it is.

Watch out for the duty read-back latency (FAN-RESEARCH §3.1): the register
lags the write, so `pwm*` must be presented as "what the EC says", never as
an echo of the last write.

Done when: reading duty/RPM/temperature needs no root and no `ec_sys`, and
`g5fan status` says `backend: hwmon`.

### 4. Name the fans after the hardware that is actually there

`fan 1 = CPU`, `fan 2 = GPU` is an assumption from this chassis. It is worth
turning into a checked fact, because the failure mode is silent and nasty:
driving a fan that is not there, or believing a GPU temperature that comes
from a powered-down dGPU.

* GPU temperature: fall back across `amdgpu` / `nouveau` / `nvidia` (already
  done in `hwmon_temp_c`) *and* handle "the dGPU is off" (PRIME / runtime
  power management) — the GPU temp then reads 0 or nothing, and the daemon
  should say so rather than hold the GPU fan at the curve floor;
* CPU temperature: `cpu_ceiling_c()` only looks at `coretemp`; AMD Clevo
  variants report through `k10temp`/`zenpower`, where the ceiling currently
  falls back to 90 °C;
* labels: derive `CPU`/`GPU` from what answered (EC register present? hwmon
  chip present?) instead of the current fixed `FAN_LABELS`;
* if only one fan responds, do not write the pair blindly — the firmware
  command assigns all four, so a single-fan chassis needs the read-modify-
  write path that was removed for being unreliable (see FAN-RESEARCH §3.1).

Done when: `g5fan doctor` on a machine with the dGPU off reports the GPU
temperature as unavailable instead of 0 °C, and the labels match reality.

### 5. What the firmware's own curve does (still unconfirmed)

Three open questions from FAN-RESEARCH §8, in the order that would help:

1. does the EC's curve react to the GPU temperature as well as the CPU's?
   If not, the daemon's per-fan GPU point is redundant, and if it does, the
   daemon is fighting a curve that already does the right thing at `auto`;
2. does the firmware honour its own settable curve table at all (function
   `0x0E`)? Nobody has confirmed an effect, and two of its four points are
   not even writable from the OS;
3. what the stock Gigabyte Silent/MaxQ curves are.

Method: `sudo ./probe-fan.sh` walks the modes by ear; a programmed load
(stress-ng on CPU only vs CPU+GPU) plus duty/RPM sampling at `auto` answers
(1) in one session.

### 6. Profiles and per-scenario automation

`g5fan profile` exists but only on the CLI. The obvious next steps:

* profiles in the panel (save the current mode/curve, apply it, delete it);
* bind a profile to AC vs battery (`udev` on the `power_supply` change), and
  to a game via a desktop entry — which is what the Windows app's "mode" is
  really for and what people actually ask for.

---

## P2 — polish and coverage

### 7. GUI

* **Curve graph.** The fan curve is currently five pairs of sliders. A small
  SVG chart of the ramp (with the live temperature marked on it) would say
  more than the sliders do.
* **Effects parity.** Drop-in `breathe`/`cycle` config is done, but the
  firmware takes one colour for the whole keyboard, so there is no per-zone
  editor to build, and the preview does not yet animate at the effect's
  speed.
* **Cancelled pkexec.** A dismissed password dialog surfaces as the raw
  message "fan control needs authentication (pkexec was cancelled or is not
  installed)". That deserves a real, clickable "authenticate" affordance.
* **A tray/indicator applet** showing mode + temperature, since the panel is
  not always open.
* **The GPUI panel has never been through a visual test.** Porting the UI from
  the Tauri/webview build to native GPUI removed the browser and the Node
  toolchain, but the window is still build- and unit-verified only: nobody has
  clicked through it on a real screen. Everything below the panel (CLIs,
  daemon, driver) is exercised on hardware as before.
* **Curve editing is fixed at five points.** `g5fan curve set` takes 2..5 and
  the old web panel could add and remove points; the GPUI editor always shows
  five. The CLI remains the way to set a shorter curve.

### 8. Packaging and CI

* **`PKGBUILD`'s `source` array is maintained by hand** (every panel source
  file, the docs, each systemd unit). Adding a file and forgetting the array
  is a silent packaging bug — the `SKIP` sums hide it. A CI check that every
  file listed under `gui/src`/`docs`/`systemd` appears in `source` (and vice
  versa) would end it.
* **The CI kernel has `CONFIG_LEDS_CLASS_MULTICOLOR` disabled**, so the
  kernel job builds with `KBUILD_MODPOST_WARN=1` and can never link. Build
  against a real kernel config (container or an Arch image) to get a green
  modpost.
* **Nothing drives the panel's own code.** The rewrite into Rust did bring
  unit tests (`gui/src/backend/fan.rs` pins the `g5fan status --json` field
  contract and mirrors the CLI's curve validation; `theme.rs` covers hex
  parsing), and `cargo clippy -D warnings` now watches the panel. What is
  still missing is anything that renders a view and asserts it: GPUI ships a
  test harness (`gpui_kit::test`), and a test that mounts `Panel` and checks
  the four views build would catch the next wiring mistake.
* **No AUR package and no GitHub release automation for the CLI alone**;
  `release.yml` builds the `.deb` + binary on a `v*` tag.
* `install.sh` used to leave the docs behind (only the PKGBUILD installed
  them, and to a path the systemd unit did not point at); both now install
  them to `/usr/share/doc/g5kbd/`.

### 9. More hardware than this one laptop

Everything here was read off a single G5 KC (BIOS FB08). The Clevo family
shares the keyboard EC protocol, but fan control is exactly the sort of thing
that varies by ODM build:

* G5 GD/GE, G7 KC, and the same chassis under other brand names;
* the ITE devices (`vid_048d&pid_8910`, `pid_8297`) that other Clevo units
  expose and this one does not;
* firmware revisions: the model guard (`G5FAN_UNSAFE=1` to override) is
  crude, and a wrong model is refused rather than handled.

What it takes: the same approach as `probe-kb.sh`/`probe-fan.sh` — a script
that captures the DSDT, the EC map and the effects of each mode, and a
`docs/HARDWARE.md` table of what is confirmed per model.

### 10. Small leftovers

* the second two-part firmware op (`0xF6`, `FDAT` 0x09+0x0A) seen next to the
  LED command in the DSDT is still unidentified;
* the keyboard LED interface only exposes zone 0 of the firmware's four;
* the effect daemon's CPU cost is unmeasured (a 5 Hz userspace effect loop is
  nothing, but it has never been checked against a busy scheduler).
