"""Exercise the IIDX 34/010-0916 profile in the release DLL in a separate process without launching the game.

Requires npm run build and the release bilimani_iidx.dll.
Positive tests map the developer's ignored analysis/iidx34/bm2dx.dll without resolving
imports or executing its entry point. All invoked originals are synthetic stubs.
Evidence: docs/iidx34-analysis.md. No platform traffic is made. Artifacts remain under analysis/smoke-*.
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
import urllib.error

parser = argparse.ArgumentParser(description=__doc__)
for name in ['reject', 'occupied-port', 'song-info', 'queue', 'lane-generator-patch', 'bad-lane-layout', 'bad-lane-update', 'bad-playback', 'user', 'bad-user']:
    parser.add_argument('--' + name, action='store_true')
parser.add_argument('--game-dll', type=pathlib.Path, default=pathlib.Path('analysis/iidx34/bm2dx.dll'))
parser.add_argument('--bad-core', action='store_true')
parser.add_argument('--bad-scene', action='store_true')
args = parser.parse_args()
reject, occupied_port, song_info = args.reject, args.occupied_port, args.song_info or args.queue
lane_generator_patch, bad_lane_layout, bad_lane_update = args.lane_generator_patch, args.bad_lane_layout, args.bad_lane_update
lane_disabled = bad_lane_layout or bad_lane_update
bad_playback = args.bad_playback
playback_disabled = bad_playback or bad_lane_update
startup_rejected = reject or occupied_port or args.bad_core or args.bad_scene
root = pathlib.Path(__file__).resolve().parent.parent
work = root / 'analysis' / ('smoke-iidx34-' + uuid.uuid4().hex)
work.mkdir(parents=True)
dll_path = work / 'bilimani_iidx.dll'
shutil.copy2(root / 'target/release/bilimani_iidx.dll', dll_path)
shutil.copytree(root / 'dist/web/iidx/card', work / 'bilimani_web/iidx/card', ignore=shutil.ignore_patterns('config.json'))
port_blocker = socket.socket()
port_blocker.bind(('127.0.0.1', 0))
overlay_port = port_blocker.getsockname()[1]
if occupied_port: port_blocker.listen()
else: port_blocker.close()
config = {'listen':f'127.0.0.1:{overlay_port}', 'module':'kernel32.dll' if reject else 'bm2dx.dll'}
if args.queue:
    fixture = bytearray(0x7f8)
    title = 'Queue fixture'.encode('utf-16-le')
    fixture[:len(title)] = title
    struct.pack_into('<I', fixture, 0x67c, 33001)
    fixture[0x3ec + 3] = 12
    lookup = bytearray(b'\xff' * (33002 * 4))
    struct.pack_into('<I', lookup, 33001 * 4, 0)
    (work / 'catalog.bin').write_bytes(struct.pack('<4sIII', b'IIDX', 34, 1, 33002) + lookup + fixture)
    (work / 'bilimani_iidx_aliases.csv').write_text('alias,target\nfixture alias,33001\n', encoding='utf-8')
    config.update(database_path='catalog.bin')
(work / 'bilimani_iidx.json').write_text(json.dumps(config), encoding='utf-8')
kernel = ctypes.WinDLL('kernel32', use_last_error=True)
kernel.LoadLibraryExW.argtypes = [ctypes.c_wchar_p, ctypes.c_void_p, ctypes.c_uint32]
kernel.LoadLibraryExW.restype = ctypes.c_void_p
if not reject:
    game = kernel.LoadLibraryExW(str(args.game_dll.resolve()), None, 1)
    assert game, ctypes.WinError(ctypes.get_last_error())
kernel.VirtualProtect.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_uint32, ctypes.POINTER(ctypes.c_uint32)]
kernel.VirtualProtect.restype = ctypes.c_int
kernel.GetCurrentProcess.restype = ctypes.c_void_p
kernel.FlushInstructionCache.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t]
kernel.FlushInstructionCache.restype = ctypes.c_int
def patch(address, code):
    protection = ctypes.c_uint32()
    assert kernel.VirtualProtect(address, len(code), 0x40, ctypes.byref(protection))
    try: ctypes.memmove(address, code, len(code))
    finally:
        unused = ctypes.c_uint32()
        assert kernel.VirtualProtect(address, len(code), protection.value, ctypes.byref(unused))
    assert kernel.FlushInstructionCache(kernel.GetCurrentProcess(), address, len(code))
if lane_generator_patch:
    lane_generator = game + 0x80fb60
    generator_patch = b'\xc3' + ctypes.string_at(lane_generator + 1, 15)
    patch(lane_generator, generator_patch)
if bad_lane_layout: patch(game + 0x884680, b'\xc3')
if bad_lane_update: patch(game + 0xd73238 + 18 * 8, struct.pack('<Q', game + 0x91a330))
if bad_playback: patch(game + 0x811000, b'\xc3')
if args.bad_user: patch(game + 0x597660, b'\xc3')
if args.bad_core: patch(game + 0x7bf330, b'\xc3')
if args.bad_scene: patch(game + 0xd52508 + 18 * 8, struct.pack('<Q', game + 0x8d7f60))
plugin = ctypes.CDLL(str(dll_path))
plugin.bilimani_shutdown.argtypes = []
plugin.bilimani_shutdown.restype = None
deadline = time.monotonic() + 20
logfile = work / 'bilimani_iidx.log'
while time.monotonic() < deadline:
    text = logfile.read_text(encoding='utf-8') if logfile.exists() else ''
    if 'Startup failed' in text or 'select_updates=0' in text: break
    time.sleep(.05)
else: raise AssertionError('DLL worker did not initialize within 20 seconds')
if startup_rejected:
    expected = ('Unsupported IIDX binary fingerprint' if reject else
                'Native function at RVA' if args.bad_core else
                'Unexpected selection lifecycle entry' if args.bad_scene else
                'Cannot listen for game API')
    assert expected in text, text
    if reject or args.bad_core or args.bad_scene: assert 'Native hooks installed' not in text
else:
    assert 'Native hooks installed' in text and 'select_hooks_intact=true' in text, text
    with urllib.request.urlopen(f'http://127.0.0.1:{overlay_port}/iidx/card/', timeout=3) as response:
        assert b'./assets/' in response.read()
    with urllib.request.urlopen(f'http://127.0.0.1:{overlay_port}/api/v1/state', timeout=3) as response:
        state = json.load(response)
        assert (args.queue or state['ready'] is False) and state['queue']['items'] == [] and 'feed' not in state
    with urllib.request.urlopen(f'http://127.0.0.1:{overlay_port}/api/v1/iidx/now-playing', timeout=3) as response:
        assert json.load(response) == state['now_playing'] == dict(phase='idle', song=None, players=[], lane_order=[], playback=None)
    for table, slot, rva in [(0xd52508, 16, 0x8d7b70), (0xd52508, 17, 0x8d7f60),
                             (0xd52508, 18, 0x8d8250), (0xcbe910, 1, 0x7dc590),
                             (0xd73238, 17, 0x91a330), (0xd73238, 18, 0x91a390), (0xd7bb48, 18, 0x8beac0)]:
        hooked = ctypes.c_void_p.from_address(game + table + slot*8).value
        if lane_disabled and playback_disabled and table in (0xd73238, 0xd7bb48) and slot == 18:
            assert hooked == game + (0x91a330 if bad_lane_update and table == 0xd73238 else rva)
        else:
            assert hooked != game+rva, f'Slot {slot} was not patched'
            assert abs(hooked-plugin._handle) < 0x4000000, 'Hook is not inside plugin image'
    with urllib.request.urlopen(f'http://127.0.0.1:{overlay_port}/api/v1/meta', timeout=3) as response:
        meta = json.load(response)
        assert meta['major_version'] == 34 and meta['build'] == '010-0916', meta
    # The restored menu captures controller navigation through the verified hook.
    assert ctypes.c_void_p.from_address(game + 0xd9ff20 + 3 * 8).value != game + 0xa67d70
    if lane_disabled: assert 'Lane-order sampling disabled:' in text, text
    elif lane_generator_patch:
        assert 'patched RANDOM generator at RVA 80fb60' in text, text
        assert ctypes.string_at(lane_generator, 16) == generator_patch
    else: assert 'Lane-order sampling enabled' in text, text
    assert ('Playback sampling disabled:' if playback_disabled else 'Playback sampling enabled') in text, text
    assert ('Player sampling disabled:' if args.bad_user else 'Player sampling enabled') in text, text

if args.user or args.bad_user:
    assert not startup_rejected
    def user_snapshot():
        with urllib.request.urlopen(f'http://127.0.0.1:{overlay_port}/api/v1/iidx/user', timeout=3) as response:
            return json.load(response)
    def wait_user(check):
        until = time.monotonic() + 4
        while time.monotonic() < until:
            value = user_snapshot()
            if check(value): return value
            time.sleep(.05)
        raise AssertionError(f'Player snapshot did not converge: {value}')
    assert user_snapshot() == dict(active_side=None, players=[])
    # Synthetic data in this process only. The game entry point never runs.
    side = 1
    offset = side * 0x3e66db8
    def put(rva, data): ctypes.memmove(game + rva, data, len(data))
    put(0x328c068 + offset + 0x20, b'SMOKE\0\0\0')
    put(0x328c068 + offset + 0x34, struct.pack('<IIii', 12345, 678, 18, 17))
    put(0x70f2e00 + offset, struct.pack('<Q', 123))
    put(0x70f2e18 + offset, struct.pack('<I', 0))
    put(0x32b34d8 + offset, b'\1')
    for rva in [0x223614c, 0x2234354]: put(0x107c0f0 + offset + rva, b'\1')
    put(0x107c0f0 + offset + 0x2233e54 + 16, struct.pack('<I', 99))
    put(0x107c0f0 + offset + 0x2233e68 + 16, struct.pack('<I', 20261005))
    activity = ctypes.create_string_buffer(1024)
    struct.pack_into('<I', activity, 128 + 244 * side + 4, 3)
    struct.pack_into('<I', activity, 128 + 244 * side + 116, 4567)
    put(0xb3ea128, struct.pack('<Q', ctypes.addressof(activity)))
    for style in range(2):
        block = bytearray(124)
        struct.pack_into('<I', block, 0, 100)
        struct.pack_into('<I', block, 112, 100000)
        block[120] = 1
        put(0x107c0f0 + offset + 0x2233e7c + style * 620 + 4 * 124, bytes(block))
    put(0x107c0e8, struct.pack('<II', 1, 1))
    put(0xb60b3b0 + side * 4, struct.pack('<I', 1))
    if args.bad_user:
        time.sleep(.2)
        assert user_snapshot() == dict(active_side=None, players=[])
    else:
        profile = wait_user(lambda v: v['active_side'] == 2)['players'][0]
        assert profile['dj_name'] == 'SMOKE' and profile['sp'] == dict(dan='kaiden', dj_points=12345), profile
        assert profile['dp'] == dict(dan='chuden', dj_points=678), profile
        assert profile['weekly']['sp'] == dict(songs=103, keys=104567), profile
        assert profile['weekly']['dp'] == dict(songs=100, keys=100000), profile
        assert set(profile) == {'side','dj_name','sp','dp','weekly'}
        put(0x107c0f0 + offset + 0x2234354, b'\0')
        wait_user(lambda v: v['players'] and v['players'][0]['weekly'] is None)
    put(0xb60b3b0 + side * 4, struct.pack('<I', 0))
    wait_user(lambda v: v == dict(active_side=None, players=[]))
    put(0x107c0e8, struct.pack('<II', 0, 0))
    print('PASS player snapshot: guards, real HTTP, synthetic 2P profile/week delta, missing data, logout')

if song_info:
    assert not startup_rejected and not occupied_port
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
        data[0x3dc] = 34
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
    for rva, value in [(0x8d7b70, 71), (0x8d7f60, 72), (0x8d8250, 73),
                       (0x91a2c0, 74), (0x91a330, 75), (0x91a390, 76), (0x81a4f0, 0),
                       (0x7f0860, 0), (0x7bf560, ctypes.addressof(reservation)),
                       (0x5fa200, 1), (0x5fa370, ctypes.addressof(selected_record)),
                       (0x5fa3d0, 3), (0x92fcc0, 1)]:
        return_value(rva, value)
    # The original database accessor remains guarded and returns an empty header,
    # so catalog startup stays pending while display metadata must still work.
    scene = ctypes.create_string_buffer(9000)
    controller = ctypes.create_string_buffer(64)
    ctypes.c_void_p.from_buffer(scene).value = game + 0xd52508
    ctypes.c_uint32.from_buffer(scene, 88).value = 3
    ctypes.c_int32.from_buffer(scene, 136).value = 1000
    ctypes.c_void_p.from_buffer(scene, 152).value = ctypes.addressof(controller)
    ctypes.c_void_p.from_buffer(controller, 8).value = game + 0x8da060
    stage = ctypes.create_string_buffer(16)
    ctypes.c_void_p.from_buffer(stage).value = game + 0xd73238
    # The old +8 field is an analyzer job, not a song record. Leave a distinct
    # non-song object there so this fixture cannot bless the old layout again.
    stage_job = ctypes.create_string_buffer(0x7f8)
    ctypes.c_void_p.from_address(game + 0xb4e0088).value = ctypes.addressof(stage_job)
    ctypes.c_void_p.from_address(game + 0xb4e0098).value = ctypes.addressof(stage_record)
    for side in range(2):
        ctypes.c_uint32.from_address(game + 0xb60b3a8 + side * 4).value = 3
        ctypes.c_uint32.from_address(game + 0xb60b3b0 + side * 4).value = 1
    options = ctypes.create_string_buffer(572)
    ctypes.c_void_p.from_buffer(options).value = game + 0xd23de0
    ctypes.c_void_p.from_address(game + 0xb3e9f28).value = ctypes.addressof(options)

    def lane_table(side, values):
        for key, destination in enumerate(values):
            ctypes.c_uint32.from_address(game + 0xb1259a0 + side * 32 + key * 4).value = destination

    def playback_clock(frame, fps=120.0):
        ctypes.c_int32.from_address(game + 0xb1281c0 + 0x2a29f8).value = frame
        ctypes.c_float.from_address(game + 0xb1281c0 + 0x2a2a10).value = fps
        ctypes.c_int32.from_address(game + 0xb3e3b90 + 96).value = 18000

    playback_clock(0)

    for side in range(2):
        lane_table(side, range(8))
    callback = ctypes.CFUNCTYPE(ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_size_t, ctypes.c_size_t)

    def invoke(instance, slot):
        table = ctypes.c_void_p.from_buffer(instance).value
        address = ctypes.c_void_p.from_address(table + slot * 8).value
        return callback(address)(ctypes.addressof(instance), 0, 0, 0)

    def wait_song(phase, title, chart_id=None, keys=None, lane_status=None, position=None):
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            with urllib.request.urlopen(f'http://127.0.0.1:{overlay_port}/api/v1/iidx/now-playing', timeout=2) as response:
                data = json.load(response)
            if (data['phase'] == phase and (data['song'] or {}).get('title') == title
                    and (chart_id is None or data['players'][0]['chart']['id'] == chart_id)
                    and (keys is None or data['lane_order'][0]['keys'] == keys)
                    and (lane_status is None or data['lane_order'][0]['status'] == lane_status)
                    and (position is None or (data.get('playback') or {}).get('position_ms') == position)):
                return data
            time.sleep(.05)
        raise AssertionError(f'Song snapshot not published: {phase} {title}: {data}')

    assert invoke(scene, 16) == 71
    assert invoke(scene, 18) == 73
    data = wait_song('selecting', 'Manual selection fixture')
    assert data['playback'] is None
    assert [p['side'] for p in data['players']] == [1, 2]
    chart = data['song']['charts'][0]
    assert chart['bpm'] == dict(min=100, max=200) and chart['note_count'] == 1234
    assert chart['radar']['notes'] == 150.25
    assert chart['density'] is None
    assert chart['lane_counts'] is None
    if lane_disabled:
        assert data['lane_order'] == []
    else:
        assert data['lane_order'][0]['keys'] == [1, 2, 3, 4, 5, 6, 7]
        assert data['lane_order'][0]['random'] == 'off'
    # The native selection job publishes model +144, under the same shared_ptr lock.
    model = ctypes.create_string_buffer(160)
    ctypes.c_void_p.from_address(game + 0xb3e9fa0).value = ctypes.addressof(model)
    publication_lock = ctypes.c_uint32.from_address(game + 0xc51b324)
    publication_lock.value = 1
    time.sleep(.3)
    invoke(scene, 18)
    assert publication_lock.value == 1  # busy: no wait or unlock
    publication_lock.value = 0
    # Publish two distinct native difficulty histograms on the next frame.
    analyzer = ctypes.create_string_buffer(72)
    ctypes.c_void_p.from_buffer(analyzer).value = game + 0xc8b2d8
    ctypes.c_uint32.from_buffer(analyzer, 8).value = 33001
    native_histograms = []
    for side, (index, notes, scratch) in enumerate([(3, [0, 2, 42, 1], [0, 0, 2, 1]), (8, [3, 4], [1, 2])]):
        ctypes.c_uint32.from_buffer(analyzer, 12 + side * 4).value = index
        detail = ctypes.create_string_buffer(116)
        for offset, values in [(0, notes), (24, scratch)]:
            vector = (ctypes.c_uint32 * len(values))(*values)
            begin = ctypes.addressof(vector)
            struct.pack_into('<QQQ', detail, offset, begin, begin + len(values) * 4, begin + len(values) * 4)
            native_histograms.append(vector)
        ctypes.c_uint32.from_buffer(detail, 112).value = 6500
        lane_totals = ([1, 2, 3, 4, 5, 6, 21, 3] + [0] * 8 if index == 3
                       else [1, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 2])
        struct.pack_into('<16I', detail, 48, *lane_totals)
        ctypes.c_void_p.from_buffer(analyzer, 24 + side * 24).value = ctypes.addressof(detail)
        analyzer[40 + side * 24] = b'\x01'
        native_histograms.append(detail)
    ctypes.c_void_p.from_buffer(model, 144).value = ctypes.addressof(analyzer)
    time.sleep(.3)
    invoke(scene, 18)
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        data = wait_song('selecting', 'Manual selection fixture')
        if data['song']['charts'][0]['density'] is not None:
            break
        time.sleep(.05)
    assert data['song']['charts'][0]['density'] == dict(bin_ms=1000, duration_ms=6500, notes=[0, 2, 42, 1], scratch=[0, 0, 2, 1])
    assert data['song']['charts'][1]['density']['notes'] == [3, 4]
    assert data['song']['charts'][0]['lane_counts'] == dict(sides=[dict(side=1, keys=[1,2,3,4,5,6,21], scratch=3)])
    assert data['song']['charts'][1]['lane_counts'] == dict(sides=[
        dict(side=1, keys=[1,0,1,0,0,0,0], scratch=1), dict(side=2, keys=[0,1,0,1,0,0,0], scratch=2)])
    with urllib.request.urlopen(f'http://127.0.0.1:{overlay_port}/api/v1/iidx/lane-counts', timeout=2) as response:
        counts_api = json.load(response)
    assert counts_api['song_id'] == 33001 and counts_api['basis'] == 'original_chart'
    assert counts_api['charts'][0]['lane_counts'] == data['song']['charts'][0]['lane_counts']
    assert publication_lock.value == 0
    # A later analyzer can expose just one chart; merge it without losing the
    # other chart or freezing the first publication for this song.
    ctypes.c_uint32.from_buffer(analyzer, 16).value = 11
    native_histograms[0][1] = 7
    ctypes.c_uint32.from_buffer(native_histograms[2], 48).value = 6
    time.sleep(.3)
    invoke(scene, 18)
    time.sleep(.15)
    merged = wait_song('selecting', 'Manual selection fixture')
    assert merged['song']['charts'][0]['density']['notes'] == [0, 7, 42, 1]
    assert merged['song']['charts'][1]['density']['notes'] == [3, 4]
    # Cache contains owned arrays: the native analyzer can disappear before stage init.
    ctypes.c_void_p.from_address(game + 0xb3e9fa0).value = 0
    # DP and two-player SP share a double layout but use different chart indices.
    return_value(0x81a4f0, 1)
    ctypes.c_uint32.from_address(game + 0xb60b3a4).value = 1
    time.sleep(.3)
    invoke(scene, 18)
    wait_song('selecting', 'Manual selection fixture', 'DPA')
    ctypes.c_uint32.from_address(game + 0xb60b3a4).value = 0
    time.sleep(.3)
    invoke(scene, 18)
    wait_song('selecting', 'Manual selection fixture', 'SPA')
    # A folder must clear the selected song despite the native record fallback.
    return_value(0x5fa200, 0)
    time.sleep(.3)
    assert invoke(scene, 18) == 73
    wait_song('selecting', None)
    assert invoke(scene, 17) == 72
    assert wait_song('idle', None)['lane_order'] == []
    ctypes.c_void_p.from_address(game + 0xb4e0098).value = ctypes.addressof(selected_record)
    ctypes.c_uint32.from_buffer(options, 8 + 48).value = 1
    lane_table(0, [4, 3, 0, 1, 2, 5, 6, 7])
    assert invoke(stage, 16) == 74
    cached = wait_song('playing', 'Manual selection fixture')
    assert cached['playback'] == (None if playback_disabled else dict(position_ms=0, duration_ms=150000))
    if not playback_disabled:
        for frame in [600, 1200, 1200, 0, 17999]:  # advance, pause, retry, end
            playback_clock(frame)
            time.sleep(.3)
            assert invoke(stage, 18) == 76
            current = wait_song('playing', 'Manual selection fixture', position=round(frame * 1000 / 120))
            assert current['playback']['duration_ms'] == 150000
            with urllib.request.urlopen(f'http://127.0.0.1:{overlay_port}/api/v1/state', timeout=2) as response:
                assert json.load(response)['now_playing']['playback'] == current['playback']
        playback_clock(600, float('nan'))
        time.sleep(.3)
        invoke(stage, 18)
        time.sleep(.3)
        assert wait_song('playing', 'Manual selection fixture')['playback'] is None
        playback_clock(0)
        time.sleep(.3)
        invoke(stage, 18)
        wait_song('playing', 'Manual selection fixture', position=0)
        print('PASS: native playback clock -> both HTTP snapshots; advance, pause, retry, invalid clock clearing and recovery')
    assert cached['song']['charts'][0]['density']['notes'] == [0, 7, 42, 1]
    assert cached['song']['charts'][0]['lane_counts']['sides'][0]['keys'] == [6,2,3,4,5,6,21]
    with urllib.request.urlopen(f'http://127.0.0.1:{overlay_port}/api/v1/iidx/lane-order', timeout=2) as response:
        order_api = json.load(response)
    assert order_api['song_id'] == 33001 and order_api['lane_order'] == cached['lane_order']
    if lane_disabled:
        assert cached['lane_order'] == []
    else:
        assert cached['lane_order'][0]['keys'] == [3, 4, 5, 2, 1, 6, 7]
        assert cached['lane_order'][0]['random'] == 'random'
        # Successful sound loading also sets this general stage flag to 1.
        # Several subsequent gameplay samples must retain the current layout.
        ctypes.c_ubyte.from_address(game + 0xb3e7e2e).value = 1
        for _ in range(3):
            time.sleep(.3)
            assert invoke(stage, 18) == 76
        time.sleep(.2)  # Let the worker publish the post-update snapshot.
        wait_song('playing', 'Manual selection fixture', keys=[3, 4, 5, 2, 1, 6, 7], lane_status='ready')
        # Same-stage retry replaces the owned map; an invalid read clears it.
        lane_table(0, [6, 5, 4, 3, 2, 1, 0, 7])
        time.sleep(.3)
        assert invoke(stage, 18) == 76
        wait_song('playing', 'Manual selection fixture', keys=[7, 6, 5, 4, 3, 2, 1])
        lane_table(0, [0] * 8)
        time.sleep(.3)
        assert invoke(stage, 18) == 76
        invalid = wait_song('playing', 'Manual selection fixture', lane_status='unavailable')
        assert invalid['lane_order'][0]['keys'] is None
        ctypes.c_uint32.from_buffer(options, 8 + 48).value = 3
        time.sleep(.3)
        invoke(stage, 18)
        dynamic = wait_song('playing', 'Manual selection fixture', lane_status='dynamic')
        assert dynamic['lane_order'][0]['keys'] is None
    assert invoke(stage, 17) == 75
    cleared = wait_song('idle', None)
    assert cleared['lane_order'] == [] and cleared['playback'] is None
    ctypes.c_void_p.from_address(game + 0xb4e0098).value = ctypes.addressof(stage_record)
    assert invoke(stage, 16) == 74
    different = wait_song('playing', 'Actual stage fixture')
    assert all(c['density'] is None for c in different['song']['charts'])
    assert all(c['lane_counts'] is None for c in different['song']['charts'])
    if not (lane_disabled and playback_disabled):
        # The stage's analysis job may complete after initialization returns.
        ctypes.c_uint32.from_buffer(analyzer, 8).value = 33002
        ctypes.c_void_p.from_address(game + 0xb3e9fa0).value = ctypes.addressof(model)
        time.sleep(.3)
        invoke(stage, 18)
        time.sleep(.15)
        late = wait_song('playing', 'Actual stage fixture')
        assert late['song']['charts'][0]['density']['notes'] == [0, 7, 42, 1]
        assert late['song']['charts'][1]['density'] is None
    assert invoke(stage, 17) == 75
    wait_song('idle', None)
    if lane_disabled:
        print('PASS: incompatible lane sampler isolated; web, song/density callbacks, stage cleanup and core hooks remain available')
    else:
        print('PASS: real DLL callbacks -> song/density/lane JSON, mapping direction, sustained gameplay after sound loading, same-stage retry, invalid/dynamic lane clearing, two-chart analyzer, same-song merge, late stage publication, busy lock, SP/DP, cached stage, folders, cleanup and preserved returns')

if args.queue:
    def api(path, method='GET', body=None):
        request = urllib.request.Request(f'http://127.0.0.1:{overlay_port}/api/v1/{path}', method=method,
            data=None if body is None else json.dumps(body).encode(), headers={} if body is None else {'Content-Type':'application/json'})
        with urllib.request.urlopen(request, timeout=3) as response: return json.load(response)
    def await_queue(predicate, advance=False):
        for _ in range(60):
            if advance: invoke(scene, 18)
            queue = api('queue')
            if predicate(queue): return queue
            time.sleep(.05)
        raise AssertionError(f'Queue transition missing: {queue}')
    assert api('songs?q=fixture%20alias')['songs'][0]['id'] == 33001
    assert api('aliases/reload','POST') == {'count':1}
    (work / 'bilimani_iidx_aliases.csv').write_text('alias,target\ninvalid,99999\n', encoding='utf-8')
    try: api('aliases/reload','POST')
    except urllib.error.HTTPError as error: assert error.code == 422
    else: raise AssertionError('Invalid alias replacement accepted')
    assert api('songs?q=fixture%20alias')['songs'][0]['id'] == 33001
    return_value(0x81a4f0,0)
    return_value(0x5fa200,1)
    return_value(0x7bf330,1)  # reservation available
    return_value(0x7bf5d0,0)  # synthetic update already exposes the chosen record
    invoke(scene,16);invoke(scene,18)
    time.sleep(.1)
    first=api('queue','POST',{'song_id':33001,'mode':'SP','requester':'Viewer'})['id']
    await_queue(lambda q: q['current'] and q['current']['id']==first,True)
    second=api('queue','POST',{'song_id':33001,'mode':'SP'})['id']
    return_value(0x7bf330,0)
    api(f'queue/{second}/priority','POST')
    await_queue(lambda q: q['items'][0]['executing'])
    try: api(f'queue/{second}','DELETE')
    except urllib.error.HTTPError as error: assert error.code == 409
    else: raise AssertionError('Executing item was deleted')
    failed=await_queue(lambda q: q['items'][0]['error'],True)
    assert failed['current']['id']==first
    return_value(0x7bf330,1)
    api(f'queue/{second}/priority','POST')
    await_queue(lambda q: q['current']['id']==second,True)
    api(f'queue/{second}','DELETE')
    assert api('queue')['current'] is None
    return_value(0xa67d70,78)
    input_fixture=ctypes.create_string_buffer(0x68)
    ctypes.c_void_p.from_buffer(input_fixture).value=game+0xd9ff20
    original_input=bytes(input_fixture)
    assert invoke(input_fixture,3)==78 and bytes(input_fixture)==original_input
    print('PASS: DLL queue -> native ack/failure/retry, busy deletion, priority replacement, CSV reload rollback and closed-menu input forwarding')

# SDK v0 initializes menu rendering/input callbacks without requiring a D3D device.
destroy_callback = None
GET_BUTTON = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_uint32, ctypes.POINTER(ctypes.c_bool), ctypes.c_void_p)
@GET_BUTTON
def get_button(button, pressed, velocity):
    pressed[0] = False
    return 0
DRAW = ctypes.CFUNCTYPE(None, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p)
REGISTER = ctypes.CFUNCTYPE(ctypes.c_int, DRAW, ctypes.c_void_p)
@REGISTER
def register(draw, user):
    for event in [0, 1, 2, 3]: draw(event, None, user)
    return 0
class SdkV0(ctypes.Structure):
    _fields_ = [('size', ctypes.c_uint32), ('functions', ctypes.c_void_p * 18)]
INIT = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p)
@INIT
def sdk_init(version, destroy, table):
    global destroy_callback
    assert version == 0
    api = SdkV0.from_address(table)
    assert api.size == ctypes.sizeof(SdkV0)
    api.functions[3] = ctypes.cast(get_button, ctypes.c_void_p).value
    api.functions[17] = ctypes.cast(register, ctypes.c_void_p).value
    destroy_callback = ctypes.CFUNCTYPE(None)(destroy)
    return 0
plugin.spice_sdk_entry_point.argtypes = [INIT]
plugin.spice_sdk_entry_point.restype = ctypes.c_int
assert plugin.spice_sdk_entry_point(sdk_init) == 0 and destroy_callback
destroy_callback()
if not startup_rejected:
    try: urllib.request.urlopen(f'http://127.0.0.1:{overlay_port}/api/v1/state', timeout=1)
    except urllib.error.URLError: pass
    else: raise AssertionError('Game API still running after shutdown')
assert not (work / 'bilimani.db').exists() and not (work / 'obs').exists()
port_blocker.close()
print(f'PASS: DLL startup, API boundaries and SDK shutdown ({work.name})')
