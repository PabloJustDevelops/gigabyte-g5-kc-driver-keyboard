# g5kbd kernel module
#
# Build against the running kernel:
#   make            (auto-selects gcc or clang/LLVM)
#   sudo make install
#
# Optional DKMS install (survives kernel updates):
#   sudo dkms add -v 1.0.0 .
#   sudo dkms install -m g5kbd -v 1.0.0

obj-m := g5kbd.o
KDIR ?= /lib/modules/$(shell uname -r)/build
PWD := $(shell pwd)

# CachyOS/Arch kernels are built with clang: pass LLVM=1 (clang + ld.lld).
# Detect it so plain `make` works everywhere.
ifneq ($(shell grep -c '^CONFIG_CC_IS_CLANG=y' $(KDIR)/.config 2>/dev/null),0)
LLVM := 1
endif

all:
	$(MAKE) -C $(KDIR) M=$(PWD) modules $(if $(LLVM),LLVM=1)

clean:
	$(MAKE) -C $(KDIR) M=$(PWD) clean

install: all
	install -d /lib/modules/$(shell uname -r)/extra/g5kbd
	install -m 644 g5kbd.ko /lib/modules/$(shell uname -r)/extra/g5kbd/
	install -m 644 99-g5kbd.rules /etc/udev/rules.d/
	depmod -a
	modprobe g5kbd || true
	@echo
	@echo "Installed. Check the device with:"
	@echo "  ls -l /sys/class/leds/rgb:kbd"
	@echo "  cat /sys/class/leds/rgb:kbd/brightness"
