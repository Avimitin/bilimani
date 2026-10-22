"""Verify IIDX 34 input dispatch and frame-pacing evidence, without running Windows code.

Requires pefile, capstone and unicorn. Checks the exact supplied DLL hash,
disassembles the caller chain, and emulates the native rate getter and pacing
arithmetic with synthetic inputs. Does not measure real polling or call SleepEx,
Direct3D, a device driver, or the DLL entrypoint.
"""

import argparse
import hashlib
import math
import struct
from pathlib import Path

import capstone
import pefile
import unicorn
from unicorn import x86_const as x86


SHA256 = "3755587f44785effe5008f74cfe7dccbdf423ad09feead48e00a90f78a42fe83"


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dll", type=Path)
    args = parser.parse_args()
    data = args.dll.read_bytes()
    require(hashlib.sha256(data).hexdigest() == SHA256, "Unexpected input build")
    pe = pefile.PE(data=data, fast_load=True)
    pe.parse_data_directories(directories=[3])
    base = pe.OPTIONAL_HEADER.ImageBase
    require(base == 0x180000000, "Unexpected image base")
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True

    def decode(rva, size):
        return list(md.disasm(pe.get_data(rva, size), base + rva))

    def function(rva):
        entry = next(e.struct for e in pe.DIRECTORY_ENTRY_EXCEPTION
                     if e.struct.BeginAddress == rva)
        return decode(rva, entry.EndAddress - rva)

    def target(insn):
        if insn.mnemonic == "call" and insn.operands[0].type == capstone.CS_OP_IMM:
            return insn.operands[0].imm - base
        return None

    def qword(rva):
        return struct.unpack("<Q", pe.get_data(rva, 8))[0]

    for table, expected in [(0xD9FF20, 0xA67D70), (0xDA00F8, 0xA6A7A0),
                            (0xD9BCC8, 0xA54980)]:
        require(qword(table + 24) == base + expected, f"Unexpected vtable {table:x}")
    print("PASS: manager/device slot 3 targets match the documented input chain")

    main_loop = function(0x923ED0)
    calls = [(i.address - base, target(i)) for i in main_loop if target(i) is not None]
    expected = [(0x92403C, 0xA6A600), (0x92404A, 0xA675D0), (0x9247C0, 0x95F4F0)]
    selected = [pair for pair in calls if pair[1] in {x[1] for x in expected}]
    require(selected == expected, "Unexpected input/pacing call count or order")
    for callsite in (0x92403C, 0x92404A):
        seq = decode(callsite, 0x20)
        dispatch = seq[3]
        require(dispatch.mnemonic == "call"
                and dispatch.operands[0].type == capstone.CS_OP_MEM
                and dispatch.operands[0].mem.disp == 24,
                "Getter is not followed by the expected slot 3 dispatch")
    back = next(i for i in main_loop if i.address - base == 0x924811)
    require(back.mnemonic == "je" and back.operands[0].imm == base + 0x923F35,
            "Unexpected outer loop back edge")
    print("PASS: one device update and input update per main-loop iteration, then VSYNC")

    # Enumerate direct getter calls whose immediately following call dispatches
    # slot 3. This is a bounded idiom search, not a universal indirect-call proof.
    section = next(s for s in pe.sections if s.Name.rstrip(b"\0") == b".text")
    text = section.get_data()
    sites = []
    at = -1
    while True:
        at = text.find(b"\xe8", at + 1)
        if at < 0 or at + 5 > len(text):
            break
        rva = section.VirtualAddress + at
        if rva + 5 + struct.unpack_from("<i", text, at + 1)[0] != 0xA675D0:
            continue
        following = next((i for i in decode(rva + 5, 65) if i.mnemonic == "call"), None)
        if (following and following.operands[0].type == capstone.CS_OP_MEM
                and following.operands[0].mem.disp == 24):
            sites.append(rva)
    require(sites == [0x92404A, 0x92818C, 0x9283FE], "Dispatch sites changed")
    print("PASS: identified getter/slot-3 sites are main loop and two startup paths")

    require([(i.mnemonic, i.op_str) for i in decode(0xA3D034, 0x1D)] == [
        ("mov", "dword ptr [rbp - 0x29], 2"),
        ("mov", "dword ptr [rbp - 0x25], 0x78"),
        ("mov", "dword ptr [rbp + 0x17], 3"),
        ("mov", "rax, qword ptr [rbp - 0x29]"),
        ("mov", "qword ptr [rbp + 0x1b], rax")],
        "TDJ type-3 / two-displays / 120-Hz table entry changed")
    store = decode(0x9234E0, 12)
    require([(i.mnemonic, i.op_str) for i in store] == [
        ("mov", "rax, qword ptr [rbx]"),
        ("mov", "ecx, dword ptr [rax + 0xc]"),
        ("mov", "dword ptr [rsi + 0x7e0], ecx")],
        "Selected refresh-rate store changed")
    print("PASS: TDJ table contains 120; selected display rate is stored at root+2016")

    cpu = unicorn.Uc(unicorn.UC_ARCH_X86, unicorn.UC_MODE_64)
    # Include the real singleton getter and rate getter. No external calls occur.
    for rva in (0x922000, 0xA3D000, 0x95F000):
        cpu.mem_map(base + rva, 0x1000)
        cpu.mem_write(base + rva, pe.get_data(rva, 0x1000))
    for rva in (0xB4E3000, 0xBCBE000):
        cpu.mem_map(base + rva, 0x1000)
    obj, stack, stop = 0x20000000, 0x30000000, 0x2000F000
    cpu.mem_map(obj, 0x10000)
    cpu.mem_map(stack, 0x10000)
    cpu.mem_write(base + 0xB4E3360, struct.pack("<Q", obj))

    for definition, stored_rate, expected_rate in [(3, 120, 120), (3, 144, 144), (2, 120, 60)]:
        cpu.mem_write(obj + 2000, struct.pack("<I", definition))
        cpu.mem_write(obj + 2016, struct.pack("<I", stored_rate))
        rsp = stack + 0xF008
        cpu.mem_write(rsp, struct.pack("<Q", stop))
        cpu.reg_write(x86.UC_X86_REG_RSP, rsp)
        cpu.reg_write(x86.UC_X86_REG_RCX, obj + 1992)
        cpu.emu_start(base + 0xA3D200, stop, count=100)
        require(cpu.reg_read(x86.UC_X86_REG_RIP) == stop, "Rate getter did not return")
        result = cpu.reg_read(x86.UC_X86_REG_EAX)
        require(result == expected_rate, "Unexpected native rate result")
    print("PASS: native getter returns stored TDJ rate; definition 2 forces 60")

    # Execute just the native floating-point pacing calculation, stopping before
    # its condition/SleepEx. Set synthetic QPC values in the expected stack slot.
    # Resolve the constants from RIP-relative instructions rather than guessing.
    for insn in decode(0x95F580, 0x4B):
        for op in insn.operands:
            if op.type == capstone.CS_OP_MEM and op.mem.base == capstone.x86_const.X86_REG_RIP:
                address = insn.address + insn.size + op.mem.disp
                # Globals are already mapped; literal constants may be elsewhere.
                if base <= address < base + 0x2000000:
                    page = address & ~0xFFF
                    if not any(lo <= page <= hi for lo, hi, _ in cpu.mem_regions()):
                        cpu.mem_map(page, 0x1000)
                        cpu.mem_write(page, pe.get_data(page - base, 0x1000))
    rbp = stack + 0xE000
    cpu.reg_write(x86.UC_X86_REG_RBP, rbp)
    cpu.mem_write(base + 0xBCBE9A8, struct.pack("<q", 1_000_000))
    cpu.mem_write(base + 0xBCBE9C0, struct.pack("<q", 1_000_000))
    for rate, elapsed_us in [(120, 0), (120, 2000), (120, 10000), (60, 2000)]:
        cpu.reg_write(x86.UC_X86_REG_EAX, rate)
        cpu.mem_write(rbp - 0x49, struct.pack("<q", 1_000_000 + elapsed_us))
        cpu.emu_start(base + 0x95F580, base + 0x95F5CB, count=100)
        require(cpu.reg_read(x86.UC_X86_REG_RIP) == base + 0x95F5CB,
                "Pacing arithmetic did not reach stop")
        bits = cpu.reg_read(x86.UC_X86_REG_XMM3) & ((1 << 64) - 1)
        value = struct.unpack("<d", struct.pack("<Q", bits))[0]
        expected_wait = 1000.0 / rate - elapsed_us / 1000.0 - 1.0
        require(math.isclose(value, expected_wait, abs_tol=1e-9), "Pacing formula mismatch")
    print("PASS: native arithmetic = 1000/rate - elapsed_ms - 1")
    print("Conclusion: frame-coupled input, nominal 8.333 ms at 120 Hz; no measured wall-clock bound")


if __name__ == "__main__":
    main()
