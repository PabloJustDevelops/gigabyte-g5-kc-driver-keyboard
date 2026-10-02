#!/usr/bin/env bash
#
# probe-fan.sh — verify the Clevo EC fan protocol on this Gigabyte G5 KC
#
#   sudo ./probe-fan.sh
#
# The fan commands were decoded from this machine's own DSDT (see
# docs/FAN-RESEARCH.md) but the *effects* still have to be confirmed by a
# human, because nothing in the EC reports whether a duty was accepted the way
# the LED path could be eyeballed:
#
#   set duty   : FDAT=fan  FBUF=duty(0-255)  doorbell 0xC1
#   back to auto: FDAT=0xFF FBUF=fan         doorbell 0xC1
#   read-back  : duty 0xCE (CPU) / 0xCF (GPU)
#                rpm  0xD0/0xD1 (CPU) / 0xD2/0xD3 (GPU), 16-bit big-endian
#                     period, not a speed: rpm = 2156220 / period
#
# This walks the modes in a safe order and asks you to confirm each one by
# ear, leaving the fans on the firmware curve at the end. It never leaves a
# manual duty pinned: the script traps EXIT and puts the fans back on auto.
#
# Options:
#   --duty-only     skip the curve section
#   --no-prompt     run every step unattended, just record the numbers
#
# For a quick, non-interactive version of the same checks, run
# `g5fan doctor --write`: it reads the EC around a duty write and says
# whether the EC took it.

set -u
cd "$(dirname "$0")"

HERE="$(pwd)"
LOGDIR="$HERE/logs"
mkdir -p "$LOGDIR"
ECPY="$HERE/tools/ec.py"
G5FAN="$HERE/src/g5fan.py"

DUTY_ONLY=0
NO_PROMPT=0
for a in "$@"; do
  case "$a" in
    --duty-only) DUTY_ONLY=1 ;;
    --no-prompt) NO_PROMPT=1 ;;
    -h|--help)   sed -n '2,26p' "$0"; exit 0 ;;
  esac
done

say()  { printf '%s\n' "$*"; }
rule() { say "----------------------------------------------------------------"; }

# ask CONFIRM_PROMPT <text>; returns 0 for yes. Non-interactive -> assume yes.
ask() {
  [ "$NO_PROMPT" = 1 ] && return 0
  local reply
  printf '%s [y/N] ' "$1"
  read -r reply || return 1
  case "$reply" in [yY]*) return 0 ;; *) return 1 ;; esac
}

# --- 0. preconditions --------------------------------------------------
say "================================================================"
say " G5 KC fan EC probe"
say "================================================================"
rule

if [ "$(id -u)" -ne 0 ]; then
  say "Please run as root:  sudo $0" >&2
  exit 1
fi

say "[0/6] Machine:"
say "      vendor : $(cat /sys/class/dmi/id/sys_vendor 2>/dev/null)"
say "      product: $(cat /sys/class/dmi/id/product_name 2>/dev/null)"
say "      BIOS   : $(cat /sys/class/dmi/id/bios_version 2>/dev/null)"
VENDOR=$(cat /sys/class/dmi/id/sys_vendor 2>/dev/null)
PRODUCT=$(cat /sys/class/dmi/id/product_name 2>/dev/null)
if [ "$VENDOR" != "GIGABYTE" ] || ! case "$PRODUCT" in G5*|G6*|G7*) true ;; *) false ;; esac; then
  say "ERROR: this is not a Gigabyte G5/G6/G7 — aborting rather than poking"
  say "       an EC we have not reverse-engineered." >&2
  exit 1
fi
rule

# --- 1. EC write access ------------------------------------------------
say "[1/6] Enabling EC write access (ec_sys write_support=1) ..."
modprobe -r ec_sys 2>/dev/null
modprobe ec_sys write_support=1 2>/dev/null || modprobe ec_sys 2>/dev/null
mountpoint -q /sys/kernel/debug || mount -t debugfs none /sys/kernel/debug 2>/dev/null

IO=$(ls /sys/kernel/debug/ec/ec*/io 2>/dev/null | head -1)
if [ -z "$IO" ]; then
  say "ERROR: no /sys/kernel/debug/ec/ec*/io after modprobe." >&2
  say "Your kernel may lack CONFIG_ACPI_EC_DEBUGFS." >&2
  exit 1
