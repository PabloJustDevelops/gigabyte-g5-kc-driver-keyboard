#!/usr/bin/env bash
#
# probe-effects.sh — hunt for firmware effect modes on the G5 KC keyboard
# (run as root:  sudo ./probe-effects.sh)
#
# Background: the DSDT (CLV0001._DSM -> SCMD, cmd 0x67) maps LED commands to
# the EC mailbox. Static colour, brightness, and master enable are proven.
# This probe tests the one remaining LED sub-op (0xF6 -> two-part 0x09/0x0A)
# plus a handful of speculative sub-commands, asking you to report what the
# keyboard does. Everything resets on reboot.

set -u
cd "$(dirname "$0")"
HERE="$(pwd)"
ECPY="$HERE/tools/ec.py"

if [ "$(id -u)" -ne 0 ]; then
  echo "Please run as root:  sudo $0" >&2
  exit 1
fi

IO=$(ls /sys/kernel/debug/ec/ec*/io 2>/dev/null | head -1)
if [ -z "$IO" ] || [ ! -w "$IO" ]; then
  echo "EC not writable — load it first:"
  echo "  modprobe -r ec_sys; modprobe ec_sys write_support=1"
  exit 1
fi

wr() { python3 "$ECPY" wr "$1" "$2" >/dev/null; }
enable()  { wr f9 0c; wr fa 3f; wr f8 c4; }
setrgb() { # $1=B $2=R $3=G
  enable
  wr f9 03; wr fa "$1"; wr fb "$2"; wr fc "$3"; wr f8 ca
}
setbri() { wr f9 06; wr fa "$1"; wr f8 ca; }

ask() {
  local ans
  printf '      >>> %s  [y/n] ' "$1"
  read -r ans || true
  case "$ans" in y|Y|s|S) return 0;; *) return 1;; esac
}

echo "================================================================"
echo " Firmware-effect hunt on the G5 KC keyboard"
echo "================================================================"

echo "[1/4] Baseline: solid WHITE, full brightness"
setrgb ff ff ff
setbri ff
sleep 1

echo "[2/4] The DSDT's 0xF6 op (two-part 0x09 + 0x0A mailbox write)."
echo "      Sending it exactly as the firmware does, with white then red..."
setrgb ff ff ff
wr f9 09; wr fa ff; wr fb ff; wr fc ff; wr f8 ca
wr f9 0a; wr fa ff; wr fb ff; wr fc ff; wr f8 ca
sleep 2
if ask "After the 0xF6 white write: did the light DO anything special
      (blink, fade, cycle, two-colour, off) beyond staying solid white?";
then echo "      -> 0xF6 produced an effect (white)." ; else echo "      -> 0xF6: nothing special (white)."; fi

setrgb ff 00 00
wr f9 09; wr fa 00; wr fb ff; wr fc 00; wr f8 ca
wr f9 0a; wr fa 00; wr fb ff; wr fc 00; wr f8 ca
sleep 2
if ask "And with RED did 0xF6 do anything special?"; then
  echo "      -> 0xF6 produced an effect (red)."
else
  echo "      -> 0xF6: nothing special (red)."
fi

echo "[3/4] Speculative sub-ops on doorbell 0xCA (each preceded by a fresh"
echo "      WHITE so an effect is obvious). 2 seconds each — answer after each."
setrgb ff ff ff

for spec in "01:00" "02:00" "08:00" "0b:00" "01:01" "02:01"; do
  sub="${spec%%:*}"; p1="${spec#*:}"
  setrgb ff ff ff
  wr f9 "$sub"; wr fa "$p1"; wr f8 ca
  sleep 2
  if ask "Doorbell 0xCA sub=0x${sub} p1=0x${p1}: did anything animate
        (breathe, cycle, wave, blink, flicker)?"; then
    echo "      -> sub 0x${sub} p1=0x${p1} LOOKS LIVE."
  else
    echo "      -> sub 0x${sub} p1=0x${p1}: nothing."
  fi
done

echo "[4/4] Doorbell 0xC4 sub-command scan around the enable (0x0C):"
echo "      trying subs 0x09/0x0A/0x0B/0x0D/0x0E with FBUF=0x3F..."
for sub in 09 0a 0b 0d 0e; do
  enable
  wr f9 "$sub"; wr fa 3f; wr f8 c4
  sleep 2
  if ask "Doorbell 0xC4 sub=0x${sub} FBUF=0x3F: did anything animate or
        visibly change?"; then
    echo "      -> 0xC4 sub 0x${sub} LOOKS LIVE."
  else
    echo "      -> 0xC4 sub 0x${sub}: nothing."
  fi
done

echo
echo "Restoring: solid blue, full brightness (firmware default look)."
setrgb c8 00 00
setbri ff
echo "Done."
