"""Test the actual DDR release DLL with synthetic memory, never run game code.

Requires the locally copied analysis/ddr/2026022400/gamemdx.dll. LoadLibraryEx
maps it without resolving imports or executing DllMain. Native originals are
replaced with return stubs. This is integration validation, not a real-game test.
"""
import ctypes as c
import json
import pathlib
import shutil
import socket
import struct
import time
import urllib.request
import uuid
import argparse

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--wrong-build', action='store_true')
parser.add_argument('--bad-entry', action='store_true')
args = parser.parse_args()

root = pathlib.Path(__file__).resolve().parent.parent
work = root / 'analysis' / ('ddr-smoke-' + uuid.uuid4().hex)
work.mkdir(parents=True)
shutil.copy2(root / 'target/release/bilimani_ddr.dll', work / 'bilimani_ddr.dll')
sock = socket.socket()
sock.bind(('127.0.0.1', 0))
port = sock.getsockname()[1]
sock.close()
(work / 'bilimani_ddr.json').write_text(json.dumps({'listen': f'127.0.0.1:{port}', 'module': 'kernel32.dll' if args.wrong_build else 'gamemdx.dll', 'controls': {'enabled': False}}))
k = c.WinDLL('kernel32', use_last_error=True)
k.LoadLibraryExW.argtypes = [c.c_wchar_p, c.c_void_p, c.c_uint32]
k.LoadLibraryExW.restype = c.c_void_p
k.VirtualProtect.argtypes = [c.c_void_p, c.c_size_t, c.c_uint32, c.POINTER(c.c_uint32)]
k.FlushInstructionCache.argtypes = [c.c_void_p, c.c_void_p, c.c_size_t]
k.GetCurrentProcess.restype = c.c_void_p
game = k.LoadLibraryExW(str(root / 'analysis/ddr/2026022400/gamemdx.dll'), None, 1)
assert game, c.WinError(c.get_last_error())
def patch(address, data):
    old = c.c_uint32()
    assert k.VirtualProtect(address, len(data), 0x40, c.byref(old))
    c.memmove(address, data, len(data))
    ignored = c.c_uint32()
    assert k.VirtualProtect(address, len(data), old.value, c.byref(ignored))
    k.FlushInstructionCache(k.GetCurrentProcess(), address, len(data))
def q(address, value): c.c_uint64.from_address(address).value = value
def u(address, value): c.c_uint32.from_address(address).value = value
buffers = []
def alloc(size):
    b = c.create_string_buffer(size)
    buffers.append(b)
    return c.addressof(b)
def string(address, text):
    data = text.encode()
    assert len(data) < 16
    c.memmove(address, data, len(data))
    q(address + 16, len(data)); q(address + 24, 15)
for rva in [0xf1a10, 0xf1d40, 0x588b0, 0x59540]: patch(game + rva, b'\x31\xc0\xc3')
scene, model, view, panel, listing, item, metadata, record, db, session, wrapper, blocker = [alloc(n) for n in [160, 1024, 1200, 512, 600, 400, 64, 600, 128, 512, 16, 64]]
q(scene + 136, model); q(scene + 144, view); c.c_uint8.from_address(scene + 104).value = 1
q(view + 1088, panel); q(panel + 192, listing); c.c_uint8.from_address(panel + 297).value = 1
u(model + 264, 1); u(model, 4); u(model + 4, 3); u(model + 8, 4); q(model + 400, metadata)
items = alloc(8); q(items, item); q(listing + 104, items); q(listing + 112, items + 8)
q(item + 40, game + 0x35beb0); q(item + 328, metadata); q(metadata, record)
q(record, game + 0x355338); u(record + 8, 12345)
string(record + 24, 'Smoke Track'); string(record + 64, 'smoke yomi'); string(record + 104, 'Test Artist')
for i in range(10):
    c.c_int32.from_address(record + 476 + 4*i).value = -1
    c.c_uint8.from_address(record + 272 + i).value = 12 if i != 5 else 0
