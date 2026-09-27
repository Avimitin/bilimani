"""Exercise real DLL startup/shutdown without executing game code or networking.

Usage: py scripts/smoke-dll.py [--reject | --input-detour | --bad-input-detour]
Additional options: --occupied-port --no-sdk-input --sdk-renderer --profiles
Requires the built release DLL and the ignored local game copy for positive mode.
Artifacts remain under analysis/smoke-* for inspection.
"""
import ctypes
import json
import os
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
profiles = "--profiles" in sys.argv
bad_input_detour = "--bad-input-detour" in sys.argv
input_detour = "--input-detour" in sys.argv or bad_input_detour
startup_rejected = reject or bad_input_detour
assert not (reject and input_detour), "Detour checks require the supported game image"
work = root / "analysis" / ("smoke-" + uuid.uuid4().hex)
work.mkdir(parents=True)
dll_path = work / "chart_requester.dll"
shutil.copy2(root / "target/release/chart_requester.dll", dll_path)
shutil.copytree(root / "web", work / "chart_request_static")
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
profile_ids = [str(uuid.uuid4()), str(uuid.uuid4())]
if profiles:
    assert not startup_rejected
    for profile_id, cards in zip(profile_ids, [["E004012345678901", "E004012345678902"], ["E004012345678903"]]):
        config += f'\n[[profiles]]\nid = "{profile_id}"\nname = "Smoke profile"\ncards = {json.dumps(cards)}\n[profiles.bilibili]\nenabled = false\nauth_code = "smoke-only-disabled"\n'
(work / "chart-requester.toml").write_text(config, encoding="utf-8")

kernel = ctypes.WinDLL("kernel32", use_last_error=True)
kernel.LoadLibraryExW.argtypes = [ctypes.c_wchar_p, ctypes.c_void_p, ctypes.c_uint32]
kernel.LoadLibraryExW.restype = ctypes.c_void_p
if not reject:
    # Map/relocate only. DONT_RESOLVE_DLL_REFERENCES prevents imports, TLS
    # callbacks and the game entry point from running. Never call game exports.
    game = kernel.LoadLibraryExW(str(root / "analysis/bm2dx.dll"), None, 1)
    assert game, ctypes.WinError(ctypes.get_last_error())
if input_detour:
    # Synthetic MinHook x64 entry + relay in this process's private image.
    # Route to a real loaded executable module, never to the game body.
    kernel.VirtualProtect.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_uint32,
                                      ctypes.POINTER(ctypes.c_uint32)]
    kernel.VirtualProtect.restype = ctypes.c_int
    kernel.GetCurrentProcess.argtypes = []
    kernel.GetCurrentProcess.restype = ctypes.c_void_p
    kernel.FlushInstructionCache.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t]
    kernel.FlushInstructionCache.restype = ctypes.c_int

    def patch(address, code):
        protection = ctypes.c_uint32()
        assert kernel.VirtualProtect(address, len(code), 0x40, ctypes.byref(protection))
        try:
            ctypes.memmove(address, code, len(code))
        finally:
            unused = ctypes.c_uint32()
            assert kernel.VirtualProtect(address, len(code), protection.value, ctypes.byref(unused))
        assert kernel.FlushInstructionCache(kernel.GetCurrentProcess(), address, len(code))

    input_entry = game + 0xa7a2f0
    input_relay = game + 0x1000
    input_target = ctypes.cast(kernel.GetCurrentProcessId, ctypes.c_void_p).value
    original_entry = ctypes.string_at(input_entry, 16)
    assert original_entry == bytes.fromhex("48894c24085553565741544155415641")
    patched_entry = b"\xe9" + struct.pack("<i", input_relay - input_entry - 5) + original_entry[5:]
    if bad_input_detour:
        patched_entry = patched_entry[:5] + bytes([patched_entry[5] ^ 1]) + patched_entry[6:]
    relay_code = bytes.fromhex("ff2500000000") + struct.pack("<Q", input_target)
    patch(input_relay, relay_code)
    patch(input_entry, patched_entry)
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
    assert database.execute("PRAGMA user_version").fetchone()[0] == 2
    settings = json.loads(database.execute("SELECT settings FROM settings").fetchone()[0])
    profile = json.loads(database.execute("SELECT p.settings FROM stream_profiles p JOIN settings s ON p.id = s.default_profile").fetchone()[0])
    assert settings["overlay"]["port"] == overlay_port
    assert "bilibili" not in settings and profile["enabled"] is False
    if profiles:
        assert database.execute("SELECT count(*) FROM stream_cards").fetchone()[0] == 3
