#!/usr/bin/env bash
#
# install.sh — install / uninstall g5kbd (Gigabyte G5 keyboard backlight)
#
#   sudo ./install.sh                 install (build kernel module + GUI)
#   sudo ./install.sh --no-gui        install without the GUI
#   sudo ./install.sh --no-fan        install without the fan tool/watchdog
#   sudo ./install.sh --uninstall     remove everything
#
# What it installs:
#   /usr/lib/modules/$(uname -r)/extra/g5kbd.ko   kernel driver (CLV0001 -> rgb:kbd)
#   /etc/modules-load.d/g5kbd.conf                auto-load g5kbd at boot
#   /etc/udev/rules.d/99-g5kbd.rules              make the LED attrs world-writable
#   /usr/bin/g5kbd                                CLI (EC fallback when the module is absent)
#   /usr/bin/g5fan                                fan control CLI (unless --no-fan)
#   /usr/bin/g5kbd-gui                            Tauri v2 GUI (unless --no-gui)
#   /usr/lib/systemd/system/g5kbd.service         boot-time restore
#   /usr/lib/systemd/system-sleep/g5kbd           restore after suspend
#   /usr/lib/systemd/system/g5fan-watchdog.service  fan daemon: duty curves + thermal ceiling (unless --no-fan)
#   /var/lib/g5kbd/, /var/lib/g5fan/              saved state
#
set -eu

SRC="$(cd "$(dirname "$0")" && pwd)"
BIN_SRC="$SRC/src/g5kbd.py"
FAN_SRC="$SRC/src/g5fan.py"
GUI_DIR="$SRC/gui"
KERNEL_DIR="$SRC/kernel"
NO_GUI=0
NO_FAN=0

if [ "$(id -u)" -ne 0 ]; then
  echo "Please run as root:  sudo $0 [--no-gui|--no-fan|--uninstall]" >&2
  exit 1
fi

UNINSTALL=0
for a in "$@"; do
  case "$a" in
    --uninstall) UNINSTALL=1 ;;
    --no-gui)    NO_GUI=1 ;;
    --no-fan)    NO_FAN=1 ;;
  esac
done

if [ "$UNINSTALL" = 1 ]; then
  echo "==> Uninstalling g5kbd"
  systemctl disable --now g5kbd.service 2>/dev/null || true
  systemctl disable --now g5fan-watchdog.service 2>/dev/null || true
  # Put the fans back on the firmware curve before the module goes away.
  if [ -x /usr/bin/g5fan ]; then
    g5fan auto >/dev/null 2>&1 || true
  fi
  if [ -f /usr/lib/modules/"$(uname -r)"/extra/g5kbd.ko ]; then
    rmmod g5kbd 2>/dev/null || true
    rm -f /usr/lib/modules/"$(uname -r)"/extra/g5kbd.ko
  fi
  rm -f /usr/bin/g5kbd /usr/bin/g5fan /usr/bin/g5kbd-gui \
        /etc/modprobe.d/g5kbd.conf \
        /etc/modules-load.d/g5kbd.conf \
        /etc/udev/rules.d/99-g5kbd.rules \
        /usr/lib/systemd/system/g5kbd.service \
        /usr/lib/systemd/system/g5fan-watchdog.service \
        /usr/share/polkit-1/actions/dev.g5kbd.fan.policy \
        /usr/lib/systemd/system-sleep/g5kbd \
        /usr/share/applications/g5kbd-gui.desktop \
        /usr/share/icons/hicolor/32x32/apps/g5kbd.png \
        /usr/share/icons/hicolor/128x128/apps/g5kbd.png
  rm -rf /var/lib/g5kbd /var/lib/g5fan /usr/share/doc/g5kbd
  systemctl daemon-reload
  echo "==> Done. The keyboard goes back to its firmware default (blue) on"
  echo "    reboot, and the fans follow the firmware curve again."
  exit 0
fi

echo "==> Installing g5kbd"

# ---------- 1. kernel module -------------------------------------------------
# Native build against the running kernel — unless DKMS already owns the
# module for it, in which case DKMS rebuilds it on every kernel update and
# the native copy would only conflict with it.
KREL="$(uname -r)"
KVERDIR="/lib/modules/$KREL"
MODDIR="$KVERDIR/extra"

# Read name/version out of the DKMS config so we address its tree correctly.
DKMS_NAME="$(sed -n 's/^PACKAGE_NAME="\(.*\)"/\1/p' "$KERNEL_DIR/dkms.conf")"
DKMS_VER="$(sed -n 's/^PACKAGE_VERSION="\(.*\)"/\1/p' "$KERNEL_DIR/dkms.conf")"
DKMS_SRC="/usr/src/${DKMS_NAME}-${DKMS_VER}"

