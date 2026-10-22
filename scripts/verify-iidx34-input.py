"""Exercise the supplied IIDX 34 TDJ input decoder in an isolated CPU emulator.

Requires pefile and unicorn. Does not load the DLL into Windows, contact hardware,
or modify the file. Only the decoder and its button-query instructions execute;
device refresh and the compiler's security-cookie check are replaced with RET
stubs in emulator memory. This is a behavior experiment, not a latency benchmark.

Usage: python scripts/verify-iidx34-input.py path/to/bm2dx.dll
"""

import argparse
import hashlib
import struct
from pathlib import Path

import pefile
import unicorn
from unicorn import x86_const as x86


EXPECTED_SHA256 = "3755587f44785effe5008f74cfe7dccbdf423ad09feead48e00a90f78a42fe83"
UPDATE = 0xA54980
PRESSED = 0xA53A10
HELD = 0xA53930
RELEASED = 0xA53950
OBJECT = 0x20000000
VTABLE = OBJECT + 0x2000
STUB = OBJECT + 0x3000
STOP = STUB + 0x10
STACK = 0x30000000


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


class Decoder:
    def __init__(self, pe):
        self.base = pe.OPTIONAL_HEADER.ImageBase
        self.cpu = unicorn.Uc(unicorn.UC_ARCH_X86, unicorn.UC_MODE_64)
        # Decoder and query functions, with no Windows loader or DLL entrypoint.
        for rva, size in [(0xA53000, 0x2000), (0x1055000, 0x1000)]:
            self.cpu.mem_map(self.base + rva, size)
            self.cpu.mem_write(self.base + rva, pe.get_data(rva, size))
        # Compiler cookie-check stub; it is outside the input algorithm.
        self.cpu.mem_map(self.base + 0xB01000, 0x1000)
        self.cpu.mem_write(self.base + 0xB01AC0, b"\xc3")
        self.cpu.mem_map(OBJECT, 0x10000)
        self.cpu.mem_map(STACK, 0x10000)
        self.cpu.mem_write(OBJECT + 24, struct.pack("<Q", VTABLE))
        self.cpu.mem_write(VTABLE + 24, struct.pack("<Q", STUB))
        self.cpu.mem_write(STUB, b"\xc3")
        # Zero native repeat-suppression threshold to isolate status decoding.
        self.cpu.mem_write(OBJECT + 3524, struct.pack("<i", 0))

    def call(self, rva, key=12):
        rsp = STACK + 0xF008  # Windows x64 entry alignment and shadow space.
        self.cpu.mem_write(rsp, struct.pack("<Q", STOP))
        self.cpu.reg_write(x86.UC_X86_REG_RSP, rsp)
        self.cpu.reg_write(x86.UC_X86_REG_RCX, OBJECT)
        self.cpu.reg_write(x86.UC_X86_REG_RDX, key)
        self.cpu.emu_start(self.base + rva, STOP, count=10000)
        require(self.cpu.reg_read(x86.UC_X86_REG_RIP) == STOP,
                f"Instruction limit reached at RVA {rva:x}")
        return self.cpu.reg_read(x86.UC_X86_REG_RAX) & 0xFF

    def feed(self, counter=0, current=(), history=()):
        require(len(history) <= 33, "History exceeds observed 33-entry capacity")
        status = bytearray(202)
        status[3] = counter
        for key in current:  # 0..6 = P1; 7..13 = P2.
            status[27 + key] = 0xFF
        for i, (p1, p2) in enumerate(history):
            status[136 + 2 * i:138 + 2 * i] = bytes((p1, p2))
        self.cpu.mem_write(OBJECT + 72, bytes(status))
        self.call(UPDATE)
        return {
            "history_count": struct.unpack("<I", self.cpu.mem_read(OBJECT + 320, 4))[0],
            "pressed": [self.call(PRESSED, 12 + k) for k in range(14)],
            "held": [self.call(HELD, 12 + k) for k in range(14)],
            "released": [self.call(RELEASED, 12 + k) for k in range(14)],
        }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dll", type=Path)
    args = parser.parse_args()
    data = args.dll.read_bytes()
    require(hashlib.sha256(data).hexdigest() == EXPECTED_SHA256, "Unexpected input build")
    pe = pefile.PE(data=data, fast_load=True)
    require(pe.OPTIONAL_HEADER.ImageBase == 0x180000000, "Unexpected image base")
    print(f"Input SHA-256: {EXPECTED_SHA256}")
    print(f"Unicorn {unicorn.__version__}; controlled buffers, no physical timing measurements")

    snapshot = Decoder(pe).feed()
    require(not any(snapshot["pressed"]), "Empty snapshot produced a press")
    print("PASS: snapshot released, no history -> no press")

    history_decoder = Decoder(pe)
    first = history_decoder.feed(counter=1, history=[(1, 0)])
    require(first["pressed"][0] == first["held"][0] == 1, "History-only press lost")
    second = history_decoder.feed(counter=1, history=[(1, 0)])
    require(second["history_count"] == 0 and second["released"][0] == 1,
            "Unchanged counter incorrectly replayed history")
    print("PASS: history-only P1 key 1 -> press; unchanged counter next poll -> release")

    for k in range(14):
        masks = (1 << k, 0) if k < 7 else (0, 1 << (k - 7))
        r = Decoder(pe).feed(counter=1, history=[masks])
        require(r["pressed"] == [int(i == k) for i in range(14)], "Incorrect key mapping")
    print("PASS: all 14 history key bits map to the expected key queries")

    for slot in (0, 16, 32):
        samples = [(0, 0)] * 33
        samples[slot] = (1, 0)
        r = Decoder(pe).feed(counter=33, history=samples)
        require(r["history_count"] == 33 and r["pressed"][0] == 1,
                "History sample position changed observed press")
    print("PASS: first/middle/last buffer positions produce the same press state")

    repeat = Decoder(pe).feed(counter=3, history=[(1, 0), (0, 0), (1, 0)])
    require(repeat["pressed"][0] == 1, "Multiple samples not OR-combined")
    print("PASS: down/up/down history becomes one pressed state, not two events")

    release_decoder = Decoder(pe)
    release_decoder.feed(current=[0])
    r = release_decoder.feed(counter=1, history=[(1, 0)])
    require(r["held"][0] == 1 and r["released"][0] == 0 and r["pressed"][0] == 0,
            "History did not retain the prior held state")
    print("PASS: history can also retain a held state after the current byte is released")

    wrap_decoder = Decoder(pe)
    wrap_decoder.cpu.mem_write(OBJECT + 316, bytes([255]))
    r = wrap_decoder.feed(counter=0, history=[(1, 0)])
    require(r["history_count"] == 1 and r["pressed"][0] == 1, "Counter wrap failed")
    r = Decoder(pe).feed(counter=100, history=[(0, 0)] * 32 + [(1, 0)])
    require(r["history_count"] == 33 and r["pressed"][0] == 1, "History cap failed")
    print("PASS: 255 -> 0 counter wrap reads one sample; large delta caps at 33")


if __name__ == "__main__":
    main()
