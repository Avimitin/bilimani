"""Read-only comparison of the documented 33/34 monitor-adjust paths.

Requires pefile and capstone. Does not load or execute either DLL.
Usage: python scripts/compare-iidx-monitor-adjust.py old33.dll new34.dll
"""
import argparse
import hashlib
import re
import struct

import capstone
import pefile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("old33")
parser.add_argument("new34")
args = parser.parse_args()
images = []
for path, expected in [
    (args.old33, "c61b6dcb8894062e56d60da8ca90053b27f129e1a8e8da5e54457aa42602397d"),
    (args.new34, "3755587f44785effe5008f74cfe7dccbdf423ad09feead48e00a90f78a42fe83"),
]:
    pe = pefile.PE(path, fast_load=True)
    assert hashlib.sha256(pe.__data__).hexdigest() == expected, "Unexpected input build"
    images.append(pe)
old, new = images
decoder = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
decoder.detail = True


def instructions(pe, rva, size):
    items = list(decoder.disasm(pe.get_data(rva, size), pe.OPTIONAL_HEADER.ImageBase + rva))
    assert sum(item.size for item in items) == size, "Incomplete decode"
    return items


def normalized(items):
    # Ignore relocation-sensitive operands, retaining constants/registers/fields.
    result = []
    for item in items:
        operands = re.sub(r"\[rip[^]]*\]", "[RIP_TARGET]", item.op_str)
        if item.group(capstone.CS_GRP_CALL) or item.group(capstone.CS_GRP_JUMP):
            operands = re.sub(r"0x[0-9a-f]+", "CODE_TARGET", operands)
        result.append((item.mnemonic, operands))
    return result


before = instructions(old, 0x8285b0, 0xcc)
after = instructions(new, 0x814ab0, 0xcc)
assert normalized(before) == normalized(after)
assert old.get_data(0x5d4aa0, 4) == new.get_data(0x5c6c90, 4) == bytes.fromhex("8b4110c3")
assert old.get_data(0x5d4b90, 4) == new.get_data(0x5c6d80, 4) == bytes.fromhex("895110c3")
assert old.get_data(0x5d4bf0, 5) == new.get_data(0x5c6de0, 5) == bytes.fromhex("ba0a000000")
assert old.get_data(0x82864f, 44) == new.get_data(0x814b4f, 44)
for pe, items in [(old, before), (new, after)]:
    division = next(item for item in items if item.mnemonic == "divss")
    target = division.address + division.size + division.disp - pe.OPTIONAL_HEADER.ImageBase
    assert struct.unpack("<f", pe.get_data(target, 4))[0] == 60.0
# Include both return paths, including the type-2/type-3 return-zero branch.
assert normalized(instructions(old, 0x946b10, 0x46)) == normalized(instructions(new, 0x92d3f0, 0x46))
print(f"PASS: {len(after)} timing-conversion instructions match after address normalization; "
      "getter/setter, raw default 10, arithmetic bytes, divisor 60.0, "
      "and calibration result branches match")