if command -v dkms >/dev/null 2>&1 && [ -n "$DKMS_NAME" ] \
   && dkms status -m "$DKMS_NAME" 2>/dev/null | grep -q "$KREL"; then
  # DKMS owns the module for this kernel, so the native build below would only
  # conflict with it.
  #
  # The subtlety: DKMS keeps its own *snapshot* of the tree per kernel. Just
  # copying the new g5kbd.c into /usr/src and running `dkms build` is not
  # enough — the already-built module is kept and nothing is recompiled, so
  # the old driver silently stays live. A source change needs the full
  # remove -> add -> build -> install cycle.
  echo "==> g5kbd is managed by DKMS for $KREL — rebuilding from source"
  install -Dm644 "$KERNEL_DIR/g5kbd.c"  "$DKMS_SRC/g5kbd.c"
  install -Dm644 "$KERNEL_DIR/Makefile" "$DKMS_SRC/Makefile"
  install -Dm644 "$KERNEL_DIR/dkms.conf" "$DKMS_SRC/dkms.conf"
  install -Dm644 "$KERNEL_DIR/99-g5kbd.rules" "$DKMS_SRC/99-g5kbd.rules"

  DKMS_LOG="$(mktemp)"
  # Unload first: dkms remove refuses while the module is in use, and its
  # error is easy to miss behind a redirect.
  if ! modprobe -r g5kbd >"$DKMS_LOG" 2>&1; then
    echo "    warning: could not unload g5kbd:"
    sed 's/^/      /' "$DKMS_LOG"
  fi
  # `set -e` is on, so every dkms step is guarded explicitly: a failed rebuild
  # must not abort the rest of the install.
  DKMS_RC=0
  dkms remove  -m "$DKMS_NAME" -v "$DKMS_VER"            >>"$DKMS_LOG" 2>&1 || true
  dkms add     -m "$DKMS_NAME" -v "$DKMS_VER" "$DKMS_SRC" >>"$DKMS_LOG" 2>&1 || DKMS_RC=1
  dkms build   -m "$DKMS_NAME" -v "$DKMS_VER"            >>"$DKMS_LOG" 2>&1 || DKMS_RC=1
  dkms install -m "$DKMS_NAME" -v "$DKMS_VER"            >>"$DKMS_LOG" 2>&1 || DKMS_RC=1
  if [ "$DKMS_RC" -ne 0 ]; then
    echo "    warning: the DKMS rebuild failed — last lines of the log:"
    tail -15 "$DKMS_LOG" | sed 's/^/      /'
    echo "    full log: $DKMS_LOG"
  else
    rm -f "$DKMS_LOG"
  fi
  depmod -a
  # Whatever happened above, make sure *something* is loaded.
  modprobe g5kbd 2>/dev/null || true
  if ! lsmod | grep -q '^g5kbd '; then
    echo "    warning: g5kbd is not loaded — check 'dmesg | tail'."
  fi
elif [ -d "$KVERDIR/build" ]; then
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

# ---------- 2b. fan CLI + watchdog ----------
# *Changing* a fan needs root (the EC is not group-writable like the LED node),
# so g5fan elevates with sudo/pkexec itself. Looking at one does not: the
# snapshot the daemon publishes to /run/g5fan is world-readable, and the state
# file only ever holds the selected mode.
if [ "$NO_FAN" = 0 ]; then
  install -Dm755 "$FAN_SRC" /usr/bin/g5fan
  install -Dm644 "$SRC/systemd/g5fan-watchdog.service" \
    /usr/lib/systemd/system/g5fan-watchdog.service
  # Writes from the GUI (no terminal to prompt on, and the prompt is scoped to
  # just `g5fan`). The reads never get here at all.
  install -Dm644 "$SRC/polkit/dev.g5kbd.fan.policy" \
    /usr/share/polkit-1/actions/dev.g5kbd.fan.policy
  install -d -m 755 /var/lib/g5fan
  chmod 755 /var/lib/g5fan 2>/dev/null || true   # fix a pre-0.4 install
fi

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

# ---------- 4b. desktop launcher -------------------------------------------
# `tauri build --no-bundle` never generates a .desktop entry, so without this
# the panel gets installed but stays invisible in the application menu.
if [ -x /usr/bin/g5kbd-gui ]; then
  echo "==> Installing the desktop launcher"
  install -Dm644 "$GUI_DIR/g5kbd-gui.desktop" /usr/share/applications/g5kbd-gui.desktop
  for s in 32x32 128x128; do
    [ -f "$GUI_DIR/src-tauri/icons/$s.png" ] && \
      install -Dm644 "$GUI_DIR/src-tauri/icons/$s.png" "/usr/share/icons/hicolor/$s/apps/g5kbd.png"
  done
  command -v update-desktop-database >/dev/null 2>&1 && \
    update-desktop-database /usr/share/applications 2>/dev/null || true
