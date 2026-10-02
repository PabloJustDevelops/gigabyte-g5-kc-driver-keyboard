# Gigabyte G5 KC fan control — how Windows does it, and what Linux can do

Reverse-engineering notes for `g5fan`. Machine under test: **Gigabyte G5 KC**
(DMI: `GIGABYTE` / `G5 KC`, Insyde BIOS `FB08`), CachyOS, kernel 7.2.
Investigation date: 2026-09-28. Companion to
[WINDOWS-RESEARCH.md](WINDOWS-RESEARCH.md), which covers the keyboard.

## TL;DR

The fans are driven by the **same ACPI EC mailbox as the keyboard backlight**,
through the same `CLV0001` (`\_SB_.DCHU`) `_DSM` dispatcher — and unlike the
keyboard, the *firmware itself documents the commands in the DSDT*, so nothing
had to be guessed. Three things fall out:

| Capability | How | Status |
|---|---|---|
| fixed duty per fan | `_DSM` `0x68` | decoded from the AML; the EC's read-back and tacho confirm the telemetry side |
| hand fans back to the firmware curve | `_DSM` `0x69` | decoded from the AML |
| read duty, tacho and temperatures | EC RAM `0xCE`–`0xD3`, `0x07`, `0x0A` | read off this machine, values check out |
| edit the fan curve | `_DSM` `0x0E` (raw buffer) | decoded from the AML, **deliberately not used** — see §3.4 |

The Windows app has five modes, but this firmware only really offers the two
direct commands at the top of that table. There is no mode register that
selects a built-in quiet curve, so **`g5fan` builds its curves in userspace**
and drives the duty itself — see §5 for why that is the honest option rather
than a shortcut.

## 1. The Windows module

Same Control Center 3.0 package (v3.55) as the keyboard, but a different UWP
module. Extracted to `win/fan/`:

```
Program Files (x86)/ControlCenter/AppInstall/FanSpeedSetting/
  FanSpeedSetting.appxbundle          -> bundle/
    WapProjFanSpeedSetting_3.52.1.0_x64.appx   -> inner/
      FanSpeedSetting/FanSpeedSetting.exe      the UI (WPF, .NET 4.6.1)
      FanSpeedSetting/InsydeDCHU.dll           the EC access library
      FanSpeedSetting/mtlanguage.ini
      FanSpeedSetting/oem.ini
```

It talks to the EC through the **same** `InsydeDCHU.dll` API the keyboard
module uses (`SetDCHU_Data`, `SetDCHU_DataEx`, `GetDCHU_Data_Integer`,
`GetDCHU_Data_Buffer`) — i.e. `_DSM` calls against `CLV0001`, not a private
USB device. Its resource strings confirm the same story: `SetWMI CMD=`,
`GetWMI CMD=`, `Add_FocusFanTableEvent`, `AntiDust_Fan`.

## 2. The mode list (from the app's own `oem.ini`)

```ini
[ControlCenter]
SupportXTUFanTable=1
FanMode=Default
;0:Auto 1:Max 3:Silent 5:MAXQ 6:Custom
SituationalMode=3
;0:Quiet 1:power saving 2:performance 3:Entertainment
SupportFanSpeedOffset=1
```

and from the UI's own control names:

```
RB_FAN_turbo_Click     RB_FAN_Maxq_Click     RB_FAN_Silent_Click
TurboFanStatus         SupportMaxQ_FAN       SupportFanSpeedOffset
```

So the panel is **Auto · Turbo ("Max") · Silent · MaxQ · Custom** — the five
`g5fan` modes, with `1:Max` shown to the user as "Turbo".

## 3. The command set, out of the DSDT

`dsdt.dat` (this machine) disassembles with `iasl -d`. The dispatcher is
`\_SB_.DCHU._DSM`, and it routes by `Arg2` (the function id) through six
helpers — `CPKG`, `OCWR`, `CC30`, `GCMD`, `SCMD`. The keyboard command
`0x67` goes through `SCMD`; **so do both fan commands**:

### 3.1 Set duty — function `0x68`

