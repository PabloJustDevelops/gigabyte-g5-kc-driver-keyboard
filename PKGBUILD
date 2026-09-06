# Maintainer: PabloJustDevelops
# Build with:  makepkg -si   (inside this repo)
pkgname=g5kbd
pkgver=0.3.0
pkgrel=1
pkgdesc="Keyboard backlight control for Gigabyte G5 (Clevo-ODM) laptops on Linux: kernel driver (CLV0001 -> rgb:kbd), CLI and Tauri v2 GUI"
arch=('x86_64')
url=""
license=('MIT' 'GPL2')
depends=('python' 'webkit2gtk-4.1' 'gtk3')
makedepends=('linux-headers' 'rust' 'cargo' 'make' 'gcc' 'clang' 'lld' 'npm')
source=("src/g5kbd.py"
        "systemd/g5kbd.service"
        "systemd/system-sleep/g5kbd"
        "systemd/g5kbd-ec.conf"
        "systemd/g5kbd-modules-load.conf"
        "kernel/g5kbd.c"
        "kernel/Makefile"
        "kernel/99-g5kbd.rules"
        "gui/package.json"
        "gui/package-lock.json"
        "gui/vite.config.ts"
        "gui/tsconfig.json"
        "gui/index.html"
        "gui/src/styles.css"
        "gui/src/main.tsx"
        "gui/src/App.tsx"
        "gui/src/components/BrightnessPanel.tsx"
        "gui/src/components/ColorPanel.tsx"
        "gui/src/components/EffectsPanel.tsx"
        "gui/src/components/KeyboardPreview.tsx"
        "gui/src/components/Section.tsx"
        "gui/src/components/Sidebar.tsx"
        "gui/src/components/TopChips.tsx"
        "gui/src/components/ui/button.tsx"
        "gui/src/components/ui/slider.tsx"
        "gui/src/components/ui/switch.tsx"
        "gui/src/hooks/useBacklight.ts"
        "gui/src/views/HomeView.tsx"
        "gui/src/views/LightingView.tsx"
        "gui/src/views/PerformanceView.tsx"
        "gui/src/views/ProfilesView.tsx"
        "gui/src/lib/api.ts"
        "gui/src/lib/cn.ts"
        "gui/src/lib/color.ts"
        "gui/src/lib/keyboard.ts"
        "gui/src/lib/types.ts"
        "gui/src-tauri/Cargo.toml"
        "gui/src-tauri/Cargo.lock"
        "gui/src-tauri/build.rs"
        "gui/src-tauri/tauri.conf.json"
        "gui/src-tauri/capabilities/default.json"
        "gui/src-tauri/icons/icon.png"
        "gui/src-tauri/icons/128x128.png"
        "gui/src-tauri/icons/32x32.png"
        "gui/src-tauri/src/main.rs"
        "gui/src-tauri/src/lib.rs"
        "README.md"
        "docs/WINDOWS-RESEARCH.md")sha256sums=('SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP'
            'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP'
            'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP'
            'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP'
            'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP'
            'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP'
            'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP'
            'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP')

build() {
  # --- kernel module against the running kernel's headers ---
  cd "$srcdir/kernel"
  make || make CC=clang LLVM=1
  # --- GUI: frontend (vite) then Rust core (tauri) ---
  cd "$srcdir/gui"
  npm install --no-audit --no-fund
  npm run tauri -- build --no-bundle
}

package() {
  # --- kernel module (one per running kernel; rebuild needed after updates) ---
  _krel="$(uname -r)"
  install -dm755 "$pkgdir/usr/lib/modules/$_krel/extra"
  install -m644 "$srcdir/kernel/g5kbd.ko" \
    "$pkgdir/usr/lib/modules/$_krel/extra/g5kbd.ko"
  # --- CLI + GUI ---
  install -Dm755 "$srcdir/src/g5kbd.py" "$pkgdir/usr/bin/g5kbd"
  install -Dm755 "$srcdir/gui/src-tauri/target/release/g5kbd-gui" \
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
  install -dm755 "$pkgdir/var/lib/g5kbd"
  # non-root control needs a group-writeable state dir (udev rule grants
  # the LED node itself to wheel)
  chgrp -R wheel "$pkgdir/var/lib/g5kbd"
  chmod 2775 "$pkgdir/var/lib/g5kbd"
  # --- docs ---
  install -Dm644 "$srcdir/README.md" "$pkgdir/usr/share/doc/g5kbd/README.md"
  install -Dm644 "$srcdir/docs/WINDOWS-RESEARCH.md" \
    "$pkgdir/usr/share/doc/g5kbd/WINDOWS-RESEARCH.md"
}
