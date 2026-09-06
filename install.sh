#!/usr/bin/env bash
#
# install.sh — install / uninstall g5kbd (Gigabyte G5 keyboard backlight)
#
#   sudo ./install.sh                 install (build kernel module + GUI)
#   sudo ./install.sh --no-gui        install without the GUI
#   sudo ./install.sh --uninstall     remove everything
#
# What it installs:
#   /usr/lib/modules/$(uname -r)/extra/g5kbd.ko   kernel driver (CLV0001 -> rgb:kbd)
#   /etc/modules-load.d/g5kbd.conf                auto-load g5kbd at boot
#   /etc/udev/rules.d/99-g5kbd.rules              make the LED attrs world-writable
#   /usr/bin/g5kbd                                CLI (EC fallback when the module is absent)
#   /usr/bin/g5kbd-gui                            Tauri v2 GUI (unless --no-gui)
#   /usr/lib/systemd/system/g5kbd.service         boot-time restore
#   /usr/lib/systemd/system-sleep/g5kbd           restore after suspend
#   /var/lib/g5kbd/                               saved state
#
set -eu

SRC="$(cd "$(dirname "$0")" && pwd)"
BIN_SRC="$SRC/src/g5kbd.py"
GUI_DIR="$SRC/gui"
KERNEL_DIR="$SRC/kernel"
NO_GUI=0

if [ "$(id -u)" -ne 0 ]; then
  echo "Please run as root:  sudo $0 [--no-gui|--uninstall]" >&2
  exit 1
fi

UNINSTALL=0
for a in "$@"; do
  case "$a" in
    --uninstall) UNINSTALL=1 ;;
    --no-gui)    NO_GUI=1 ;;
  esac
done

if [ "$UNINSTALL" = 1 ]; then
  echo "==> Uninstalling g5kbd"
  systemctl disable --now g5kbd.service 2>/dev/null || true
  if [ -f /usr/lib/modules/"$(uname -r)"/extra/g5kbd.ko ]; then
    rmmod g5kbd 2>/dev/null || true
    rm -f /usr/lib/modules/"$(uname -r)"/extra/g5kbd.ko
  fi
  rm -f /usr/bin/g5kbd /usr/bin/g5kbd-gui \
        /etc/modprobe.d/g5kbd.conf \
        /etc/modules-load.d/g5kbd.conf \
        /etc/udev/rules.d/99-g5kbd.rules \
        /usr/lib/systemd/system/g5kbd.service \
        /usr/lib/systemd/system-sleep/g5kbd
  rm -rf /var/lib/g5kbd
  systemctl daemon-reload
  echo "==> Done. The keyboard goes back to its firmware default (blue) on reboot."
  exit 0
fi

echo "==> Installing g5kbd"

# ---------- 1. kernel module (native build against the running kernel) ------
KREL="$(uname -r)"
KVERDIR="/lib/modules/$KREL"
MODDIR="$KVERDIR/extra"
if [ -d "$KVERDIR/build" ]; then
  echo "==> Building the kernel module for $KREL"
  ( cd "$KERNEL_DIR" && make >/dev/null )
  install -d "$MODDIR"
  install -m644 "$KERNEL_DIR/g5kbd.ko" "$MODDIR/"
  depmod -a
  echo "==> Loading g5kbd"
  if ! modprobe g5kbd 2>/dev/null; then
    echo "    warning: modprobe g5kbd failed — check 'dmesg | tail'. The CLI"
    echo "    falls back to the raw-EC path, so basic control still works."
  fi
else
  echo "==> Kernel headers for $KREL not found — skipping the kernel module."
  echo "    (g5kbd CLI will use the raw-EC path instead.)"
fi

# ---------- 2. CLI ----------
install -Dm755 "$BIN_SRC" /usr/bin/g5kbd

# ---------- 3. udev: let the desktop user control the LED (no root) ----------
install -Dm644 "$KERNEL_DIR/99-g5kbd.rules" /etc/udev/rules.d/99-g5kbd.rules
install -Dm644 "$SRC/systemd/g5kbd-kmod-load.conf" \
  /etc/modules-load.d/g5kbd.conf       # auto-load g5kbd at every boot
udevadm control --reload-rules 2>/dev/null || true
# Apply world-write on the LED attributes right now too (udev only acts on
# future adds; sysfs ignores chgrp, so chmod is the only option):
if [ -d /sys/class/leds/rgb:kbd ]; then
  chmod 0666 /sys/class/leds/rgb:kbd/brightness \
             /sys/class/leds/rgb:kbd/multi_intensity 2>/dev/null || true
fi

# ---------- 4. GUI (Tauri v2: web frontend + Rust core) ----------
# Needs node/npm for the frontend and cargo for the Rust core. First build
# downloads the crates and takes a few minutes.
if [ "$NO_GUI" = 0 ] && command -v npm >/dev/null 2>&1 && command -v cargo >/dev/null 2>&1; then
  echo "==> Building the GUI (npm + cargo tauri, first build takes a while)"
  if ( cd "$GUI_DIR" && npm install >/dev/null 2>&1 \
        && npm run tauri -- build --no-bundle >/dev/null 2>&1 ) \
     && [ -x "$GUI_DIR/src-tauri/target/release/g5kbd-gui" ]; then
    install -m755 "$GUI_DIR/src-tauri/target/release/g5kbd-gui" /usr/bin/g5kbd-gui
  else
    echo "    warning: GUI build failed — skipping (CLI still installed)."
    rm -f /usr/bin/g5kbd-gui
  fi
elif [ "$NO_GUI" = 0 ]; then
  echo "==> npm/cargo not found — skipping the GUI. To build it later:"
  echo "    cd gui && npm install && npm run tauri -- build --no-bundle"
  echo "    sudo install -m755 src-tauri/target/release/g5kbd-gui /usr/bin/g5kbd-gui"
fi

# ---------- 5. boot / suspend restore ----------
install -Dm644 "$SRC/systemd/g5kbd.service" /usr/lib/systemd/system/g5kbd.service
install -Dm755 "$SRC/systemd/system-sleep/g5kbd" /usr/lib/systemd/system-sleep/g5kbd
install -d -m 755 /var/lib/g5kbd
# Non-root members of wheel control the keyboard through the LED node (udev)
# and the CLI needs to persist its state file: make the state dir group-writeable.
chgrp -R wheel /var/lib/g5kbd 2>/dev/null || true
chmod 2775 /var/lib/g5kbd 2>/dev/null || true

systemctl daemon-reload
systemctl enable g5kbd.service >/dev/null 2>&1 || true

echo
echo "==> Installed."
if [ -e /sys/class/leds/rgb:kbd ]; then
  echo "    kernel driver live: /sys/class/leds/rgb:kbd  (no root needed)"
else
  echo "    kernel module not (yet) active — the CLI uses the raw-EC path."
fi
echo
echo "    Try:"
echo "      g5kbd color red        (or any RRGGBB hex / colour name)"
echo "      g5kbd brightness 60"
echo "      g5kbd off / on"
[ "$NO_GUI" = 0 ] && echo "      g5kbd-gui             (graphical panel)"
echo "    Your colour is remembered and restored at boot and after suspend."