```asl
If ((ToInteger (Arg1) == 0x68))
{
    Local4 = ARGS
    EC.FDAT = 0x01;  EC.FBUF = (Local4 & 0xFF);        EC.FCMD = 0xC1
    EC.FDAT = 0x02;  EC.FBUF = ((Local4 >> 0x08) & 0xFF); EC.FCMD = 0xC1
    EC.FDAT = 0x03;  EC.FBUF = ((Local4 >> 0x10) & 0xFF); EC.FCMD = 0xC1
    EC.FDAT = 0x04;  EC.FBUF = ((Local4 >> 0x18) & 0xFF); EC.FCMD = 0xC1
}
```

The 32-bit argument is four duty bytes, one per fan. In mailbox terms:

```
FDAT = fan number (1..4)      FBUF = duty 0..255      FCMD = 0xC1   (doorbell, last)
```

**Fan 1 is the CPU, fan 2 is the GPU** — the AML addresses four fans, but this
chassis has two (the read-back registers line up: `DUT1`/`RPM1` and `DUT2`/`RPM2`).

#### There is no "set one fan"

All four of those mailbox writes happen on **every** call, whatever the
argument's high bytes say. `0x68` with `0x00000099` sets fan 1 to `0x99` and
fans 2, 3 and 4 **to zero**. That makes the obvious implementation — pack one
byte, send it — a fan-stopper: the first version of `g5fan` wrote the CPU fan
and dropped the GPU fan to 0 %, which is exactly what
`g5fan doctor --write` reports ("fan 1: 35 % -> 0 %  moved, but not to the
target").

The read-modify-write rescue — read `0xCE`/`0xCF` with `ec_read()`, overlay the
byte being changed, send the whole word — looks like it fixes this, and it does
not. **The EC's duty read-back lags the mailbox write by more than the gap
between two sysfs writes**, so the second write reads the *old* duty back and
stamps it over the first. That is how `g5fan doctor --write` reported
`fan 1 (CPU): 35% -> 35%  IGNORED (still the firmware value)` while fan 2 took
its new value: the fan-2 write had read fan 1's pre-write duty and sent it
straight back.

So the interface does not pretend a single fan can be named. The driver
exposes **one** attribute:

```
/sys/bus/acpi/devices/CLV0001:00/fan_duty     read: "89 89"   write: "153 153"
```

Both duties, in CPU-then-GPU order, because that is the shape of the command.
Nothing is read on the write path at all, so there is no window for a stale
value to sneak in, and every caller (turbo, manual, the curve daemon) computes
both duties anyway. `fan_duty` *reads* from the EC, so it reports what the
fans are really doing — and being world-readable, it keeps duty visible on a
system where `ec_sys` is not loaded.

The raw-EC path needs none of this: its mailbox write addresses a single fan,
because it *is* one of those four writes. `g5fan` sends both anyway, so the
two paths behave identically.

### 3.2 Back to automatic — function `0x69`

```asl
If ((ToInteger (Arg1) == 0x69))
{
    If (Local4 & 0x01) { EC.FDAT = 0xFF; EC.FBUF = 0x01; EC.FCMD = 0xC1 }
    If (Local4 & 0x02) { EC.FDAT = 0xFF; EC.FBUF = 0x02; EC.FCMD = 0xC1 }
    If (Local4 & 0x04) { EC.FDAT = 0xFF; EC.FBUF = 0x03; EC.FCMD = 0xC1 }
    If (Local4 & 0x08) { EC.FDAT = 0xFF; EC.FBUF = 0x04; EC.FCMD = 0xC1 }
}
```

A bitmask, one bit per fan, and note the **argument order flips**: the
auto command puts `0xFF` in `FDAT` and the *fan number* in `FBUF`, whereas
the duty command puts the fan number in `FDAT`. Easy to get backwards.

### 3.3 Telemetry (no mailbox needed — just read EC RAM)

From the `\_SB_.PCI0.LPCB.EC` field declarations. The `RAM` operation region
(`SystemMemory`, `0xFE700100`) maps onto the EC's own 256-byte window at
offset 0, which `DUT1` at `0xCE` pins down — the keyboard colours sit at
`0x80` and read back as the blue default, which agrees.

| Register | Offset | Meaning |
|---|---|---|
| `DUT1` | `0xCE` | commanded duty, fan 1 (CPU) |
| `DUT2` | `0xCF` | commanded duty, fan 2 (GPU) |
| `RPM1` | `0xD0` | fan 1 **tacho period**, 16-bit |
| `RPM2` | `0xD2` | fan 2 **tacho period**, 16-bit |
| `RPM4` | `0xD4` | fan 4 tacho, 16-bit (no such fan here) |
| `RPM3` | `0xE0` | fan 3 tacho, 16-bit (no such fan here) |
| — | `0x07` | CPU temperature, °C |
| — | `0x0A` | GPU temperature, °C |

The name `RPM1` is a misnomer. The register holds a **period**, not a speed,
and it is stored **big-endian** — which is why reading it as the little-endian
16-bit value the ACPI word field suggests gives nonsense (the live CPU fan
reads `03 CC`, i.e. 52227 little-endian and 972 big-endian). The Clevo formula
turns the period into a speed:

```
rpm = 2156220 / period        period 0 or >= 0xFF00  ->  fan stopped
```

So the CPU fan above is `2156220 / 972 ≈ 2218 rpm`. This chassis exposes no
`fan*_input` in hwmon, so this is the only place fan speed exists at all, and
`g5fan` reads it through `ec_sys`.

### 3.4 The fan curve — function `0x0E`, and why `g5fan` does not use it

There *is* a settable curve. It lives in a second EC window
(`OperationRegion RAM3, SystemMemory, 0xFE700300`, i.e. outside the 256 bytes
`ec_sys` exposes) and the Windows app mirrors it in `Custom.ini` as
`Fan_CPU.T1..D4` / `Fan_CPU.D*_Default`, which is where its `Fan_CPU.T1=`
strings come from.

The **write** path is `CC30` case `0x0E` → method `PK0E`:

```asl
Method (PK0E, 3, NotSerialized)
{
    CreateByteField (BUFF, 0x02, W002)   /* F1T2 */
    CreateByteField (BUFF, 0x03, W003)   /* F1D2 */
    CreateByteField (BUFF, 0x04, W004)   /* F1T3 */
    CreateByteField (BUFF, 0x05, W005)   /* F1D3 */
    CreateByteField (BUFF, 0x06, W006)   /* F2T2 */   ... through 0x0d
    CreateWordField (BUFF, 0x0E, W101)   /* F1R1 */   ... through 0x1f
    BUFF = DerefOf (Arg2 [Zero])
    If (ECOK) { EC.F1T2 = W002; EC.F1D2 = W003; /* ... */ }
}
```

Three things fall out of this, and together they are why `g5fan` drives the
duty instead:

1. **Only the middle two points are settable.** `PK0E` writes `T2/D2` and
   `T3/D3`. `T1/D1` and `T4/D4` are firmware-owned and unreachable from the
   OS, even for the Windows app — so a "4-point curve editor" is a lie on this
   platform regardless of who is asking.
2. **The RPM set-points cannot be read back.** `PK0E` writes `F1R1..F3R3`
   (its three 16-bit RPM set-point words per fan) out of the same buffer, but
the matching read command (`PK0D` / function `0x0D`) fills in the T/D points
and stops before them. Writing the table therefore means clobbering three
values per fan that nothing on this side can see, on the strength of a guess.
3. **Whether this EC acts on the table at all is unverified.** Nothing in the
   AML selects a mode, and no experiment has shown the fans following a
   written curve.

Against that, writing the duty directly:

* is one integer, on the same code path Windows' own fan slider uses;
* has the EC's own read-back (`0xCE`) to confirm it landed;
* allows a full five-point curve instead of a two-point nudge;
* keeps the firmware's table — including its RPM set-points — untouched, so
   `auto` still means exactly what it says.

So `g5fan`'s curves are evaluated by the `g5fan` daemon (`g5fan supervise`,
the process behind `g5fan-watchdog.service`) and applied as duty. The kernel
driver consequently exposes only `fan_mode` and `fan{1,2}_duty`; it has no
curve attribute at all.