fi

# ---------- 5. boot / suspend restore ----------
install -Dm644 "$SRC/systemd/g5kbd.service" /usr/lib/systemd/system/g5kbd.service
install -Dm755 "$SRC/systemd/system-sleep/g5kbd" /usr/lib/systemd/system-sleep/g5kbd

# The reference docs travel with the install: the systemd units link to them
# (`Documentation=`), and the roadmap is where the known gaps are written down.
install -Dm644 "$SRC/README.md" /usr/share/doc/g5kbd/README.md
install -Dm644 "$SRC/docs/WINDOWS-RESEARCH.md" \
  /usr/share/doc/g5kbd/WINDOWS-RESEARCH.md
install -Dm644 "$SRC/docs/FAN-RESEARCH.md" /usr/share/doc/g5kbd/FAN-RESEARCH.md
install -Dm644 "$SRC/docs/ROADMAP.md" /usr/share/doc/g5kbd/ROADMAP.md
install -d -m 755 /var/lib/g5kbd
# Non-root members of wheel control the keyboard through the LED node (udev)
# and the CLI needs to persist its state file: make the state dir group-writeable.
chgrp -R wheel /var/lib/g5kbd 2>/dev/null || true
chmod 2775 /var/lib/g5kbd 2>/dev/null || true

systemctl daemon-reload
systemctl enable g5kbd.service >/dev/null 2>&1 || true
if [ "$NO_FAN" = 0 ]; then
  systemctl enable g5fan-watchdog.service >/dev/null 2>&1 || true
  # ec_sys has to carry write_support=1 for the watchdog to reach the EC.
  install -Dm644 "$SRC/systemd/g5kbd-ec.conf" /etc/modprobe.d/g5kbd.conf
  if ! grep -q '^ec_sys$' /etc/modules-load.d/g5kbd.conf 2>/dev/null; then
    echo "ec_sys" >> /etc/modules-load.d/g5kbd.conf
  fi
  systemctl restart g5fan-watchdog.service >/dev/null 2>&1 || true
fi

echo
echo "==> Installed."
if [ -e /sys/class/leds/rgb:kbd ]; then
  echo "    kernel driver live: /sys/class/leds/rgb:kbd  (no root needed)"
else
  echo "    kernel module not (yet) active — the CLI uses the raw-EC path."
fi
if [ "$NO_FAN" = 0 ]; then
  # The driver puts its fan attributes on the ACPI device (CLV0001:00); the
  # platform device the ACPI core mirrors it to carries none. Accept either.
  FAN_ATTR="$(ls -d /sys/bus/acpi/devices/CLV0001:*/fan_mode \
                       /sys/bus/platform/devices/CLV0001:*/fan_mode \
                       2>/dev/null | head -1)"
  if [ -n "$FAN_ATTR" ]; then
    echo "    fan control live: ${FAN_ATTR%/fan_mode}/fan_{mode,duty}"
  else
    echo "    warning: no fan_mode attribute found — g5fan will fall back to"
    echo "             the raw EC.  Check: dmesg | grep g5kbd, or g5fan doctor"
  fi
  if [ -e /sys/kernel/debug/ec/ec0/io ]; then
    echo "    ec_sys present — g5fan reads duty/RPM/temperatures from it."
  else
    echo "    warning: no /sys/kernel/debug/ec/ec0/io — run: modprobe ec_sys write_support=1"
  fi
fi
echo
echo "    Try:"
echo "      g5kbd color red        (or any RRGGBB hex / colour name)"
echo "      g5kbd brightness 60"
echo "      g5kbd off / on"
[ "$NO_FAN" = 0 ] && {
  echo "      g5fan status           (fan mode, duty, RPM, temperatures)"
  echo "      g5fan turbo            (full speed)  ·  g5fan auto"
  echo "      g5fan silent | maxq | custom   (duty curves)"
  echo "      g5fan manual 60        (fixed duty)"
  echo "      g5fan doctor           (checks every layer)"
  echo "      g5kbd fan status       (same thing, shorter to type)"
}
[ "$NO_GUI" = 0 ] && echo "      g5kbd-gui             (graphical panel)"
echo "    Your colour is remembered and restored at boot and after suspend."
[ "$NO_FAN" = 0 ] && echo "    The fan daemon is running: it drives the silent/maxq/custom curves,"
[ "$NO_FAN" = 0 ] && echo "    hands the fans back to the firmware curve if the CPU gets too hot,"
[ "$NO_FAN" = 0 ] && echo "    and publishes the numbers the panel shows — which is why looking"
[ "$NO_FAN" = 0 ] && echo "    at the fans in the GUI never asks for a password. Stop it with:"
[ "$NO_FAN" = 0 ] && echo "      sudo systemctl disable --now g5fan-watchdog.service"
