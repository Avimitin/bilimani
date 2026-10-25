"""Exercise the release IIDX DLL against a mapped 010-0917 game image.

Maps the supplied bm2dx.dll with DONT_RESOLVE_DLL_REFERENCES, loads the release
bilimani_iidx.dll and checks fingerprint selection, every native guard, vtable
hook installation, catalog capture, gameplay song/player snapshots and cleanup.
Original scene functions are replaced by synthetic stubs after guard checks;
the native read-only database getter remains intact. Only local HTTP is used,
with no game entry point or platform connection executed.
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
import urllib.request

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
assert "Song-decision sampling enabled" in text, text

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

kernel.VirtualProtect.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_uint32, ctypes.POINTER(ctypes.c_uint32)]
kernel.VirtualProtect.restype = ctypes.c_int
kernel.GetCurrentProcess.restype = ctypes.c_void_p
kernel.FlushInstructionCache.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t]
kernel.FlushInstructionCache.restype = ctypes.c_int


def stub(rva, result):
    address = game + rva
    code = b'\x48\xb8' + struct.pack('<Q', result) + b'\xc3'
    old = ctypes.c_uint32()
    assert kernel.VirtualProtect(address, len(code), 0x40, ctypes.byref(old))
    try:
        ctypes.memmove(address, code, len(code))
    finally:
        unused = ctypes.c_uint32()
        assert kernel.VirtualProtect(address, len(code), old.value, ctypes.byref(unused))
    assert kernel.FlushInstructionCache(kernel.GetCurrentProcess(), address, len(code))


def put(rva, data):
    ctypes.memmove(game + rva, data, len(data))


def read_api(path):
    with urllib.request.urlopen(f'http://127.0.0.1:{port}/api/v1/{path}', timeout=3) as response:
        return json.load(response)


def wait_api(path, predicate):
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        value = read_api(path)
        if predicate(value):
            return value
        time.sleep(0.05)
    raise AssertionError(f'{path} did not converge: {value}')


callback = ctypes.CFUNCTYPE(ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_size_t, ctypes.c_size_t)


def invoke(instance, slot):
    table = ctypes.c_void_p.from_buffer(instance).value
    address = ctypes.c_void_p.from_address(table + slot * 8).value
    return callback(address)(ctypes.addressof(instance), 0, 0, 0)


try:
    for rva, result in [(0x8d7bb0, 71), (0x8d7fa0, 72), (0x8d8290, 73), (0x91a300, 74),
                        (0x91a370, 75), (0x91a3d0, 76), (0x8bea80, 74),
                        (0x8beb00, 76), (0x81a530, 0)]:
        stub(rva, result)
    record = bytearray(0x7f8)
    title = 'Gameplay record at +0x18'.encode('utf-16-le')
    record[:len(title)] = title
    record[0x3ec + 3] = 12
    struct.pack_into('<H', record, 0x3dc, 34)
    struct.pack_into('<I', record, 0x67c, 1)
    # Exercise the real LEA accessor and its displacement, not a copied mock.
    database = struct.pack('<4sIIIII', b'IIDX', 34, 1, 2, 0xffffffff, 0) + record
    put(0xb60c870, database)
    put(0xb60b3a4, struct.pack('<IIIII', 0, 3, 3, 1, 0))
    put(0x107c0e8, struct.pack('<II', 1, 1))
    put(0x70f2e00, struct.pack('<Q', 123) + bytes(20))
    put(0x32b34d8, b'\x01')
    put(0x328c068 + 0x20, b'SMOKE\0\0\0')
    put(0x328c068 + 0x34, struct.pack('<IIii', 12345, 678, 18, 17))

    select = ctypes.create_string_buffer(9000)
    ctypes.c_void_p.from_buffer(select).value = game + 0xd52508
    assert invoke(select, 16) == 71
    wait_api('state', lambda state: state['ready'])
    assert read_api('songs?q=Gameplay')['songs'][0]['id'] == 1
    wait_api('iidx/user', lambda user: user['active_side'] == 1)

    # Only the executed decision member qualifies; queued confirmation and the
    # shared exit member are not decision frames. Exercise the real update hook.
    controller = ctypes.create_string_buffer(112)
    song = ctypes.create_string_buffer(bytes(record))
    struct.pack_into('<Q', select, 152, ctypes.addressof(controller))
    struct.pack_into('<Q', select, 8448, ctypes.addressof(song))
    struct.pack_into('<Q', controller, 56, game + 0x8d9330)
    assert invoke(select, 18) == 73
    assert read_api('state')['now_playing']['phase'] != 'deciding'
    struct.pack_into('<Q', controller, 56, 0)
    struct.pack_into('<Q', controller, 8, game + 0x8d9330)
    assert invoke(select, 18) == 73
    assert read_api('state')['now_playing']['phase'] != 'deciding'
    struct.pack_into('<I', controller, 88, 1)
    assert invoke(select, 18) == 73
    deciding = wait_api('state', lambda state: state['now_playing']['phase'] == 'deciding')
    assert deciding['phase'] == 'other' and deciding['plays'] == 0
    assert deciding['now_playing']['song']['title'] == 'Gameplay record at +0x18'
    assert deciding['now_playing']['playback'] is None
    # Exit starts before stage initialization; no turntable state leaks across it.
    struct.pack_into('<Q', controller, 8, game + 0x8d9f70)
    assert invoke(select, 18) == 73
    wait_api('state', lambda state: state['now_playing']['phase'] == 'idle')
    struct.pack_into('<Q', controller, 8, game + 0x8d9330)
    assert invoke(select, 18) == 73
    wait_api('state', lambda state: state['now_playing']['phase'] == 'deciding')
    assert invoke(select, 17) == 72
    wait_api('state', lambda state: state['now_playing']['phase'] == 'idle')

    # Native +8 owns an analyzer job; the actual record is at +0x18. Keep them
    # distinct, with readable non-song data at the former (incorrect) address.
    job = ctypes.create_string_buffer(0x7f8)
    song = ctypes.create_string_buffer(bytes(record))
    put(0xb4e0088, struct.pack('<Q', ctypes.addressof(job)))
    put(0xb4e0098, struct.pack('<Q', ctypes.addressof(song)))
    tables = [0xd73238, 0xd73338, 0xd73438, 0xd73538, 0xd73638, 0xd73738,
              0xd73838, 0xd73938, 0xd7b470, 0xd7b570, 0xd7b670, 0xd7b870, 0xd7bb48]
    for plays, table in enumerate(tables, start=1):
        stage = ctypes.create_string_buffer(16)
        ctypes.c_void_p.from_buffer(stage).value = game + table
        assert invoke(stage, 16) == 74
        state = wait_api('state', lambda state: state['phase'] == 'playing' and state['plays'] == plays)
        playing = state['now_playing']
        assert playing['song'] is not None, (hex(table), playing)
        assert playing['song']['title'] == 'Gameplay record at +0x18', playing
        assert playing['players'] == [{'side': 1, 'chart': {'mode': 'SP', 'id': 'SPA'}}]
        for _ in range(2):
            time.sleep(0.3)
            assert invoke(stage, 18) == 76
            assert read_api('iidx/now-playing')['song']['id'] == 1
            assert read_api('iidx/user')['players'][0]['dj_name'] == 'SMOKE'
        assert invoke(stage, 17) == 75
        cleared = wait_api('state', lambda state: state['phase'] == 'other')
        assert cleared['now_playing']['song'] is None

    # Missing native song clears telemetry; never recover from +8 or last select.
    put(0xb4e0098, bytes(8))
    assert invoke(stage, 16) == 74
    missing = wait_api('state', lambda state: state['phase'] == 'playing')
    assert missing['now_playing']['song'] is None
    assert invoke(stage, 17) == 75
    put(0xb60b3b0, bytes(8))
    wait_api('iidx/user', lambda user: user == {'active_side': None, 'players': []})
finally:
    plugin.bilimani_shutdown()
    time.sleep(0.5)
print(f'PASS: IIDX 34 / 010-0917 guards, native catalog getter, decision/exit lifecycle, all 13 stage families, '
      f'sustained song/player snapshots, missing record, cleanup and shutdown ({work.name})')
