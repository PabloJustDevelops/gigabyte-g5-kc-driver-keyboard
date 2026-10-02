# Maintainer: PabloJustDevelops
# Build with:  makepkg -si   (inside this repo)
pkgname=g5kbd
pkgver=0.4.0
pkgrel=1
pkgdesc="Keyboard backlight and fan control for Gigabyte G5 (Clevo-ODM) laptops on Linux: kernel driver (CLV0001 -> rgb:kbd), CLIs and GPUI panel"
arch=('x86_64')
url=""
license=('MIT' 'GPL2')
# The panel draws itself with GPUI: it needs the Vulkan loader (wgpu), the
# X11/Wayland client libraries it links against, and fontconfig (zed-font-kit
# finds the system fonts through it). The Wayland backend is loaded at
# runtime, so libwayland-client is a hard dependency rather than optional.
depends=('python' 'vulkan-icd-loader' 'libxkbcommon' 'libxkbcommon-x11' 'libxcb' 'wayland' 'fontconfig')
makedepends=('linux-headers' 'rust' 'cargo' 'make' 'gcc' 'clang' 'lld' 'pkgconf' 'wayland' 'libxkbcommon' 'libxcb' 'fontconfig' 'freetype2' 'vulkan-headers')
source=("src/g5kbd.py"
        "src/g5fan.py"
        "systemd/g5kbd.service"
        "systemd/g5fan-watchdog.service"
        "systemd/system-sleep/g5kbd"
        "systemd/g5kbd-ec.conf"
        "systemd/g5kbd-modules-load.conf"
        "polkit/dev.g5kbd.fan.policy"
        "kernel/g5kbd.c"
        "kernel/Makefile"
        "kernel/99-g5kbd.rules"
        "gui/Cargo.toml"
        "gui/Cargo.lock"
        "gui/src/main.rs"
        "gui/src/panel.rs"
        "gui/src/views.rs"
        "gui/src/widgets.rs"
        "gui/src/theme.rs"
        "gui/src/backend/mod.rs"
        "gui/src/backend/kbd.rs"
        "gui/src/backend/fan.rs"
        "gui/g5kbd-gui.desktop"
        "gui/icons/128x128.png"
        "gui/icons/32x32.png"
        "README.md"
        "docs/WINDOWS-RESEARCH.md"
        "docs/FAN-RESEARCH.md"
        "docs/ROADMAP.md")
sha256sums=('SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP'
            'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP'
            'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP'
            'SKIP' 'SKIP' 'SKIP' 'SKIP')

build() {
  # --- kernel module against the running kernel's headers ---
  cd "$srcdir/kernel"
  make || make CC=clang LLVM=1
  # --- Panel: a plain cargo build. GPUI is compiled as part of the binary;
  # there is no web frontend and no Node toolchain any more.
  cd "$srcdir/gui"
  cargo build --frozen --release
}

package() {
  # --- kernel module (one per running kernel; rebuild needed after updates) ---
  _krel="$(uname -r)"
  install -dm755 "$pkgdir/usr/lib/modules/$_krel/extra"
  install -m644 "$srcdir/kernel/g5kbd.ko" \
    "$pkgdir/usr/lib/modules/$_krel/extra/g5kbd.ko"
  # --- CLI + GUI ---
  install -Dm755 "$srcdir/src/g5kbd.py" "$pkgdir/usr/bin/g5kbd"
  install -Dm755 "$srcdir/src/g5fan.py" "$pkgdir/usr/bin/g5fan"
  install -Dm755 "$srcdir/gui/target/release/g5kbd-gui" \
    "$pkgdir/usr/bin/g5kbd-gui"
  # --- udev + systemd ---
  install -Dm644 "$srcdir/kernel/99-g5kbd.rules" \
    "$pkgdir/usr/lib/udev/rules.d/99-g5kbd.rules"
  install -Dm644 "$srcdir/systemd/g5kbd-ec.conf" \
    "$pkgdir/etc/modprobe.d/g5kbd.conf"
  install -Dm644 "$srcdir/systemd/g5kbd-modules-load.conf" \
    "$pkgdir/etc/modules-load.d/g5kbd.conf"
  install -Dm644 "$srcdir/systemd/g5kbd.service" \
    "$pkgdir/usr/lib/systemd/system/g5kbd.service"
  install -Dm755 "$srcdir/systemd/system-sleep/g5kbd" \
    "$pkgdir/usr/lib/systemd/system-sleep/g5kbd"
  install -Dm644 "$srcdir/systemd/g5fan-watchdog.service" \
    "$pkgdir/usr/lib/systemd/system/g5fan-watchdog.service"
  # Fan control is root-only (the EC is not group-writable), so the GUI goes
  # through polkit instead of sudo to get an authentication prompt.
  install -Dm644 "$srcdir/polkit/dev.g5kbd.fan.policy" \
    "$pkgdir/usr/share/polkit-1/actions/dev.g5kbd.fan.policy"
  # 0755, not 0700: the daemon writes as root, but `g5fan profile list` and
  # the state file are world-readable on purpose (see FAN-RESEARCH 4.1), so a
  # desktop user can look without elevating. install.sh uses the same mode.
  install -dm755 "$pkgdir/var/lib/g5fan"
  install -dm755 "$pkgdir/var/lib/g5kbd"
  # non-root control needs a group-writeable state dir (udev rule grants
  # the LED node itself to wheel)
  chgrp -R wheel "$pkgdir/var/lib/g5kbd"
  chmod 2775 "$pkgdir/var/lib/g5kbd"
  # --- docs ---
  install -Dm644 "$srcdir/README.md" "$pkgdir/usr/share/doc/g5kbd/README.md"
  install -Dm644 "$srcdir/docs/WINDOWS-RESEARCH.md" \
    "$pkgdir/usr/share/doc/g5kbd/WINDOWS-RESEARCH.md"
  install -Dm644 "$srcdir/docs/FAN-RESEARCH.md" \
    "$pkgdir/usr/share/doc/g5kbd/FAN-RESEARCH.md"
  install -Dm644 "$srcdir/docs/ROADMAP.md" \
    "$pkgdir/usr/share/doc/g5kbd/ROADMAP.md"
}
