"""Verify the IIDX 34 monitor-validation bypass and optionally merge a patch JSON.

Requires pefile, capstone and unicorn. The DLL is read only. This verifies the
complete hash, PE identity, instruction bytes and branch behavior. An optional
base JSON retains other options and replaces the equivalent upstream monitor
bypass, avoiding overlapping duplicate options. Existing output is not replaced.
"""

import argparse
import copy
import hashlib
import json
from pathlib import Path

import capstone
import pefile
import unicorn
from unicorn import x86_const as x86


ROOT = Path(__file__).resolve().parents[1]
IDENTIFIER = "LDJ-6aa23d57_b02b3c"
SOURCE = ROOT / "patches" / "iidx34" / f"{IDENTIFIER}.json"
SHA256 = "3755587f44785effe5008f74cfe7dccbdf423ad09feead48e00a90f78a42fe83"
OFFSET = 9544808
RVA = 0x91B068


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dll", type=Path)
    parser.add_argument("--base-json", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    raw = args.dll.read_bytes()
    require(hashlib.sha256(raw).hexdigest() == SHA256, "Unexpected DLL hash")
    pe = pefile.PE(data=raw, fast_load=True)
    identifier = f"LDJ-{pe.FILE_HEADER.TimeDateStamp:x}_{pe.OPTIONAL_HEADER.AddressOfEntryPoint:x}"
    require(identifier == IDENTIFIER, "Unexpected PE identity")
    require(pe.get_rva_from_offset(OFFSET) == RVA, "File-offset/RVA mapping changed")
    source = json.loads(SOURCE.read_text(encoding="utf-8"))
    option = source[1]
    patch = option["patches"][0]
    require(option["peIdentifier"] == identifier and option["type"] == "memory",
            "Patch metadata mismatch")
    require(patch["offset"] == OFFSET and patch["dllName"] == "bm2dx.dll",
            "Patch destination mismatch")
    before, after = (bytes.fromhex(patch[k]) for k in ("dataDisabled", "dataEnabled"))
    require(raw[OFFSET:OFFSET + len(before)] == before, "Original bytes mismatch")
    require(len(before) == len(after) == 6, "Unexpected patch length")
    base = pe.OPTIONAL_HEADER.ImageBase
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    original = list(md.disasm(before, base + RVA))
    changed = list(md.disasm(after, base + RVA))
    target = base + 0x91B1AE
    require(len(original) == 1 and original[0].mnemonic == "jne"
            and original[0].operands[0].imm == target, "Original branch mismatch")
    require([i.mnemonic for i in changed] == ["nop", "jmp"]
            and changed[1].operands[0].imm == target, "Patched branch target mismatch")

    for enabled, code in [(False, before), (True, after)]:
        for equal in (False, True):
            cpu = unicorn.Uc(unicorn.UC_ARCH_X86, unicorn.UC_MODE_64)
            cpu.mem_map(base + 0x91B000, 0x1000)
            cpu.mem_write(base + RVA, code)
            cpu.reg_write(x86.UC_X86_REG_EFLAGS, 2 | (0x40 if equal else 0))
            cpu.emu_start(base + RVA, base + 0x91BFFF, count=2 if enabled else 1)
            actual = cpu.reg_read(x86.UC_X86_REG_RIP)
            expected = target if enabled or not equal else base + RVA + 6
            require(actual == expected, "Native branch behavior mismatch")
    print("PASS: hash, identifier, file offset, full instruction guard and four branch cases")
    print("Behavior: bypass validation/retries/error; retain initial measurement and FPS result")

    output = copy.deepcopy(source)
    if args.base_json:
        require(args.base_json.name == f"{identifier}.json", "Base filename has a different PE identity")
        output = json.loads(args.base_json.read_text(encoding="utf-8-sig"))
        require(isinstance(output, list), "Base JSON must be an array")
        replaced = False
        for i, entry in enumerate(output):
            if entry.get("name") in (
                "Bypass Lightning Monitor Error",
                "Disable Monitor Check (TDJ validation)",
            ):
                require(not replaced, "Duplicate monitor options in base JSON")
                edits = entry.get("patches", [])
                require(len(edits) == 1 and edits[0].get("offset") == OFFSET
                        and edits[0].get("dllName") == "bm2dx.dll"
                        and edits[0].get("dataDisabled") in ("0F85", before.hex().upper())
                        and edits[0].get("dataEnabled") in ("90E9", after.hex().upper()),
                        "Base monitor option is not equivalent to the verified bypass")
                output[i] = copy.deepcopy(option)
                replaced = True
        if not replaced:
            output.append(copy.deepcopy(option))
        if output and "name" not in output[0]:
            output[0]["localModification"] = "Verified TDJ validation bypass with full instruction guard"
            output[0]["verifiedMonitorPatchSha256"] = SHA256
        print(f"Prepared {sum('name' in item for item in output)} options; other entries retained")

    if args.output:
        require(args.output.name == f"{identifier}.json", "Output needs the exact PE filename")
        require(not args.output.exists(), "Output already exists; choose a new output directory")
        args.output.parent.mkdir(parents=True, exist_ok=True)
        with args.output.open("x", encoding="utf-8", newline="\n") as handle:
            json.dump(output, handle, ensure_ascii=False, indent=4)
            handle.write("\n")
        print(f"Created: {args.output}")
    require(hashlib.sha256(args.dll.read_bytes()).hexdigest() == SHA256, "DLL changed during verification")


if __name__ == "__main__":
    main()
