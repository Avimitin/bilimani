"""Read-only IIDX 34 results-background evidence checks.

Requires pefile and capstone. Does not load the DLL, render, or access the network.
See docs/iidx34-result-background-analysis.md for interpretation and limitations.
"""

import argparse
import hashlib
import struct
from pathlib import Path

import capstone
import pefile


SHA256 = "3755587f44785effe5008f74cfe7dccbdf423ad09feead48e00a90f78a42fe83"


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dll", type=Path)
    args = parser.parse_args()
    data = args.dll.read_bytes()
    require(hashlib.sha256(data).hexdigest() == SHA256, "Unexpected DLL SHA-256")
    pe = pefile.PE(data=data)
    base = pe.OPTIONAL_HEADER.ImageBase
    require(base == 0x180000000 and pe.FILE_HEADER.Machine == 0x8664,
            "Unexpected image base or architecture")
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True

    def u32(rva):
        return struct.unpack("<I", pe.get_data(rva, 4))[0]

    def pointer(rva):
        return struct.unpack("<Q", pe.get_data(rva, 8))[0] - base

    def table(rva, name, entries):
        descriptor = u32(pointer(rva - 8) + 12)
        encoded = name.encode() + b"\0"
        require(pe.get_data(descriptor + 16, len(encoded)) == encoded,
                f"Unexpected RTTI at {rva:x}")
        for slot, target in entries.items():
            require(pointer(rva + slot * 8) == target,
                    f"Unexpected slot {slot} at {rva:x}")

    def function(rva):
        entry = next(e.struct for e in pe.DIRECTORY_ENTRY_EXCEPTION
                     if e.struct.BeginAddress == rva)
        return list(md.disasm(pe.get_data(rva, entry.EndAddress - rva), base + rva))

    def calls(rva):
        return {i.operands[0].imm - base for i in function(rva)
                if i.mnemonic == "call" and i.operands[0].type == capstone.CS_OP_IMM}

    def callsite(rva, target):
        insn = next(md.disasm(pe.get_data(rva, 5), base + rva))
        require(insn.mnemonic == "call"
                and insn.operands[0].type == capstone.CS_OP_IMM
                and insn.operands[0].imm == base + target,
                f"Unexpected direct call at {rva:x}")

    table(0xD72950, ".?AVCStageResultScene@@",
          {16: 0x914080, 17: 0x915580, 18: 0x915B10, 27: 0x915A10})
    table(0xD70598, ".?AVStageResultDrawBg@@",
          {0: 0x906D30, 1: 0x906D60, 2: 0x906E70})
    table(0xD9B358, ".?AVAfpTextureEx@@", {})
    print("PASS: results scene, background component and dynamic texture RTTI/vtables")

    for site, target in [(0x911ED5, 0x906B40),
                         (0x906FD4, 0x81E9F0), (0x906FF7, 0x329F30),
                         (0x907007, 0x32A460), (0x90700E, 0x32A1C0),
                         (0xA4F989, 0x344B60), (0xA4F9F5, 0x344EA0),
                         (0x63DA6D, 0x329E50)]:
        callsite(site, target)
    for site, target, value in [(0x906FCA, 0xD70620, b"illust\0"),
                                (0x906FF0, 0xCA56F0, b"result_bg\0")]:
        insn = next(md.disasm(pe.get_data(site, 7), base + site))
        operand = insn.operands[1]
        require(insn.mnemonic == "lea" and operand.type == capstone.CS_OP_MEM
                and operand.mem.base == capstone.x86.X86_REG_RIP
                and insn.address + insn.size + operand.mem.disp == base + target,
                f"Unexpected string reference at {site:x}")
        require(pe.get_data(target, len(value)) == value, "Unexpected string")
    require(pe.get_data(0x911ECE, 7) == bytes.fromhex("488d8e08010000"),
            "Background no longer at scene +264")
    require(pe.get_data(0x906FE1, 7) == bytes.fromhex("498b9510010000"),
            "Archive resource no longer at background +272")
    print("PASS: scene owns background; illust binds result_bg or is hidden")

    require({0x620AD0, 0x818EE0} <= calls(0x906B40),
            "Background archive construction/loading path changed")
    require({0xA4FD40, 0x329E50} <= calls(0x63D9E0),
            "Existing descriptor binding example changed")
    require({0x344B60, 0x344EA0} <= calls(0xA4F8A0),
            "Texture wrapper no longer registers and builds a descriptor")
    require(0x344C30 in calls(0xA4FA30),
            "Texture destructor no longer unregisters its handle")
    # This leaf has no unwind/pdata entry; decode its known five-byte body.
    getter = list(md.disasm(pe.get_data(0xA4FD40, 5), base + 0xA4FD40))
    require(getter[0].mnemonic == "lea"
            and getter[0].operands[1].type == capstone.CS_OP_MEM
            and getter[0].operands[1].mem.disp == 24
            and getter[1].mnemonic == "ret", "Descriptor getter changed")
    for rva in (0x329E50, 0x329F30):
        require(any(i.mnemonic == "call"
                    and i.operands[0].type == capstone.CS_OP_MEM
                    and i.operands[0].mem.base == capstone.x86.X86_REG_RIP
                    and i.address + i.size + i.operands[0].mem.disp == base + 0xC66CA8
                    for i in function(rva)), "AFP texture binding import changed")
    print("PASS: archive and dynamic-descriptor setters share afp_core_137")
    print("Offline evidence only: no hook, GPU upload, layout or frame-time validation.")


if __name__ == "__main__":
    main()