## 4. What the two implementations do

| | Windows | `g5fan` |
|---|---|---|
| duty | `_DSM 0x68` | `_DSM 0x68` via the driver, or the same mailbox write directly via `ec_sys` |
| auto | `_DSM 0x69` | same |
| duty / tacho / temp read | its own WMI method | EC RAM `0xCE`–`0xD3`, `0x07`, `0x0A` via `ec_sys` |
| curve | `_DSM 0x0E` (2 of 4 points, RPM set-points guessed) | evaluated in userspace, applied as duty |
| modes | one UI, one enum | five CLI verbs / five GUI buttons |
| thermal safety | the EC's own firmware curve | the daemon's ceiling + the EC's own curve |

Both routes end in the same EC writes, which is the same conclusion the
keyboard work reached: the `CLV0001` `_DSM` is the interface, and the raw
mailbox is a perfectly good fallback for everything that fits in a byte.

### 4.1 Who is allowed to look at the fans

Changing a fan needs the EC, and the EC is root-only, so `g5fan` elevates
(sudo from a terminal, polkit from the panel). *Reading* one should not need
anything, but the only place duty, tacho and temperature live is the same EC —
permanently handing that window to the desktop user would be a much bigger
power than the feature deserves.

So the one process that already has the EC (the daemon) republishes what it
sees, atomically, to `/run/g5fan/status.json` (0644) on every tick, and
`g5fan status --cached` reports that instead of the hardware. The file is what
makes the panel's numbers unprivileged:

