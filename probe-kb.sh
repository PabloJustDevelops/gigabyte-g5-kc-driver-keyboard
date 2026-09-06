#!/usr/bin/env bash
#
# probe-kb.sh — verify the Clevo EC keyboard-backlight mailbox protocol on this
# Gigabyte G5 KC (run as root:  sudo ./probe-kb.sh)
#
# Sends the EC commands that gigactl reverse-engineered on newer G5/G6 models:
#   master enable   : FDAT=0x0C  FBUF=0x3F  doorbell 0xC4
#   set color zone0 : FDAT=0x03  FBUF=B  FBF1=R  FBF2=G  doorbell 0xCA   (B,R,G!)
#   brightness      : FDAT=0x06  FBUF=level(0-255)         doorbell 0xCA
#   master disable  : FDAT=0x0C  FBUF=0x20  doorbell 0xC4
#
# Everything resets on reboot; at the end we restore the firmware default
# (blue, full brightness).

set -u
cd "$(dirname "$0")"

HERE="$(pwd)"
ECPY="$HERE/tools/ec.py"
LOGDIR="$HERE/logs"
mkdir -p "$LOGDIR"

OWNER="${SUDO_USER:-$(stat -c %U "$HERE" 2>/dev/null || echo root)}"

echo "================================================================"
echo " G5 KC keyboard-backlight EC probe"
echo "================================================================"

# --- 0. root ----------------------------------------------------------
if [ "$(id -u)" -ne 0 ]; then
  echo "Please run as root:  sudo $0" >&2
  exit 1
fi

# --- 1. enable EC writes ----------------------------------------------
echo "[1/6] Enabling EC write access (ec_sys write_support=1) ..."
modprobe -r ec_sys 2>/dev/null
modprobe ec_sys write_support=1 2>/dev/null || modprobe ec_sys 2>/dev/null

if ! mountpoint -q /sys/kernel/debug; then
  mount -t debugfs none /sys/kernel/debug 2>/dev/null
fi

IO=$(ls /sys/kernel/debug/ec/ec*/io 2>/dev/null | head -1)
if [ -z "$IO" ]; then
  echo "ERROR: no /sys/kernel/debug/ec/ec*/io after modprobe." >&2
  echo "Your kernel may lack CONFIG_ACPI_EC_DEBUGFS or ship ec_sys built-in." >&2
  exit 1
fi
echo "      EC interface: $IO"
if [ ! -w "$IO" ]; then
  echo "ERROR: $IO is read-only (module loaded without write_support?)." >&2
  echo "Try:  modprobe -r ec_sys && modprobe ec_sys write_support=1" >&2
  exit 1
fi

# --- 2. save pre-state dump + DSDT -------------------------------------
echo "[2/6] Snapshotting EC RAM and DSDT ..."
python3 "$ECPY" dump 0 0x100 > "$LOGDIR/ec-ram-before.txt" 2>&1
echo "      EC RAM  -> logs/ec-ram-before.txt"

if [ -r /sys/firmware/acpi/tables/DSDT ]; then
  cat /sys/firmware/acpi/tables/DSDT > "$HERE/dsdt.dat"
  chown "$OWNER" "$HERE/dsdt.dat" 2>/dev/null
  echo "      DSDT    -> dsdt.dat ($(stat -c %s "$HERE/dsdt.dat" 2>/dev/null) bytes)"
fi

wr() { python3 "$ECPY" wr "$1" "$2" >/dev/null; }

# mailbox helpers (values are hex)
kb_enable()  { wr f9 0c; wr fa 3f; wr f8 c4; }
kb_disable() { wr f9 0c; wr fa 20; wr f8 c4; }
# color given as BRG hex triple
kb_color()   { kb_enable; wr f9 03; wr fa "$1"; wr fb "$2"; wr fc "$3"; wr f8 ca; }
kb_bright()  { kb_enable; wr f9 06; wr fa "$1"; wr f8 ca; }

ask() { # $1 = question; returns 0 on y/Y
  local ans
  printf '      >>> %s  [y/n] ' "$1"
  read -r ans || true
  case "$ans" in y|Y|s|S) return 0;; *) return 1;; esac
}

echo "[3/6] Master-enabling the keyboard backlight (doorbell 0xC4) ..."
kb_enable
sleep 1

echo "[4/6] Colour test — each colour stays until you answer."
echo "      Watch the keyboard backlight and answer honestly."

for entry in "red:00 ff 00" "green:00 00 ff" "blue:ff 00 00" "white:ff ff ff"; do
  name="${entry%%:*}"; brg="${entry#*:}"
  set -- $brg
  kb_color "$1" "$2" "$3"
  sleep 1
  if ask "Are the keys now lit ${name}?"; then
    echo "      -> ${name} works."
  else
    echo "      -> ${name}: no (or wrong) change."
  fi
done

echo "[5/6] Brightness test (white first, then 25 / 60 / 100%) ..."
kb_color ff ff ff
sleep 1
for lvl in 40 153 255; do
  kb_bright "$(printf '%02x' "$lvl")"
  sleep 1
  pct=$(( lvl * 100 / 255 ))
  ask "Did the brightness change to about ${pct}% (of full)?" \
    && echo "      -> brightness step ${lvl} (${pct}%) ok." \
    || echo "      -> brightness step ${lvl} (${pct}%): no visible change."
done

echo "[6/6] On/off test ..."
kb_disable
sleep 1
if ask "Did the backlight turn OFF?"; then echo "      -> off works."; else echo "      -> off: no change."; fi
kb_enable
sleep 1
if ask "Did the backlight turn back ON?"; then echo "      -> on works."; else echo "      -> on: no change."; fi

# --- restore firmware-ish default: blue (0,0,200), full brightness ----
echo "Restoring default look: blue, full brightness."
kb_color c8 00 00
kb_bright ff

chown -R "$OWNER" "$LOGDIR" 2>/dev/null || true
echo
echo "Done. If nothing ever changed, the 0xCA mailbox is not the right"
echo "protocol for this EC and we will dig into the Windows app instead."
