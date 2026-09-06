# Gigabyte G5 KC keyboard backlight — how Windows does it, and how Linux can

Reverse-engineering notes for `g5kbd`. Machine under test: **Gigabyte G5 KC**
(DMI: `GIGABYTE` / `G5 KC`, Insyde BIOS `FB08`), CachyOS, kernel 7.2.
Investigation date: 2026-09-06.

## TL;DR

The G5 KC is a **Clevo ODM design wearing Gigabyte branding**. Windows drives
its keyboard LEDs through the **EC (embedded controller)**, via Clevo's
Control Center stack. There is **no USB RGB chip** on this machine (no
ITE/OpenRGB device), so nothing USB-side needs a driver. On Linux the same EC
is reachable through the kernel's `acpi_ec`/`ec_sys` interface, and the
Clevo-family mailbox protocol was verified live — colour, brightness and on/off
all work (details below).

## 1. The Windows driver stack (from the mounted Windows partition)

Location on disk: `C:\Program Files (x86)\ControlCenter` (classic, "Package
v3.55") plus the UWP app `CLEVOCO.ControlCenter3.0` in `WindowsApps`. oem.ini
identifies it as Clevo's **Control Center 3.0 package v3.55**.

| Component | What it is | Evidence |
|---|---|---|
| `AcpiBridge.sys` + `AcpiBridge.inf` | Insyde KMDF (WDF 1.15) kernel driver | binds `ACPI\CLV0001`; exports device interface `{86994c74-ad43-4812-b7e7-0c420b5c5fd7}`; service name `AcpiBridge`, `SERVICE_DEMAND_START` |
| `ACPI0002.inf` / `AcpiBridge1.sys` | same family, second device | binds `ACPI\CLV0002`; installs the broker exe below |
| `DCHUService.exe` | "Control Center Hotkey Service" (`CCDCHUService`, auto-start) | the user-mode broker the UWP apps talk to |
| `InsydeDCHU.dll` | Insyde H2O DCHU API | BIOS/EC access library (also used by the keyboard app) |
| `CC3.0.appxbundle` | Clevo Control Center 3.0 UI | `ControlCenter30.exe` (WindowsApps) |
| module **Keyboard** (`Keyboard.appxbundle` → `KB_Perkey_3.32.0.0`) | LED control UI | `LedKeyboardSetting.exe`, `perkey_api.dll`, `LineLEDAPI.dll`, `DataAddress.dll`, `Windowshook.dll` |

`CLV0001` and `CLV0002` are Clevo's ACPI "bridge" devices that expose the EC to
the OS. Both are present in this laptop's ACPI namespace and are visible in
Linux as `CLV0001:00` / `CLV0002:00` under `/sys/bus/acpi/devices` — unclaimed,
waiting for a driver that never comes on Linux.

## 2. Why this machine has no ITE/OpenRGB-style USB chip

`LedKeyboardSetting.exe` is a **multi-model app** shipped by Clevo for an
entire laptop family. Its string table references:

- `perkey_api.dll` → USB HID device `vid_048d&pid_8910` (VID 048d = **ITE Tech**),
  using `HidD_GetFeature`/`HidD_SetFeature` (feature reports), SetupAPI device
  enumeration, per-key effects (wave / snake / ripple / scan / breathe / static
  per-key…).
- `LineLEDAPI.dll` → USB HID device `vid_048d&pid_8297` (**ITE 8297**), the
  chip OpenRGB supports, with debug strings (`PID=%d, Read PID=%d`,
  `UsapagePage=%d myUsage=%d`).

…and also `ReadAcpiScanColor*`/`WriteAcpiScanColor*` helpers and
`DataAddress.dll` (a per-model EC-address table) — the ACPI/EC path used by
models whose keyboard is driven by the EC itself.

Evidence that **this** unit is the EC kind, not the ITE kind:

1. `lsusb` on Linux shows no 048d device (only the camera, WLAN/BT, hubs).
2. The Windows `SYSTEM` registry hive contains no `VID_048D` device entry —
   an ITE chip was never connected to this laptop on Windows either.
3. The internal keyboard is PS/2 (`i8042`/serio0) and the i2c bus carries only
   SPD EEPROM (`ee1004`) and kernel dummy devices.

So: **keyboard LEDs are driven by the Insyde EC** (which is also the Clevo
standard for this chassis class).

## 3. The EC protocol (verified live on this machine)

EC access in Linux goes through the kernel ACPI EC driver: load `ec_sys` with
`write_support=1` and use `/sys/kernel/debug/ec/ec0/io` (byte-addressed EC
RAM). This is the same access nbfc-linux's `ec_probe` and gigactl use.

Command mailbox at EC RAM `0xF8..0xFD`:

| Offset | Name | Role |
|---|---|---|
| `0xF8` | FCMD | doorbell — written **last**, triggers execution |
| `0xF9` | FDAT | sub-command |
| `0xFA` | FBUF | parameter 1 |
| `0xFB` | FBF1 | parameter 2 |
| `0xFC` | FBF2 | parameter 3 |
| `0xFD` | FBF3 | parameter 4 |

Keyboard backlight:

| Action | FDAT | FBUF | FBF1 | FBF2 | doorbell |
|---|---|---|---|---|---|
| master enable | `0x0C` | `0x3F` | – | – | `0xC4` |
| master disable | `0x0C` | `0x20` | – | – | `0xC4` |
| colour, zone 0 (whole keyboard) | `0x03` | **Blue** | **Red** | **Green** | `0xCA` |
| brightness (0–255) | `0x06` | level | – | – | `0xCA` |

Gotchas proven during this investigation:

1. **Master enable gates everything.** Until `0xC4/0x0C/0x3F` is sent, the EC
   silently ignores colour/brightness commands (tested: commands sent without
   it do nothing).
2. **Components are B·R·G, not R·G·B.** Sending R,G,B on an already-blue
   keyboard looks like "nothing happened".
3. Mirror/status EC-RAM fields (the `0xFE0B03xx` bank) report **stale** values
   and cannot be used to verify LED state — verify with your eyes.
4. The EC forgets everything on reboot and suspend; state must be re-applied
   (hence the systemd hooks in this repo).
5. Fn brightness hotkeys don't work on Linux: the EC forwards them to a Windows
   driver (`DCHU.HKDR`) that doesn't exist here.

### Verification log

`probe-kb.sh` (see repo root) drove the keyboard through every step with a
human watching; all steps confirmed (`logs/` holds the EC-RAM snapshot):

```
red ✅   green ✅   blue ✅   white ✅
brightness ~15% ✅   ~60% ✅   100% ✅
off ✅   on ✅
```

The keyboard is **single-zone RGB** — the whole board takes one colour, and
there are no per-key LEDs on this unit (the per-key UI in the Windows app is
for other Clevo models sharing the app bundle).

### Effects: there are none in this firmware (verified)

The Windows app advertises effects (breathe / cycle / wave / dance / flash /
tempo). Two independent checks show they are **not firmware modes** on this
machine:

1. **DSDT decode** (this repo's `logs/dsdt/dsdt.dsl`, via `iasl -d dsdt.dat`):
   the only LED interface is the cmd-`0x67` handler in `CLV0001._DSM -> SCMD`
   (mailbox `FCMD/FDAT/FBUF/FBF1-3` = EC RAM `0xF8..0xFD`). It implements
   exactly: master enable (arg top-nibble `0xE…`), zone colours (`0xF0–F3`),
   brightness (`0xF4`) and one two-part op (`0xF6…`, `FDAT=0x09`+`0x0A`). The
   TUXEDO/Clevo driver's mode values (`BREATHE 0x1002a000`, `CYCLE 0x33010000`,
   `WAVE 0xB0000000`, …) decode in this DSDT into legacy `0xC4` fan branches
   that this EC ignores.
2. **Live probe** (`probe-effects.sh`): the `0xF6` op and a scan of plausible
   effect sub-ops (doorbells `0xCA`/`0xC4`) produced no animation whatsoever
   (user-confirmed, all negative).

Conclusion: on the 1-zone G5 keyboard, animated effects are **host-driven** —
the program continuously streams colour/brightness (the same reason
`DeviceIoControl/CppKeyboardColour` warns its Windows themes cost ~15 % CPU).
`g5kbd` therefore implements `effect breathe|cycle` as host-side animation
over the verified mailbox (see README).

## 4. Relationship to existing open-source work

The protocol documented above matches, byte for byte, the one that
**smairio/gigactl** reverse-engineered on 2023-era G5/G6 laptops and the Clevo
keyboard commands in the **wessel-novacustom/clevo-keyboard** (TUXEDO) driver.
This investigation confirms the same command set exists in the **2021 G5 KC
firmware (BIOS FB08)** — the family protocol spans at least 2021–2024.

`g5kbd` differs from gigactl in scope on purpose: gigactl is a full fan +
keyboard + GUI suite packaged for Debian/Ubuntu; `g5kbd` is a minimal, zero
dependency, Arch-friendly CLI for just the keyboard backlight with boot/resume
restore, and it was verified on the specific machine in this repo.

## 5. Files captured during investigation

- `logs/ec-ram-before.txt` — EC RAM `0x00..0xFF` snapshot taken before the
  probe wrote anything.
- `dsdt.dat` — this machine's ACPI DSDT (needs root to read from
  `/sys/firmware/acpi/tables/DSDT`). Disassemble with `iasl -d dsdt.dat`
  (package `acpica` on Arch) — the DSDT contains the `CLV0001`/`CLV0002`
  devices and the `ECMD` mailbox wrappers.
- `tools/ec.py` — raw EC dump/read/write + mailbox CLI used during probing.

## 6. Future work

- Decode `dsdt.dat` (iasl) and map the DSDT's `ECMD`/WMI methods onto the
  raw mailbox writes, to confirm the AML path Windows' AcpiBridge driver uses.
- Effects on this EC are host-driven (verified above), so `g5kbd` implements
  them as local animations; a future step is matching the *timing/curve* of the
  Windows breathe effect more closely, or offering more modes (strobe, tempo).
- Explore binding a proper kernel platform driver to `CLV0001` to expose a
  `/sys/class/leds` entry (the userspace route was chosen because the kernel's
  ACPI EC driver already serialises access safely).
