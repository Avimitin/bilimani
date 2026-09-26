"""Read-only verification of the supported game image. No third-party Python packages."""
import hashlib
import pathlib
import struct
import sys

binary = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "analysis/bm2dx.dll")
data = binary.read_bytes()
digest = hashlib.sha256(data).hexdigest()
assert digest == "c61b6dcb8894062e56d60da8ca90053b27f129e1a8e8da5e54457aa42602397d", "Unsupported image"
pe = struct.unpack_from("<I", data, 0x3C)[0]
assert data[pe:pe+4] == b"PE\0\0"
assert struct.unpack_from("<H", data, pe+4)[0] == 0x8664
count = struct.unpack_from("<H", data, pe+6)[0]
optional_size = struct.unpack_from("<H", data, pe+20)[0]
image_base = struct.unpack_from("<Q", data, pe+24+24)[0]
sections = []
for i in range(count):
    start = pe + 24 + optional_size + i*40
    size, rva, raw_size, raw = struct.unpack_from("<IIII", data, start+8)
    sections.append((rva, raw_size, raw))

def read(rva, size):
    for base, length, raw in sections:
        if base <= rva and rva+size <= base+length:
            return data[raw+rva-base:raw+rva-base+size]
    raise ValueError(f"RVA {rva:x} is not file backed")

guards = {
    0x7d60e0: "4883ec288b051e2f010aa801755483c8",
    0x7d6150: "80790800741089511044894114448949",
    0x7d5eb0: "4c894c2420534154415641574883ec48",
    0x82ded0: "e95bb61100cccccccccccccccccccccc",
    0x606fd0: "48895c2408574883ec20488bd9b9e803",
    0x607030: "40534883ec204881c1280300008bdae8",
    0x606e60: "4883ec284881c128030000e880cdffff",
    0x949230: "4883ec28e84702000083f801751533c9",
    0x806f60: "4883ec28e8f7feffff85c07517e84eff",
}
for rva, expected in guards.items():
    assert read(rva, 16).hex() == expected, f"Function guard mismatch at {rva:x}"
for slot, expected in [(13, 0x8eb820), (14, 0x8ebeb0), (15, 0x8ec1f0)]:
    assert struct.unpack("<Q", read(0xd84788+slot*8, 8))[0] == image_base+expected
print(f"Verified x64 image, SHA-256, {len(guards)} entry-point guards and selection vtable slots.")