assert (work / "chart-requester.toml").read_text(encoding="utf-8") == config
if startup_rejected:
    reason = "Unsupported bm2dx.dll build" if reject else "Input poll at RVA a7a2f0 is incompatible"
    assert reason in text, text
    assert "Native hooks installed" not in text
    assert reason in (work / "obs/interaction.txt").read_text(encoding="utf-8")
    if bad_input_detour:
        assert "Unsupported input entry patch: observed=" in text, text
        for table, slot, rva in [(0xd84788, 13, 0x8eb820), (0xdd05c0, 3, 0xa7a2f0)]:
            assert ctypes.c_void_p.from_address(game + table + slot * 8).value == game + rva
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
            assert state["feed"] == [] and state["feed_limit"] == 10
    for table, slot, rva in [(0xd84788, 13, 0x8eb820), (0xd84788, 14, 0x8ebeb0),
                             (0xd84788, 15, 0x8ec1f0), (0xce9f40, 1, 0x7f2fd0),
                             (0xdd05c0, 3, 0xa7a2f0)]:
        hooked = ctypes.c_void_p.from_address(game + table + slot*8).value
        assert hooked != game+rva, f"Slot {slot} was not patched"
        assert abs(hooked-plugin._handle) < 0x4000000, "Hook is not inside plugin image"
    if input_detour:
        assert "Input poll chain: MinHook -> " in text, text
        assert ctypes.string_at(input_entry, 16) == patched_entry
        assert ctypes.string_at(input_relay, 14) == relay_code
        # Call the installed vtable wrapper and prove it preserves the existing
        # detour and its return value. GetCurrentProcessId ignores extra x64 args.
        instance = ctypes.create_string_buffer(0x68)
        ctypes.c_void_p.from_buffer(instance).value = game + 0xdd05c0
        input_slot = ctypes.c_void_p.from_address(game + 0xdd05c0 + 3 * 8).value
        poll = ctypes.CFUNCTYPE(ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t,
                               ctypes.c_size_t, ctypes.c_size_t)(input_slot)
        assert poll(ctypes.addressof(instance), 0, 0, 0) == os.getpid()
        print("PASS: existing MinHook relay preserved and called through the installed input wrapper")
    else:
        assert "Input poll chain: native" in text, text

if profiles:
    # Synthetic state in our private mapped image; never execute the game's code.
    # Exercise the actual DLL worker's read-only card polling and routing.
    def word(rva, value):
        ctypes.c_uint32.from_address(game + rva).value = value

    def wait_switches(count):
        deadline = time.monotonic() + 4
        while time.monotonic() < deadline:
            text = logfile.read_text(encoding="utf-8")
            lines = [line for line in text.splitlines() if "Active stream profile changed id=" in line]
            if len(lines) >= count:
                assert len(lines) == count, lines
                return lines
            time.sleep(0.05)
        raise AssertionError(f"Profile transition {count} did not arrive")

    word(0x10b90e8, 1)
    word(0x10b90ec, 1)
    word(0x6c11820, 0)
    ctypes.c_ubyte.from_address(game + 0x312771e).value = 1
    ctypes.c_uint64.from_address(game + 0x6c11808).value = 0xE004012345678901
    word(0xacd79b0, 1)
    assert f"id={profile_ids[0]};" in wait_switches(1)[-1]
    ctypes.c_uint64.from_address(game + 0x6c11808).value = 0xE004012345678902
    time.sleep(0.35)
    assert len(wait_switches(1)) == 1  # Shared profile does not reconnect.
    ctypes.c_uint64.from_address(game + 0x6c11808).value = 0xE004012345678903
    assert f"id={profile_ids[1]};" in wait_switches(2)[-1]
    word(0xacd79b0, 0)
    assert "id=global;" in wait_switches(3)[-1]
    # Same checks for a player joining on the right side.
    stride = 0x3b103a0
    word(0x6c11820 + stride, 0)
    ctypes.c_ubyte.from_address(game + 0x312771e + stride).value = 1
    ctypes.c_uint64.from_address(game + 0x6c11808 + stride).value = 0xE004012345678901
    word(0xacd79b4, 1)
    assert f"id={profile_ids[0]};" in wait_switches(4)[-1]
    word(0x6c11820 + stride, 1)  # Guest flag overrides leftover card bytes.
    assert "id=global;" in wait_switches(5)[-1]
    assert "E0040123456789" not in logfile.read_text(encoding="utf-8")
    print("PASS: DLL card routing, shared-profile cards, both sides, logout and guest fallback")

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
if not startup_rejected and "--no-sdk-input" not in sys.argv:
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        if "sdk_status=-2" in logfile.read_text(encoding="utf-8"):
            break
        time.sleep(0.05)
    else:
        raise AssertionError("DLL did not retain the SDK get_button function")
destroy_callback()
if not startup_rejected:
    assert (work / "obs/queue.txt").read_text(encoding="utf-8").strip() == "点歌已停止"
    if not occupied_port:
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{overlay_port}/api/state", timeout=1)
        except urllib.error.URLError:
            pass
        else:
            raise AssertionError("Overlay server still running after shutdown")
port_blocker.close()
print("PASS: " + ("unsupported image rejected" if reject else "unknown input patch rejected before installing hooks" if bad_input_detour else "occupied-port fallback and SDK shutdown" if occupied_port else "mapped-image hooks, web overlay, OBS files, and SDK shutdown") + f" ({work.name})")
