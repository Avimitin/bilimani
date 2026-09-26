"""Exercise real DLL startup/shutdown without executing game code or networking.

Usage: py scripts/smoke-dll.py [--reject]
Requires the built release DLL and the ignored local game copy for positive mode.
Artifacts remain under analysis/smoke-* for inspection.
"""
import ctypes
import pathlib
import shutil
import struct
import sys
import time
import uuid

root = pathlib.Path(__file__).resolve().parent.parent
reject = "--reject" in sys.argv
work = root / "analysis" / ("smoke-" + uuid.uuid4().hex)
work.mkdir(parents=True)
dll_path = work / "chart_requester.dll"
shutil.copy2(root / "target/release/chart_requester.dll", dll_path)
config = (root / "chart-requester.example.toml").read_text(encoding="utf-8")
config = config.replace("enabled = true", "enabled = false")
if reject:
    config = config.replace('module = "bm2dx.dll"', 'module = "kernel32.dll"')
(work / "chart-requester.toml").write_text(config, encoding="utf-8")

kernel = ctypes.WinDLL("kernel32", use_last_error=True)
kernel.LoadLibraryExW.argtypes = [ctypes.c_wchar_p, ctypes.c_void_p, ctypes.c_uint32]
kernel.LoadLibraryExW.restype = ctypes.c_void_p
if not reject:
    # Map/relocate only. DONT_RESOLVE_DLL_REFERENCES prevents imports, TLS
    # callbacks and the game entry point from running. Never call game exports.
    game = kernel.LoadLibraryExW(str(root / "analysis/bm2dx.dll"), None, 1)
    assert game, ctypes.WinError(ctypes.get_last_error())
plugin = ctypes.CDLL(str(dll_path))
plugin.chart_requester_shutdown.argtypes = []
plugin.chart_requester_shutdown.restype = None

deadline = time.monotonic() + 20
logfile = work / "chart-requester.log"
while time.monotonic() < deadline:
    text = logfile.read_text(encoding="utf-8") if logfile.exists() else ""
    if "Startup failed" in text or ("Native hooks installed" in text and "[status]" in text):
        break
    time.sleep(0.05)
else:
    raise AssertionError("DLL worker did not initialize within 20 seconds")
if reject:
    assert "Unsupported bm2dx.dll build" in text, text
    assert "Native hooks installed" not in text
    assert "Unsupported bm2dx.dll build" in (work / "obs/interaction.txt").read_text(encoding="utf-8")
else:
    assert "Native hooks installed" in text, text
    assert "[status]" in text and "select_updates=0" in text, text
    assert "select_hooks_intact=true" in text, text
    for slot, rva in [(13, 0x8eb820), (14, 0x8ebeb0), (15, 0x8ec1f0)]:
        hooked = ctypes.c_void_p.from_address(game + 0xd84788 + slot*8).value
        assert hooked != game+rva, f"Slot {slot} was not patched"
        assert abs(hooked-plugin._handle) < 0x4000000, "Hook is not inside plugin image"

# Simulate Spice's documented SDK initialization/shutdown callback ABI.
destroy_callback = None
INIT = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p)

@INIT
def sdk_init(version, destroy, api):
    global destroy_callback
    assert version == 0
    assert ctypes.c_uint32.from_address(api).value == 112
    destroy_callback = ctypes.CFUNCTYPE(None)(destroy)
    return 0

plugin.spice_sdk_entry_point.argtypes = [INIT]
plugin.spice_sdk_entry_point.restype = ctypes.c_int
assert plugin.spice_sdk_entry_point(sdk_init) == 0
assert destroy_callback
destroy_callback()
if not reject:
    assert (work / "obs/queue.txt").read_text(encoding="utf-8").strip() == "点歌已停止"
print("PASS: " + ("unsupported image rejected" if reject else "mapped-image hooks, OBS files, and SDK shutdown") + f" ({work.name})")