* it is telemetry, not state, so it lives in `/run` (systemd
  `RuntimeDirectory=g5fan`) and never survives a reboot — a stale description
  of a machine that no longer exists would be worse than none;
* `age_s`/`stale` come from its timestamp, which is how a reader tells a live
  daemon from one that died: past `SNAPSHOT_MAX_AGE` the text form refuses to
  print rather than dress up an old reading as current (the JSON form still
  emits it, flagged, for a caller that can render "last known" honestly);
* `g5fan status --json` describes the same reading live (as root) with the
  same key set, so the panel has one parser and the CLI one description.

Only `status` reads this way; every path that writes still goes through the
EC under an `flock`.

Moving the telemetry into the driver instead (a real hwmon device, no `ec_sys`
at all) is the next step and is written up in [ROADMAP.md](ROADMAP.md) §3.

## 5. The modes

| `g5fan` mode | Windows id | What it does here |
|---|---|---|
| `auto` | 0 | `0x69` with both bits — the firmware's own curve |
| `turbo` | 1 | duty 255 on both fans |
| `manual N` | — | pin the duty of both fans |
| `silent` | 3 | a quiet curve, evaluated by the daemon |
| `maxq` | 5 | the quietest curve, evaluated by the daemon |
| `custom` | 6 | the user's curve, evaluated by the daemon |

The first three are direct: one write, and the EC holds that duty. The last
three are curves, and the distinction is not cosmetic — it is forced by the
firmware.

**There is no mode register anywhere in this DSDT.** The dispatchers contain no
branch that takes a 0/1/3/5/6 selector and hands the fans to a built-in quiet
curve. The candidates were

- the EC has built-in curves selectable by a register this AML never writes —
  and the Windows DLL pokes raw EC memory directly through `_DSM` `0x75`, as
  does `0x79` sub-command `0x0E` (which writes `GFOF`). Neither target register
  is identifiable from static analysis alone;
- the Control Center implements Silent/MaxQ itself, as curves.

The second is at least as likely, and it is the only option a
correct-by-construction implementation can offer — so that is what `g5fan`
does, with the same five-point shape and hysteresis the daemon needs to be
usable:

| preset | curve (T:D) |
|---|---|
| `silent` | `45:15 60:25 72:40 82:65 92:100` |
| `maxq` | `48:12 62:20 75:35 85:60 92:100` |
| `custom` | `50:25 65:40 75:60 85:80 95:100` |

They are presets, not the stock Gigabyte curves — the originals are not
recoverable from this firmware's AML. `g5fan` never pretends otherwise: the
presets are named as ours, `g5fan curve` prints them, and the daemon reports
every duty change it makes.

