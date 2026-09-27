"""Exercise real DLL startup/shutdown without executing game code or networking.

Usage: py scripts/smoke-dll.py [--reject | --occupied-port | --no-sdk-input]
Requires the built release DLL and the ignored local game copy for positive mode.
Artifacts remain under analysis/smoke-* for inspection.
"""
import ctypes
import json
import pathlib
import shutil
import socket
import sqlite3
import struct
import sys
import time
import uuid
import urllib.request
import urllib.error

root = pathlib.Path(__file__).resolve().parent.parent
reject = "--reject" in sys.argv
occupied_port = "--occupied-port" in sys.argv
work = root / "analysis" / ("smoke-" + uuid.uuid4().hex)
work.mkdir(parents=True)
dll_path = work / "chart_requester.dll"
shutil.copy2(root / "target/release/chart_requester.dll", dll_path)
config = (root / "tests/fixtures/legacy-config.toml").read_text(encoding="utf-8")
config = config.replace("enabled = true", "enabled = false", 1)
port_blocker = socket.socket()
port_blocker.bind(("127.0.0.1", 0))
overlay_port = port_blocker.getsockname()[1]
if occupied_port:
    port_blocker.listen()
else:
    port_blocker.close()
config = config.replace("port = 32133", f"port = {overlay_port}")
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
with sqlite3.connect(work / "chart-requester.db") as database:
    assert database.execute("PRAGMA user_version").fetchone()[0] == 1
    settings = json.loads(database.execute("SELECT settings FROM settings").fetchone()[0])
    profile = json.loads(database.execute("SELECT settings FROM stream_profiles").fetchone()[0])
    assert settings["overlay"]["port"] == overlay_port
    assert "bilibili" not in settings and profile["enabled"] is False
assert (work / "chart-requester.toml").read_text(encoding="utf-8") == config
if reject:
    assert "Unsupported bm2dx.dll build" in text, text
    assert "Native hooks installed" not in text
    assert "Unsupported bm2dx.dll build" in (work / "obs/interaction.txt").read_text(encoding="utf-8")
else:
    assert "Native hooks installed" in text, text
    assert "[status]" in text and "select_updates=0" in text, text
    assert "select_hooks_intact=true" in text, text
    if occupied_port:
        assert "网页界面启动失败" in text, text
        assert "网页界面启动失败" in (work / "obs/interaction.txt").read_text(encoding="utf-8")
    else:
        with urllib.request.urlopen(f"http://127.0.0.1:{overlay_port}/queue", timeout=3) as response:
            assert b"/overlay.js" in response.read()
        with urllib.request.urlopen(f"http://127.0.0.1:{overlay_port}/api/state", timeout=3) as response:
            state = json.load(response)
            assert state["ready"] is False and state["queue"] == []
    for table, slot, rva in [(0xd84788, 13, 0x8eb820), (0xd84788, 14, 0x8ebeb0),
                             (0xd84788, 15, 0x8ec1f0), (0xce9f40, 1, 0x7f2fd0),
                             (0xdd05c0, 3, 0xa7a2f0)]:
        hooked = ctypes.c_void_p.from_address(game + table + slot*8).value
        assert hooked != game+rva, f"Slot {slot} was not patched"
        assert abs(hooked-plugin._handle) < 0x4000000, "Hook is not inside plugin image"

# Simulate Spice's documented SDK initialization/shutdown callback ABI.
destroy_callback = None
INIT = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p)
GET_BUTTON = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_uint32, ctypes.POINTER(ctypes.c_bool), ctypes.c_void_p)
DRAW = ctypes.CFUNCTYPE(None, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p)
REGISTER = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p)
draw_callback = None

@REGISTER
def sdk_register(callback, userdata):
    global draw_callback
    assert callback and not userdata
    draw_callback = DRAW(callback)
    return 0

@GET_BUTTON
def sdk_get_button(button, pressed, velocity):
    assert button in (14, 26)
    pressed[0] = False
    return 0

@INIT
def sdk_init(version, destroy, api):
    global destroy_callback
    assert version == 0
    assert ctypes.c_uint32.from_address(api).value == 152
    if "--sdk-renderer" in sys.argv:
        ctypes.c_void_p.from_address(api + 8 + 17 * 8).value = ctypes.cast(sdk_register, ctypes.c_void_p).value
    if "--no-sdk-input" not in sys.argv:
        # Eight-byte-aligned function table, get_button is slot 3.
        ctypes.c_void_p.from_address(api + 8 + 3 * 8).value = ctypes.cast(sdk_get_button, ctypes.c_void_p).value
    destroy_callback = ctypes.CFUNCTYPE(None)(destroy)
    return 0

plugin.spice_sdk_entry_point.argtypes = [INIT]
plugin.spice_sdk_entry_point.restype = ctypes.c_int
assert plugin.spice_sdk_entry_point(sdk_init) == 0
assert destroy_callback
if "--sdk-renderer" in sys.argv:
    assert draw_callback
    # Null/unusable frames must be ignored; lifecycle callbacks must be safe
    # even before the host has ever supplied a device.
    for event in (0, 1, 2, 3):
        draw_callback(event, None, None)
if not reject and "--no-sdk-input" not in sys.argv:
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        if "sdk_status=-2" in logfile.read_text(encoding="utf-8"):
            break
        time.sleep(0.05)
    else:
        raise AssertionError("DLL did not retain the SDK get_button function")
destroy_callback()
if not reject:
    assert (work / "obs/queue.txt").read_text(encoding="utf-8").strip() == "点歌已停止"
    if not occupied_port:
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{overlay_port}/api/state", timeout=1)
        except urllib.error.URLError:
            pass
        else:
            raise AssertionError("Overlay server still running after shutdown")
port_blocker.close()
print("PASS: " + ("unsupported image rejected" if reject else "occupied-port fallback and SDK shutdown" if occupied_port else "mapped-image hooks, web overlay, OBS files, and SDK shutdown") + f" ({work.name})")
