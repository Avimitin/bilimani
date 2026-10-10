"""Exercise real DLL startup/shutdown without executing game code or networking.

Usage: py scripts/smoke-dll.py [--reject | --input-detour | --bad-input-detour]
Additional options: --occupied-port --no-sdk-input --sdk-renderer --profiles
--song-info exercises song callbacks with synthetic records and stubbed originals.
--lane-generator-patch simulates an existing hook on the RANDOM generator.
--bad-lane-layout / --bad-lane-update verify optional lane-only degradation.
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
song_info = "--song-info" in sys.argv
lane_generator_patch = "--lane-generator-patch" in sys.argv
bad_lane_layout = "--bad-lane-layout" in sys.argv
bad_lane_update = "--bad-lane-update" in sys.argv
lane_disabled = bad_lane_layout or bad_lane_update
bad_input_detour = "--bad-input-detour" in sys.argv
input_detour = "--input-detour" in sys.argv or bad_input_detour
startup_rejected = reject or bad_input_detour
assert not (reject and input_detour), "Detour checks require the supported game image"
work = root / "analysis" / ("smoke-" + uuid.uuid4().hex)
work.mkdir(parents=True)
dll_path = work / "bilimani.dll"
shutil.copy2(root / "target/release/bilimani.dll", dll_path)
shutil.copytree(root / "web/card", work / "bilimani_web/card")
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
(work / "bilimani.toml").write_text(config, encoding="utf-8")

kernel = ctypes.WinDLL("kernel32", use_last_error=True)
kernel.LoadLibraryExW.argtypes = [ctypes.c_wchar_p, ctypes.c_void_p, ctypes.c_uint32]
kernel.LoadLibraryExW.restype = ctypes.c_void_p
if not reject:
    # Map/relocate only. DONT_RESOLVE_DLL_REFERENCES prevents imports, TLS
    # callbacks and the game entry point from running. Never call game exports.
    game = kernel.LoadLibraryExW(str(root / "analysis/bm2dx.dll"), None, 1)
    assert game, ctypes.WinError(ctypes.get_last_error())
if input_detour or song_info or lane_generator_patch or lane_disabled:
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

if input_detour:
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
if lane_generator_patch:
    lane_generator = game + 0x8236e0
    # A ret stub stands in for a plugin-owned generator. Display sampling must
    # never execute it, and must accept a valid table written by that plugin.
    generator_patch = b'\xc3' + ctypes.string_at(lane_generator + 1, 15)
    patch(lane_generator, generator_patch)
if bad_lane_layout:
    patch(game + 0x897890, b'\xc3')
if bad_lane_update:
    patch(game + 0xda50a8 + 15 * 8, struct.pack('<Q', game + 0x933640))
plugin = ctypes.CDLL(str(dll_path))
plugin.bilimani_shutdown.argtypes = []
plugin.bilimani_shutdown.restype = None

deadline = time.monotonic() + 20
logfile = work / "bilimani.log"
while time.monotonic() < deadline:
    text = logfile.read_text(encoding="utf-8") if logfile.exists() else ""
    if "Startup failed" in text or ("Native hooks installed" in text and "[status]" in text and "select_updates=0" in text):
        break
    time.sleep(0.05)
else:
    raise AssertionError("DLL worker did not initialize within 20 seconds")
with sqlite3.connect(work / "bilimani.db") as database:
    assert database.execute("PRAGMA user_version").fetchone()[0] == 2
    settings = json.loads(database.execute("SELECT settings FROM settings").fetchone()[0])
    profile = json.loads(database.execute("SELECT p.settings FROM stream_profiles p JOIN settings s ON p.id = s.default_profile").fetchone()[0])
    assert settings["overlay"]["port"] == overlay_port
    assert "bilibili" not in settings and profile["enabled"] is False
    if profiles:
        assert database.execute("SELECT count(*) FROM stream_cards").fetchone()[0] == 3
assert (work / "bilimani.toml").read_text(encoding="utf-8") == config
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
        with urllib.request.urlopen(f"http://127.0.0.1:{overlay_port}/api/now-playing", timeout=3) as response:
            assert json.load(response) == state["now_playing"] == dict(phase="idle", song=None, players=[], lane_order=[])
    for table, slot, rva in [(0xd84788, 13, 0x8eb820), (0xd84788, 14, 0x8ebeb0),
                             (0xd84788, 15, 0x8ec1f0), (0xce9f40, 1, 0x7f2fd0),
                             (0xdd05c0, 3, 0xa7a2f0), (0xda50a8, 14, 0x933640),
                             (0xda50a8, 15, 0x9336a0), (0xdae728, 15, 0x8d2350)]:
        hooked = ctypes.c_void_p.from_address(game + table + slot*8).value
        if lane_disabled and table in (0xda50a8, 0xdae728) and slot == 15:
            expected = 0x933640 if bad_lane_update and table == 0xda50a8 else rva
            assert hooked == game + expected, 'Disabled lane sampler modified a stage update'
            continue
        assert hooked != game+rva, f"Slot {slot} was not patched"
        assert abs(hooked-plugin._handle) < 0x4000000, "Hook is not inside plugin image"
    if lane_disabled:
        assert 'Lane-order sampling disabled:' in text and 'other features remain available' in text, text
        assert 'observed=' in text, text
    elif lane_generator_patch:
        assert 'patched RANDOM generator at RVA 8236e0' in text, text
        assert ctypes.string_at(lane_generator, 16) == generator_patch
    else:
        assert 'Lane-order sampling enabled' in text, text
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

if song_info:
    assert not startup_rejected and not occupied_port and not profiles
    # The real installation guards already passed. Stub only this process's
    # private mapped image; no game function body or import is executed.
    def return_value(rva, value):
        patch(game + rva, b'\x48\xb8' + struct.pack('<Q', value) + b'\xc3')

    def record(title, music_id):
        data = bytearray(0x7f8)
        for offset, value in [(0, title), (0x140, 'SMOKE GENRE'), (0x1c0, 'Smoke Artist')]:
            encoded = value.encode('utf-16-le')
            data[offset:offset + len(encoded)] = encoded
        struct.pack_into('<I', data, 0x67c, music_id)
        data[0x3dc] = 33
        data[0x3ec + 3] = 12
        data[0x3ec + 8] = 11
        struct.pack_into('<II', data, 0x3fc + 3 * 8, 200, 100)
        struct.pack_into('<I', data, 0x47c + 3 * 4, 1234)
        struct.pack_into('<6I', data, 0x4fc + 3 * 24, 15025, 10000, 5500, 0, 7500, 12000)
        return ctypes.create_string_buffer(bytes(data))

    selected_record = record('Manual selection fixture', 33001)
    stage_record = record('Actual stage fixture', 33002)
    reservation = ctypes.create_string_buffer(32)
    reservation[8] = b'\x01'
    reservation[9] = b'\x01'
    for rva, value in [(0x8eb820, 71), (0x8ebeb0, 72), (0x8ec1f0, 73),
                       (0x9335d0, 74), (0x933640, 75), (0x9336a0, 76), (0x82ded0, 0),
                       (0x806f60, 0), (0x7d60e0, ctypes.addressof(reservation)),
                       (0x606e60, 1), (0x606fd0, ctypes.addressof(selected_record)),
                       (0x607030, 3), (0x9493e0, 1)]:
        return_value(rva, value)
    # The original database accessor remains guarded and returns an empty header,
    # so catalog startup stays pending while display metadata must still work.
    scene = ctypes.create_string_buffer(9000)
    controller = ctypes.create_string_buffer(64)
    ctypes.c_void_p.from_buffer(scene).value = game + 0xd84788
    ctypes.c_uint32.from_buffer(scene, 80).value = 3
    ctypes.c_int32.from_buffer(scene, 128).value = 1000
    ctypes.c_void_p.from_buffer(scene, 144).value = ctypes.addressof(controller)
    ctypes.c_void_p.from_buffer(controller, 8).value = game + 0x8ee190
    stage = ctypes.create_string_buffer(16)
    ctypes.c_void_p.from_buffer(stage).value = game + 0xda50a8
    ctypes.c_void_p.from_address(game + 0xabac028).value = ctypes.addressof(stage_record)
    for side in range(2):
        ctypes.c_uint32.from_address(game + 0xacd79a8 + side * 4).value = 3
        ctypes.c_uint32.from_address(game + 0xacd79b0 + side * 4).value = 1
    options = ctypes.create_string_buffer(548)
    ctypes.c_void_p.from_buffer(options).value = game + 0xd54e60
    ctypes.c_void_p.from_address(game + 0xaab3aa8).value = ctypes.addressof(options)

    def lane_table(side, values):
        for key, destination in enumerate(values):
            ctypes.c_uint32.from_address(game + 0xa7ef580 + side * 32 + key * 4).value = destination

    for side in range(2):
        lane_table(side, range(8))
    callback = ctypes.CFUNCTYPE(ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_size_t, ctypes.c_size_t)

    def invoke(instance, slot):
        table = ctypes.c_void_p.from_buffer(instance).value
        address = ctypes.c_void_p.from_address(table + slot * 8).value
        return callback(address)(ctypes.addressof(instance), 0, 0, 0)

    def wait_song(phase, title, chart_id=None, keys=None, lane_status=None):
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            with urllib.request.urlopen(f'http://127.0.0.1:{overlay_port}/api/now-playing', timeout=2) as response:
                data = json.load(response)
            if (data['phase'] == phase and (data['song'] or {}).get('title') == title
                    and (chart_id is None or data['players'][0]['chart']['id'] == chart_id)
                    and (keys is None or data['lane_order'][0]['keys'] == keys)
                    and (lane_status is None or data['lane_order'][0]['status'] == lane_status)):
                return data
            time.sleep(.05)
        raise AssertionError(f'Song snapshot not published: {phase} {title}: {data}')

    assert invoke(scene, 13) == 71
    assert invoke(scene, 15) == 73
    data = wait_song('selecting', 'Manual selection fixture')
    assert [p['side'] for p in data['players']] == [1, 2]
    chart = data['song']['charts'][0]
    assert chart['bpm'] == dict(min=100, max=200) and chart['note_count'] == 1234
    assert chart['radar']['notes'] == 150.25
    assert chart['density'] is None
    if lane_disabled:
        assert data['lane_order'] == []
    else:
        assert data['lane_order'][0]['keys'] == [1, 2, 3, 4, 5, 6, 7]
        assert data['lane_order'][0]['random'] == 'off'
    # Reuse the native background request ABI without running any game code.
    detail_thread = ctypes.create_string_buffer(56)
    ctypes.c_void_p.from_buffer(detail_thread).value = game + 0xcb5cd0
    ctypes.c_uint32.from_buffer(detail_thread, 8).value = 77
    ctypes.c_void_p.from_address(game + 0xa7d33e0).value = ctypes.addressof(detail_thread)
    mutex_calls = []
    mutex_fn = ctypes.CFUNCTYPE(None, ctypes.c_uint32)
    @mutex_fn
    def mutex_lock(handle):
        mutex_calls.append(('lock', handle))
    @mutex_fn
    def mutex_unlock(handle):
        mutex_calls.append(('unlock', handle))
    patch(game + 0xc91fe0, struct.pack('<Q', ctypes.cast(mutex_lock, ctypes.c_void_p).value))
    patch(game + 0xc91fe8, struct.pack('<Q', ctypes.cast(mutex_unlock, ctypes.c_void_p).value))
    publication_lock = ctypes.c_uint32.from_address(game + 0xbaac324)
    publication_lock.value = 1
    time.sleep(.3)
    invoke(scene, 15)
    assert not mutex_calls and publication_lock.value == 1  # busy: no wait or unlock
    publication_lock.value = 0
    time.sleep(.3)
    invoke(scene, 15)
    assert mutex_calls == [('lock', 77), ('unlock', 77)]
    assert ctypes.c_uint64.from_buffer(detail_thread, 16).value == 33001 | (1 << 32)
    assert publication_lock.value == 0
    # Publish two distinct native difficulty histograms on the next frame.
    analyzer = ctypes.create_string_buffer(256)
    ctypes.c_void_p.from_buffer(analyzer).value = game + 0xcb5cf0
    ctypes.c_uint32.from_buffer(analyzer, 8).value = 33001
    native_histograms = []
    for index, notes, scratch in [(3, [0, 2, 42, 1], [0, 0, 2, 1]), (8, [3, 4], [1, 2])]:
        detail = ctypes.create_string_buffer(116)
        for offset, values in [(0, notes), (24, scratch)]:
            vector = (ctypes.c_uint32 * len(values))(*values)
            begin = ctypes.addressof(vector)
            struct.pack_into('<QQQ', detail, offset, begin, begin + len(values) * 4, begin + len(values) * 4)
            native_histograms.append(vector)
        ctypes.c_uint32.from_buffer(detail, 112).value = 6500
        ctypes.c_void_p.from_buffer(analyzer, 16 + index * 24).value = ctypes.addressof(detail)
        analyzer[32 + index * 24] = b'\x01'
        native_histograms.append(detail)
    ctypes.c_void_p.from_buffer(detail_thread, 40).value = ctypes.addressof(analyzer)
    time.sleep(.3)
    invoke(scene, 15)
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        data = wait_song('selecting', 'Manual selection fixture')
        if data['song']['charts'][0]['density'] is not None:
            break
        time.sleep(.05)
    assert data['song']['charts'][0]['density'] == dict(bin_ms=1000, duration_ms=6500, notes=[0, 2, 42, 1], scratch=[0, 0, 2, 1])
    assert data['song']['charts'][1]['density']['notes'] == [3, 4]
    assert publication_lock.value == 0
    # Cache contains owned arrays: the native analyzer can disappear before stage init.
    ctypes.c_void_p.from_address(game + 0xa7d33e0).value = 0
    # DP and two-player SP share a double layout but use different chart indices.
    return_value(0x82ded0, 1)
    ctypes.c_uint32.from_address(game + 0xacd79a4).value = 1
    time.sleep(.3)
    invoke(scene, 15)
    wait_song('selecting', 'Manual selection fixture', 'DPA')
    ctypes.c_uint32.from_address(game + 0xacd79a4).value = 0
    time.sleep(.3)
    invoke(scene, 15)
    wait_song('selecting', 'Manual selection fixture', 'SPA')
    # A folder must clear the selected song despite the native record fallback.
    return_value(0x606e60, 0)
    time.sleep(.3)
    assert invoke(scene, 15) == 73
    wait_song('selecting', None)
    assert invoke(scene, 14) == 72
    assert wait_song('idle', None)['lane_order'] == []
    ctypes.c_void_p.from_address(game + 0xabac028).value = ctypes.addressof(selected_record)
    ctypes.c_uint32.from_buffer(options, 8 + 48).value = 1
    lane_table(0, [4, 3, 0, 1, 2, 5, 6, 7])
    assert invoke(stage, 13) == 74
    cached = wait_song('playing', 'Manual selection fixture')
    assert cached['song']['charts'][0]['density']['notes'] == [0, 2, 42, 1]
    if lane_disabled:
        assert cached['lane_order'] == []
    else:
        assert cached['lane_order'][0]['keys'] == [3, 4, 5, 2, 1, 6, 7]
        assert cached['lane_order'][0]['random'] == 'random'
        # Successful sound loading also sets this general stage flag to 1.
        # Several subsequent gameplay samples must retain the current layout.
        ctypes.c_ubyte.from_address(game + 0xaab19ae).value = 1
        for _ in range(3):
            time.sleep(.3)
            assert invoke(stage, 15) == 76
        time.sleep(.2)  # Let the worker publish the post-update snapshot.
        wait_song('playing', 'Manual selection fixture', keys=[3, 4, 5, 2, 1, 6, 7], lane_status='ready')
        # Same-stage retry replaces the owned map; an invalid read clears it.
        lane_table(0, [6, 5, 4, 3, 2, 1, 0, 7])
        time.sleep(.3)
        assert invoke(stage, 15) == 76
        wait_song('playing', 'Manual selection fixture', keys=[7, 6, 5, 4, 3, 2, 1])
        lane_table(0, [0] * 8)
        time.sleep(.3)
        assert invoke(stage, 15) == 76
        invalid = wait_song('playing', 'Manual selection fixture', lane_status='unavailable')
        assert invalid['lane_order'][0]['keys'] is None
        ctypes.c_uint32.from_buffer(options, 8 + 48).value = 3
        time.sleep(.3)
        invoke(stage, 15)
        dynamic = wait_song('playing', 'Manual selection fixture', lane_status='dynamic')
        assert dynamic['lane_order'][0]['keys'] is None
    assert invoke(stage, 14) == 75
    assert wait_song('idle', None)['lane_order'] == []
    ctypes.c_void_p.from_address(game + 0xabac028).value = ctypes.addressof(stage_record)
    assert invoke(stage, 13) == 74
    different = wait_song('playing', 'Actual stage fixture')
    assert all(c['density'] is None for c in different['song']['charts'])
    assert invoke(stage, 14) == 75
    wait_song('idle', None)
    if lane_disabled:
        print('PASS: incompatible lane sampler isolated; web, song/density callbacks, stage cleanup and core hooks remain available')
    else:
        print('PASS: real DLL callbacks -> song/density/lane JSON, mapping direction, sustained gameplay after sound loading, same-stage retry, invalid/dynamic lane clearing, async request ABI, busy lock, SP/DP, cached stage, folders, cleanup and preserved returns')

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