fi
say "      EC interface: $IO"
[ -w "$IO" ] || {
  say "ERROR: $IO is read-only (module loaded without write_support?)." >&2
  say "Try:  modprobe -r ec_sys && modprobe ec_sys write_support=1" >&2
  exit 1
}
rule

# --- safety net --------------------------------------------------------
restore() {
  say
  say "==> restoring the firmware fan curve"
  G5FAN_NO_SUDO=1 python3 "$G5FAN" auto >/dev/null 2>&1 || \
    echo "  (could not reach the EC to restore auto — reboot to be safe)"
}
trap restore EXIT

# --- 2. baseline -------------------------------------------------------
say "[2/6] Baseline: fans on the firmware curve, reading back what we can."
python3 "$G5FAN" status | tee "$LOGDIR/fan-probe-baseline.txt"
say
say "NOTE: the duty/RPM read-back registers are what the firmware *mirrors*."
say "      They can be stale, so treat the numbers as a hint and your ears"
say "      as the truth."
rule

# --- 3. manual duty sweep ---------------------------------------------
say "[3/6] Manual duty sweep (30% -> 60% -> 100%)."
say "      Listen for the fans to speed up, then settle."
for pct in 30 60 100; do
  say ""
  say "  -> manual $pct%"
  G5FAN_NO_SUDO=1 python3 "$G5FAN" manual "$pct" || {
    say "     (write refused — is g5kbd.ko loaded? the raw-EC path needs root)"
    break
  }
  sleep 6
  python3 "$G5FAN" status | sed -n '3,6p' | tee -a "$LOGDIR/fan-probe-duty.txt"
  ask "     did both fans visibly speed up?" || { say "     stopping here"; break; }
done
rule

# --- 4. back to auto ---------------------------------------------------
say "[4/6] Handing the fans back to the firmware curve."
G5FAN_NO_SUDO=1 python3 "$G5FAN" auto
sleep 6
python3 "$G5FAN" status | tee -a "$LOGDIR/fan-probe-duty.txt"
say
ask "  did the fans drop back to idle?" || say "  (noted — the EC may keep a "
rule

# --- 5. fan curve ------------------------------------------------------
if [ "$DUTY_ONLY" = 1 ]; then
  say "[5/6] Curve section skipped (--duty-only)."
else
  say "[5/6] Fan curve: a temperature ramp the daemon turns into duty."
  say "      The firmware's own curve table only lets the OS write two of its"
  say "      four points and hides its RPM set-points, so g5fan shapes the"
  say "      ramp from userspace instead. This checks that it really does."
  say ""
  G5FAN_NO_SUDO=1 python3 "$G5FAN" curve | tee "$LOGDIR/fan-probe-curve.txt"
  say ""
  say "  -> a deliberately gentle curve (55:12 70:25 85:45 95:100)"
  G5FAN_NO_SUDO=1 python3 "$G5FAN" curve set 55:12 70:25 85:45 95:100 || \
    say "     (curve refused — see the output above)"
  say "  -> selecting it; the daemon picks it up within a few seconds"
  G5FAN_NO_SUDO=1 python3 "$G5FAN" custom || \
    say "     (could not select it)"
  sleep 8
  G5FAN_NO_SUDO=1 python3 "$G5FAN" status | tee -a "$LOGDIR/fan-probe-curve.txt"
  say ""
  say "  Now load the machine for a minute (a compile, a game) and watch:"
  ask "  does the fan stay quiet at idle and still ramp up under load?" \
    || say "  (noted)"
  rule
  say "  -> back to the firmware curve"
  G5FAN_NO_SUDO=1 python3 "$G5FAN" auto >/dev/null 2>&1 || true
fi

# --- 6. done -----------------------------------------------------------
say "[6/6] Done."
say "Logs written to logs/fan-probe-*.txt"
say
say "If every step behaved, the fan protocol in docs/FAN-RESEARCH.md is"
say "confirmed on this machine. If a step did not, that section lists what is"
say "still inference and what to re-check."
