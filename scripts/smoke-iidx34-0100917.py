"""Attach the release IIDX DLL to the 010-0917 game image without running it.

Maps the supplied bm2dx.dll with DONT_RESOLVE_DLL_REFERENCES, loads the release
bilimani_iidx.dll and checks fingerprint selection, every native guard, vtable
hook installation and worker startup. No game entry point, scene callback,
network request or platform connection is executed.
"""
import argparse
import ctypes
import json
import pathlib
import shutil
import socket
import struct
import time
import uuid

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--game-dll", type=pathlib.Path, required=True)
args = parser.parse_args()

root = pathlib.Path(__file__).resolve().parent.parent
work = root / "analysis" / ("smoke-iidx34-0100917-" + uuid.uuid4().hex)
work.mkdir(parents=True)
shutil.copy2(root / "target/release/bilimani_iidx.dll", work / "bilimani_iidx.dll")
shutil.copy2(args.game_dll, work / "bm2dx.dll")

with socket.socket() as probe:
    probe.bind(("127.0.0.1", 0))
    port = probe.getsockname()[1]

config = {
    "listen": f"127.0.0.1:{port}",
    "static_root": str(root / "dist/web"),
    "module": "bm2dx.dll",
    "database_path": "",
    "aliases_path": "bilimani_iidx_aliases.csv",
    "queue": {"capacity": 20, "current_timeout_seconds": 600},
    "controls": {"enabled": True, "double_tap_ms": 400},
}
(work / "bilimani_iidx.json").write_text(json.dumps(config), encoding="utf-8")

kernel = ctypes.WinDLL("kernel32", use_last_error=True)
kernel.LoadLibraryExW.argtypes = [ctypes.c_wchar_p, ctypes.c_void_p, ctypes.c_uint32]
kernel.LoadLibraryExW.restype = ctypes.c_void_p
game = kernel.LoadLibraryExW(str(work / "bm2dx.dll"), None, 1)
assert game, ctypes.WinError(ctypes.get_last_error())

plugin = ctypes.CDLL(str(work / "bilimani_iidx.dll"))
plugin.bilimani_shutdown.argtypes = []
plugin.bilimani_shutdown.restype = None

logfile = work / "bilimani_iidx.log"
deadline = time.monotonic() + 20
while time.monotonic() < deadline:
    text = logfile.read_text(encoding="utf-8") if logfile.exists() else ""
    if "Startup failed:" in text or "select_hooks_intact=true" in text:
        break
    time.sleep(0.05)
else:
    raise AssertionError("DLL worker did not initialize within 20 seconds")

assert "Startup failed:" not in text, text
assert "Native hooks installed for the verified IIDX 34 build" in text, text
assert "IIDX 34 / 010-0917 API:" in text, text
assert "select_hooks_intact=true" in text, text
assert "Player sampling enabled" in text, text

slots = [
    (0xD52508, 16, 0x8D7BB0),
    (0xD52508, 17, 0x8D7FA0),
    (0xD52508, 18, 0x8D8290),
    (0xCBE910, 1, 0x7DC5D0),
    (0xD9FF20, 3, 0xA67DB0),
    (0xD73238, 17, 0x91A370),
    (0xD73238, 18, 0x91A3D0),
    (0xD7BB48, 18, 0x8BEB00),
]
for table, slot, original in slots:
    hooked = ctypes.c_void_p.from_address(game + table + slot * 8).value
    assert hooked != game + original, (hex(table), slot)
    assert abs(hooked - plugin._handle) < 0x4000000, ("outside plugin", hex(table), slot)

plugin.bilimani_shutdown()
time.sleep(0.5)
print(f"PASS: IIDX 34 / 010-0917 fingerprint, native guards, vtable hooks and shutdown ({work.name})")