q(db, record); q(db + 8, record + 600); q(game + 0x6c9928, db)
q(wrapper, session); q(game + 0x6c80a8, wrapper); q(game + 0x6c9918, blocker)
player, player_wrapper = alloc(256), alloc(8)
q(player_wrapper, player); q(game + 0x6c9a80, player_wrapper)
c.c_uint8.from_address(player + 4).value = 1
if args.bad_entry: patch(game + 0x48370, b'\xc3')
plugin = c.CDLL(str(work / 'bilimani_ddr.dll'))
plugin.bilimani_shutdown.restype = None
base = f'http://127.0.0.1:{port}/api/v1'
def api(path, body=None):
    req = urllib.request.Request(base + path, data=None if body is None else json.dumps(body).encode(), headers={'Content-Type':'application/json'})
    with urllib.request.urlopen(req, timeout=3) as response: return json.load(response)
def wait(check):
    for _ in range(200):
        try:
            value = check()
            if value: return value
        except (OSError, KeyError): pass
        time.sleep(.05)
    raise AssertionError('Timed out; log: ' + (work / 'bilimani_ddr.log').read_text())
try:
    if args.wrong_build or args.bad_entry:
        logfile = work / 'bilimani_ddr.log'
        wait(lambda: logfile.exists() and 'Startup failed' in logfile.read_text())
        text = logfile.read_text()
        assert ('Unsupported DDR DLL' if args.wrong_build else 'DDR native entry mismatch') in text
        assert c.c_uint64.from_address(game + 0x355188).value == game + 0xf1a10
        print('DDR release DLL: incompatible binary/entry rejected before hook installation')
        raise SystemExit(0)
    wait(lambda: api('/meta'))
    update = c.CFUNCTYPE(c.c_size_t, c.c_size_t)(c.c_uint64.from_address(game + 0x355188).value)
    update(scene)
    wait(lambda: api('/meta')['ready'])
    songs = api('/songs?q=smk%20tr&limit=5')['songs']
    assert songs[0]['id'] == 12345 and len(songs[0]['charts']) == 9, songs
    assert api('/ddr/now-playing')['song']['artist'] == 'Test Artist'
    assert api('/ddr/now-playing')['selected_chart'] == {'chart': {'mode': 'SP', 'id': 'ESP'}, 'level': '12'}
    u(session + 8, 1); update(scene)
    wait(lambda: api('/ddr/now-playing')['selected_chart']['chart']['id'] == 'CSP')
    u(session + 8, 0); u(session + 4, 1); update(scene)
    wait(lambda: api('/ddr/now-playing')['selected_chart']['chart']['id'] == 'EDP')
    u(model + 4, 0); update(scene)
    wait(lambda: api('/ddr/now-playing')['selected_chart'] is None)
    u(model + 4, 3); u(session + 4, 0); update(scene)
    wait(lambda: (api('/ddr/now-playing')['selected_chart'] or {}).get('chart', {}).get('id') == 'ESP')
    # Replace only after attach has verified original instruction prefixes.
    patch(game + 0x49700, b'\xb0\x01\xc3')
    patch(game + 0x19fdc0, b'\xb0\x0c\xc3')
    patch(game + 0xf3460, b'\x31\xc0\xc3')
    patch(game + 0x48370, b'\x48\xb8' + struct.pack('<Q',session+24) + b'\xc7\x00' + struct.pack('<I',12345) + b'\xc3')
    request = api('/queue', {'song_id':12345, 'mode':'SP', 'chart':'ESP', 'requester':'Smoke viewer'})
    # Queue current has its own schema; selection is observable in native state.
    wait(lambda: (update(scene), c.c_uint32.from_address(session+24).value == 12345)[1])
    wait(lambda: api('/queue').get('current'))
    assert api('/queue')['current']['song_id'] == 12345
    actor = alloc(800)
    play = c.CFUNCTYPE(c.c_size_t, c.c_size_t)(c.c_uint64.from_address(game + 0x348468).value)
    play(actor); play(actor)
    wait(lambda: api('/state')['plays'] == 1)
    assert api('/state')['phase'] == 'playing'
    print('DDR release DLL: catalog, fuzzy search, metadata, queue mailbox and stage counter passed')
finally:
    plugin.bilimani_shutdown()
