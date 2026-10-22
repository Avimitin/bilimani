"""Read-only verification of the IIDX 34 file evidence. Python stdlib only.

Usage: python scripts/verify-iidx34.py path/to/bm2dx.dll
Does not load or execute the DLL. See docs/iidx34-analysis.md and
docs/iidx34-0100917-analysis.md for ABI reasoning.
"""
import argparse
import hashlib
import pathlib
import re
import struct

PROFILES = {
    "3755587f44785effe5008f74cfe7dccbdf423ad09feead48e00a90f78a42fe83": {
        "label": "IIDX 34 / 010-0916",
        "dir": "b010_0916",
        "database": (0x939100, "488d056937cd0ac3"),
        "input": (0xa67d70, "48894c24085553565741544155415641"),
        "select": {16: 0x8d7b70, 17: 0x8d7f60, 18: 0x8d8250},
        "stage": (0x91a2c0, 0x91a330, 0x91a390),
        "factory_stage": (0x8bea40, 0x91a330, 0x8beac0),
        "dictionary": 0x7dc590,
    },
    "c98e183ec81ecb9e1cb13c237f9c9cb0b3f65b6fdaff6fd5e6a526008c546534": {
        "label": "IIDX 34 / 010-0917",
        "dir": "b010_0917",
        "database": (0x939140, "488d052937cd0ac3"),
        "input": (0xa67db0, "48894c24085553565741544155415641"),
        "select": {16: 0x8d7bb0, 17: 0x8d7fa0, 18: 0x8d8290},
        "stage": (0x91a300, 0x91a370, 0x91a3d0),
        "factory_stage": (0x8bea80, 0x91a370, 0x8beb00),
        "dictionary": 0x7dc5d0,
    },
}

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("dll", type=pathlib.Path)
args = parser.parse_args()
data = args.dll.read_bytes()
digest = hashlib.sha256(data).hexdigest()
if digest not in PROFILES:
    raise SystemExit(f"Unsupported file SHA-256: {digest}")
config = PROFILES[digest]
root = pathlib.Path(__file__).resolve().parent.parent
profile = root / "crates/bilimani-iidx/src/versions/v34" / config["dir"]
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
    for rva, prefix in re.findall(
        r'\(\s*0x([0-9a-f]+),\s*"([0-9a-f]{32})"\s*\)', source.read_text()
    ):
        rva, prefix = int(rva, 16), bytes.fromhex(prefix)
        assert rva not in guards or guards[rva] == prefix
        guards[rva] = prefix
for rva, prefix in guards.items():
    assert read(rva, len(prefix)) == prefix, f"Guard mismatch: {rva:x}"
assert len(guards) >= 40, f"Guard extraction unexpectedly small: {len(guards)}"
database_rva, database_hex = config["database"]
assert read(database_rva, len(bytes.fromhex(database_hex))) == bytes.fromhex(database_hex)
input_rva, input_hex = config["input"]
assert read(input_rva, len(bytes.fromhex(input_hex))) == bytes.fromhex(input_hex)
vtable(0xd52508, ".?AVCMusicSelectScene@@", config["select"])
stages = {
    0xd73238: "Dan", 0xd73338: "Expert", 0xd73438: "Free",
    0xd73538: "Hazard", 0xd73638: "StepUp", 0xd73738: "PremiumFree",
    0xd73838: "Standard", 0xd73938: "Tutorial", 0xd7b470: "FreePlus",
    0xd7b570: "Hell", 0xd7b670: "Arena", 0xd7b870: "BPLBattle",
    0xd7bb48: "FactoryCheck",
}
for table, mode in stages.items():
    factory = mode == "FactoryCheck"
    targets = config["factory_stage"] if factory else config["stage"]
    vtable(table, f".?AVC{mode}StageScene@@", {
        16: targets[0], 17: targets[1], 18: targets[2],
    })
assert pointer(0xcbe910 + 8) == config["dictionary"]
assert pointer(0xd9ff20 + 3 * 8) == config["input"][0]
print(f"PASS: {config['label']} SHA-256, x64 PE, {len(guards)} native prefixes, "
      "database/input getters, 14 scene RTTI/lifecycle tables, dictionary and input slots")