If you want to hunt for the real thing anyway, `probe-fan.sh` is the harness:
run it with the machine idle, watch the fan behaviour for each mode, and if a
register moves, `tools/ec.py dump 0 0x100` before and after will catch it.
`_DSM` `0x6B` + `0x75` is the raw memory poke (`0x6B` sets `INDX` to the
address and size, `0x75` writes the value) if you ever need to drive one by
hand.

## 6. Safety

Fan control is the one feature in this repo that can hurt you if it is wrong,
so the design is deliberately conservative:

- **`auto` is the default and the resting state.** The driver's `remove`
  callback hands the fans back to the firmware curve, so unloading the module
  cannot leave a duty pinned, and the daemon releases the fans on `SIGTERM`.
- **Every write takes an `flock`** (`/run/lock/g5fan-ec.lock`), so two writers
  cannot interleave a duty write with an auto write.
- **Both duties, always.** `fan_duty` takes the pair, so there is no code
  path that can command a fan to 0 because the other one was set — the
  failure mode the first implementation had.
- **The curve is validated with the same rules** in the CLI, the GUI and the
  `_DSM` write path: temperatures must rise and duty must never fall. A curve
  that dips is refused rather than applied.
- **The daemon enforces a ceiling.** `g5fan supervise` reads the CPU
  temperature every few seconds and hands the fans to `auto` above the CPU's
  *own* `temp1_max` from hwmon (90 °C if hwmon has none), not a guessed
  number. A 30 % floor applies from 85 °C up, so no curve can idle a hot fan.
- **The EC's firmware thermal protection runs regardless** of anything here.
  Even a total failure of this software cannot overheat the machine, because
  the watchdog that matters is in the EC, not in Linux.
- `install.sh --uninstall` puts the fans back on `auto` before removing the
  module.

## 7. Files captured during the investigation

- `win/fan/` — the extracted `FanSpeedSetting.appxbundle`, the string tables
  and `oem.ini` the mode list came from.
- `dsdt.dat` — this machine's DSDT; `iasl -d dsdt.dat` reproduces everything
  above. The relevant methods are `\_SB_.DCHU.SCMD` (functions `0x68`, `0x69`),
  `\_SB_.DCHU.CC30` → `PK0E` (function `0x0E`), and the `\_SB.WMI.PK0C/0D/0E`
  mirror set that returns the table.
- `probe-fan.sh` — the human-verified walk-through, and `g5fan doctor`
  (`src/g5fan.py`) for the machine-checkable version of the same thing.
- `logs/dsdt/dsdt.dsl` — the disassembled DSDT actually used above (the raw
  `dsdt.dat` is kept alongside it).
- The two implementations: `src/g5fan.py` (CLI, curve engine, daemon) and
  `kernel/g5kbd.c` (the `fan_mode` / `fan{1,2}_duty` attributes).

## 8. What is confirmed, and what is not

Read off this machine's own firmware (§3), and now also read off its EC:

- the mailbox registers, the two doorbells (`0xC1`) and their argument order;
- the telemetry: `DUT1`/`DUT2` at `0xCE`/`0xCF`, the tacho periods at
  `0xD0`–`0xD3`, the temperatures at `0x07`/`0x0A`;
- the RPM formula — a live CPU fan reads a period of 972, i.e. ~2218 rpm,
  which is what a fan at a 35 % duty should be doing.

Confirmed on hardware since (see `g5fan doctor --write`):

- **the EC acts on a duty write** — a 60 % command lands on the EC's read-back
  as 60 %, and `0x69` releases the fan again;
- the fan actually speeds up: the tacho period moves with the duty.

Still unconfirmed:

1. whether the firmware's own curve table is honoured at all — moot while
   `g5fan` does not write it, but it is what would let a curve survive without
   the daemon;
2. what the stock Gigabyte Silent/MaxQ curves actually are;
3. whether the EC's firmware curve reacts to the *GPU* temperature as well as
   the CPU's, which decides whether the daemon's GPU point is redundant.

To check the rest, `sudo ./probe-fan.sh` walks the modes by ear;
`--duty-only` skips the curve section.

Everything that was already known to be missing — the firmware questions
above, the pinned-duty-after-suspend hole, the telemetry-in-the-driver work,
and naming the fans after the hardware that is really there — is collected in
[ROADMAP.md](ROADMAP.md) with priorities and a "done when" for each.
