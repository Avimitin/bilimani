"""Read-only verification of the 34/20260917 file evidence. Python stdlib only.

Usage: python scripts/verify-iidx34.py path/to/bm2dx.dll
Does not load or execute the DLL. See docs/iidx34-analysis.md for ABI reasoning.
"""
import argparse
import hashlib
import pathlib
import re
import struct

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("dll", type=pathlib.Path)
args = parser.parse_args()
data = args.dll.read_bytes()
root = pathlib.Path(__file__).resolve().parent.parent
profile = root / "crates/bilimani-iidx/src/versions/v34/b20260917"
expected = "3755587f44785effe5008f74cfe7dccbdf423ad09feead48e00a90f78a42fe83"
assert hashlib.sha256(data).hexdigest() == expected, "Unsupported file SHA-256"
assert data[:2] == b"MZ"
pe = struct.unpack_from("<I", data, 0x3c)[0]
assert data[pe:pe+4] == b"PE\0\0"
machine, count = struct.unpack_from("<HH", data, pe + 4)
assert machine == 0x8664
optional_size = struct.unpack_from("<H", data, pe + 20)[0]
optional = pe + 24
assert struct.unpack_from("<H", data, optional)[0] == 0x20b
base = struct.unpack_from("<Q", data, optional + 24)[0]
assert base == 0x180000000
sections = []
for index in range(count):
    at = optional + optional_size + index * 40
    virtual_size, rva, raw_size, raw = struct.unpack_from("<IIII", data, at + 8)
    sections.append((rva, virtual_size, raw, raw_size))


def read(rva, size):
    for start, virtual_size, raw, raw_size in sections:
        if start <= rva and rva + size <= start + raw_size:
            return data[raw + rva - start:raw + rva - start + size]
    raise AssertionError(f"RVA {rva:x}+{size:x} not file-backed")


def u32(rva):
    return struct.unpack("<I", read(rva, 4))[0]


def pointer(rva):
    return struct.unpack("<Q", read(rva, 8))[0] - base


def vtable(rva, name, slots):
    # MSVC x64 complete-object locator -> RVA of TypeDescriptor -> decorated name.
    locator = pointer(rva - 8)
    descriptor = u32(locator + 12)
    actual = read(descriptor + 16, len(name) + 1)
    assert actual == name.encode() + b"\0", (hex(rva), actual, name)
    for slot, target in slots.items():
        assert pointer(rva + slot * 8) == target, (hex(rva), slot)


guards = {}
for source in profile.glob("*.rs"):
    for rva, prefix in re.findall(r'\(\s*0x([0-9a-f]+),\s*"([0-9a-f]{32})"\s*\)', source.read_text()):
        rva, prefix = int(rva, 16), bytes.fromhex(prefix)
        assert rva not in guards or guards[rva] == prefix
        guards[rva] = prefix
for rva, prefix in guards.items():
    assert read(rva, len(prefix)) == prefix, f"Guard mismatch: {rva:x}"
assert len(guards) >= 40, f"Guard extraction unexpectedly small: {len(guards)}"
assert read(0x939100, 8) == bytes.fromhex("488d056937cd0ac3")
assert read(0xa67d70, 16) == bytes.fromhex("48894c24085553565741544155415641")
vtable(0xd52508, ".?AVCMusicSelectScene@@", {16: 0x8d7b70, 17: 0x8d7f60, 18: 0x8d8250})
stages = {
    0xd73238: "Dan", 0xd73338: "Expert", 0xd73438: "Free",
    0xd73538: "Hazard", 0xd73638: "StepUp", 0xd73738: "PremiumFree",
    0xd73838: "Standard", 0xd73938: "Tutorial", 0xd7b470: "FreePlus",
    0xd7b570: "Hell", 0xd7b670: "Arena", 0xd7b870: "BPLBattle",
    0xd7bb48: "FactoryCheck",
}
for table, mode in stages.items():
    factory = mode == "FactoryCheck"
    vtable(table, f".?AVC{mode}StageScene@@", {
        16: 0x8bea40 if factory else 0x91a2c0,
        17: 0x91a330,
        18: 0x8beac0 if factory else 0x91a390,
    })
assert pointer(0xcbe910 + 8) == 0x7dc590
assert pointer(0xd9ff20 + 3 * 8) == 0xa67d70
print(f"PASS: SHA-256, x64 PE, {len(guards)} native prefixes, database/input getters, "
      "14 scene RTTI/lifecycle tables, dictionary and input slots")
