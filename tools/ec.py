#!/usr/bin/env python3
"""
Minimal Embedded Controller (EC) RAM access through the kernel's ec_sys
debugfs interface (/sys/kernel/debug/ec/ec*/io).

Why this interface: the kernel ACPI EC driver serialises every access, which
is the safe way to touch the EC. Writes require the ec_sys module to have
been loaded with write_support=1 (see probe-kb.sh).

Usage:
  ec.py dump [START [LEN]]    hex dump of EC RAM (START/LEN in hex, default 0 0x100)
  ec.py rd  OFFSET            read one byte (hex)
  ec.py wr  OFFSET VALUE      write one byte (hex)
  ec.py cmd DOORBELL SUB [P1 [P2 [P3]]]   mailbox command (values hex)

The G5/G6 (Clevo-ODM) mailbox lives at EC RAM 0xF8..0xFD:
  F8 FCMD  doorbell  (written LAST, triggers execution)
  F9 FDAT  sub-command / first parameter
  FA FBUF  parameter 1
  FB FBF1  parameter 2
  FC FBF2  parameter 3
  FD FBF3  parameter 4
"""
import glob
import os
import sys

EC_SYS_GLOB = "/sys/kernel/debug/ec/ec*/io"


def find_io() -> str:
    cands = sorted(glob.glob(EC_SYS_GLOB))
    if not cands:
        sys.exit(
            "error: no %s found.\n"
            "Load the module first (as root):\n"
            "    modprobe -r ec_sys; modprobe ec_sys write_support=1"
            % EC_SYS_GLOB
        )
    return cands[0]


class Ec:
    def __init__(self, path: str):
        self.path = path
        self.writable = os.access(path, os.W_OK)

    def read(self, off: int) -> int:
        with open(self.path, "rb", buffering=0) as f:
            f.seek(off)
            b = f.read(1)
        if len(b) != 1:
            raise IOError("short read at 0x%02x" % off)
        return b[0]

    def write(self, off: int, val: int) -> None:
        if not self.writable:
            sys.exit(
                "error: %s is read-only. Reload with write support:\n"
                "    modprobe -r ec_sys && modprobe ec_sys write_support=1"
                % self.path
            )
        with open(self.path, "r+b", buffering=0) as f:
            f.seek(off)
            f.write(bytes([val & 0xFF]))

    def dump(self, start: int, length: int) -> None:
        print("EC RAM 0x%02x..0x%02x  (path: %s)" % (start, start + length - 1, self.path))
        for base in range(start, start + length, 16):
            row = []
            asc = []
            for i in range(16):
                off = base + i
                if off < start + length:
                    v = self.read(off)
                    row.append("%02x" % v)
                    asc.append(chr(v) if 32 <= v < 127 else ".")
                else:
                    row.append("  ")
                    asc.append(" ")
            print("%04x  %s  %s" % (base, " ".join(row), "".join(asc)))

    def command(self, doorbell: int, sub: int, params=()) -> None:
        """Mailbox command: FDAT=sub, FBUF.. = params, then doorbell to FCMD."""
        regs = (0xFA, 0xFB, 0xFC, 0xFD)
        if len(params) > len(regs):
            sys.exit("too many params")
        self.write(0xF9, sub)
        for reg, val in zip(regs, params):
            self.write(reg, val)
        self.write(0xF8, doorbell)


def hx(s: str) -> int:
    return int(s, 16)


def main(argv) -> int:
    ec = Ec(find_io())
    cmd = argv[1] if len(argv) > 1 else "dump"

    if cmd == "dump":
        start = hx(argv[2]) if len(argv) > 2 else 0x00
        length = hx(argv[3]) if len(argv) > 3 else 0x100
        ec.dump(start, length)
    elif cmd == "rd":
        ec.dump(hx(argv[2]), 1)
        print("value at 0x%02x = 0x%02x" % (hx(argv[2]), ec.read(hx(argv[2]))))
    elif cmd == "wr":
        off, val = hx(argv[2]), hx(argv[3])
        ec.write(off, val)
        print("wrote 0x%02x -> 0x%02x" % (off, val))
    elif cmd == "cmd":
        doorbell, sub = hx(argv[2]), hx(argv[3])
        params = [hx(a) for a in argv[4:]]
        ec.command(doorbell, sub, params)
        print("mailbox: doorbell 0x%02x sub 0x%02x params %s"
              % (doorbell, sub, ["0x%02x" % p for p in params]))
    else:
        sys.exit("unknown command: %s" % cmd)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
