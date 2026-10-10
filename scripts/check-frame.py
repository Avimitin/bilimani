"""Check the stream frame with local fixtures. Requires Playwright and Pillow.

Run against overlay_preview, or supply --url for a static asset server.
Screenshots stay in analysis/stream-frame/.
"""
import argparse
import copy
import io
import math
import pathlib

from PIL import Image, ImageChops, ImageDraw, ImageStat
from playwright.sync_api import expect, sync_playwright

ROOT = pathlib.Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--url', default='http://127.0.0.1:32133')
parser.add_argument('--browser')
args = parser.parse_args()
shots = ROOT / 'analysis/stream-frame'
shots.mkdir(parents=True, exist_ok=True)
song = dict(token=1, title='X-DEN', requester='音游打字机', mode='SP', chart='SPA', chart_style='red', remaining=480, duration=600)
live = dict(connected=True, ready=True, status='弹幕已连接', capacity=20, current=song,
            room=dict(room_id=12345, name='CookieBacon', title='IIDX 点歌'),
            queue=[dict(song, token=i+2, title=title, requester=name, chart=chart, chart_style=tone) for i, (title, name, chart, tone) in enumerate([
                ('sakura storm', '春日来信', 'SPH', 'amber'), ('Somnidiscotheque', '夜航星', 'SPA', 'red'),
                ('INFINITAS', '像素海', 'SPL', 'purple'), ('冥', 'GreenTea', 'SPA', 'red'), ('雪月花', '月下', 'SPH', 'amber')])],
            pending=[], feed_limit=10, feed=[
                dict(id=1, kind='chat', name='春日来信', text='晚上好！今天也来听歌了。'),
                dict(id=2, kind='event', text='已加入队列：sakura storm [SPH]'),
                dict(id=3, kind='chat', name='夜航星', text='这个灰色机甲框架好有氛围'),
                dict(id=4, kind='event', text='已加入队列：Somnidiscotheque [SPA]'),
                dict(id=5, kind='chat', name='像素海', text='这一段也太好听了吧！'),
                dict(id=6, kind='event', text='已加入队列：INFINITAS [SPL]'),
                dict(id=7, kind='chat', name='GreenTea', text='加油！这把一定能过。')])
payload = copy.deepcopy(live)
fail = False
with sync_playwright() as p:
    browser = p.chromium.launch(headless=True, **({'executable_path': args.browser} if args.browser else {}))
    page = browser.new_page(viewport={'width': 1920, 'height': 1080}, device_scale_factor=1)
    errors = []
    page.on('pageerror', lambda error: errors.append(str(error)))
    def state(route):
        if fail:
            route.abort()
        else:
            route.fulfill(json=payload)
    page.route('**/api/state', state)
    page.goto(args.url + '/queue')
    expect(page.locator('.current-title')).to_have_text('X-DEN')
    title_box = page.locator('.current-title').bounding_box()
    expect(page.locator('.current-song .chart')).to_have_count(0)
    expect(page.locator('#activity-list .activity-row')).to_have_count(3)
    expect(page.locator('#event-list .activity-row')).to_have_count(3)
    expect(page.locator('#queue-more')).to_have_text('另有 1 首等待中')
    expect(page.locator('body')).to_have_css('background-color', 'rgba(0, 0, 0, 0)')
    expect(page.locator('.chassis')).to_have_css('clip-path', 'none')
    expect(page.locator('.chassis')).to_have_css('mask-image', 'none')
    expect(page.locator('.station-header #request-hint')).to_have_text('点歌 <曲名> [难度]')
    expect(page.locator('.frame-current #request-hint, .queue-panel #request-hint')).to_have_count(0)
    expect(page.locator('#room-name')).to_have_text('CookieBacon')
    expect(page.locator('.bilibili-icon')).to_have_count(1)
    expect(page.locator('.station-header code')).to_have_css('text-align', 'center')
    expect(page.locator('.station-room')).to_have_css('justify-content', 'center')
    # Anchor names update with the active room, render literally, and never push
    # the persistent request format out of the narrow header.
    long_name = '<img src=x onerror="window.injected=true">' + '很长的主播名字' * 10
    payload['room']['name'] = long_name
    expect(page.locator('#room-name')).to_have_text(long_name)
    expect(page.locator('#room-name')).to_have_attribute('title', long_name)
    assert page.locator('img').count() == 0 and not page.evaluate('Boolean(window.injected)')
    header = page.locator('.station-header').bounding_box()
    hint = page.locator('#request-hint').bounding_box()
    anchor = page.locator('#room-name').bounding_box()
    assert hint['y'] + hint['height'] <= anchor['y']
    expect(page.locator('.station-header code')).to_have_css('font-size', '20px')
    expect(page.locator('#room-name')).to_have_css('font-size', '22px')
    assert page.locator('.sidebar').bounding_box()['y'] >= header['y'] + header['height']
    assert anchor['x'] + anchor['width'] <= header['x'] + header['width']
    payload['connected'] = False
    expect(page.locator('#room-name')).to_have_text('未连接')
    payload.update(connected=True, room=None)
    expect(page.locator('#room-name')).to_be_hidden()
    del payload['room']  # Older DLLs do not provide this field.
    expect(page.locator('#room-name')).to_be_hidden()
    payload.update(ready=False, room=dict(room_id=67890, name='另一个主播', title='新直播间'))
    expect(page.locator('#room-name')).to_have_text('另一个主播')
    expect(page.locator('.connection-banner')).to_be_visible()
    payload.update(ready=True, room=copy.deepcopy(live['room']))
    expect(page.locator('#room-name')).to_have_text('CookieBacon')
    expect(page.locator('.connection-banner')).to_be_hidden()
    event_box = page.locator('.event-panel').bounding_box()
    chat_box = page.locator('#activity-panel').bounding_box()
    expect(page.locator('#activity-list')).to_have_css('row-gap', '4px')
    expect(page.locator('#activity-list .activity-row').first).to_have_css('border-bottom-width', '1px')
    expect(page.locator('#activity-list .activity-text').first).to_have_css('font-size', '20px')
    assert page.locator('#activity-list .activity-row').first.bounding_box()['height'] <= 63

    def bounded():
        page.wait_for_function("Number(document.querySelector('.stream-frame').style.getPropertyValue('--frame-scale')) === Math.min(innerWidth / 1920, innerHeight / 1080)")
        assert page.evaluate('document.documentElement.scrollWidth <= innerWidth && document.documentElement.scrollHeight <= innerHeight')
        corner = page.locator('#corner-module').bounding_box()
        scale = min(page.viewport_size['width'] / 1920, page.viewport_size['height'] / 1080)
        assert corner['width'] / scale >= 300 and corner['height'] / scale >= 330
        disc = page.locator('#turntable-disc').bounding_box()
        assert abs(disc['width'] / scale - 240) < .1
        capture = page.locator('#game-capture').bounding_box()
        scale = min(page.viewport_size['width'] / 1920, page.viewport_size['height'] / 1080)
        for key, expected in dict(x=40, y=24, width=1504, height=846).items():
            assert abs(capture[key] - expected * scale) < .01, (key, capture)
        assert abs(capture['width'] / capture['height'] - 16 / 9) < 1e-6
        assert 20 <= page.locator('#top-rail').bounding_box()['height'] / scale <= 21  # Includes the edge stroke.
        bottom = page.locator('.frame-current').bounding_box()
        assert abs(bottom['y'] / scale - 890) < .1 and abs(bottom['height'] / scale - 178) < .1
        for selector, container in [('.queue-row', '#queue-body'), ('.candidate', '#pending-list'),
                                    ('#activity-list .activity-row', '.feed-body'), ('#event-list .activity-row', '#event-list'),
                                    ('.current-title, .current-song .chart, .current-meta', '.frame-current')]:
            outer = page.locator(container).bounding_box()
            for item in page.locator(selector).all():
                if not item.is_visible():
                    continue
                box = item.bounding_box()
                assert box['x'] >= outer['x'] - 1 and box['y'] >= outer['y'] - 1, selector
                assert box['x'] + box['width'] <= outer['x'] + outer['width'] + 1, selector
                assert box['y'] + box['height'] <= outer['y'] + outer['height'] + 1, selector

    def capture_clear(shot):
        box = page.locator('#game-capture').bounding_box()
        # The enlarged turntable intentionally overlaps the lower-left capture;
        # every other complete physical pixel must remain transparent.
        bounds = (math.ceil(box['x']), math.ceil(box['y']),
                  math.floor(box['x'] + box['width']), math.floor(box['y'] + box['height']))
        alpha = shot.getchannel('A').copy()
        scale = box['width'] / 1504
        alpha.paste(0, (0, math.floor(780 * scale), math.ceil(306 * scale), math.ceil(870 * scale)))
        assert alpha.crop(bounds).getextrema() == (0, 0), 'Frame obscures capture outside the turntable corner'
        assert shot.getpixel((round(132 * scale), round(850 * scale)))[3] > 0

    def transparent():
        png = page.screenshot(omit_background=True)
        shot = Image.open(io.BytesIO(png)).convert('RGBA')
        capture_clear(shot)
        # All four sides meet the opening: the transparent area has no extra
        # strips that would suggest stretching or cropping the game source.
        alpha = shot.getchannel('A')
        for edge in [(39, 24, 40, 870), (1544, 24, 1545, 870),
                     (40, 23, 1544, 24), (40, 870, 1544, 871)]:
            assert alpha.crop(edge).getextrema() == (255, 255), edge
        assert shot.getpixel((32, 44))[3] == 255
        # Armor/glow must stay outside even the capture's corner regions.
        for region in [(40, 24, 66, 40), (1480, 854, 1544, 870)]:
            assert shot.crop(region).getchannel('A').getextrema() == (0, 0), f'Unexpected corner backing: {region}'
        assert shot.getpixel((1600, 500))[3] == 255
        # The drawn chassis metal carries a slight blue tint around cyan VFD
        # glass and illuminated joints; only strong color casts are rejected.
        for region in [(200, 0, 500, 20)]:
            neutral = shot.crop(region)
            for first, second in [('R', 'G'), ('G', 'B')]:
                drift = ImageChops.difference(neutral.getchannel(first), neutral.getchannel(second))
                assert drift.getextrema()[1] <= 14
        r, g, b, _ = shot.getpixel((244, 1054))
        assert g > r + 30 and b > r + 20
        assert page.locator('#lamp-ankle').count() == 0
        assert page.locator('#lamp-top').evaluate('el => el.getBBox().height') == 0
        return png

    bounded()
    live_png = transparent()
    (shots / 'frame-live.png').write_bytes(live_png)
    # Armor, corner modules and display housings must form one connected assembly,
    # not separate opaque islands on the transparent OBS canvas.
    assembly = Image.open(io.BytesIO(live_png)).getchannel('A').point(lambda alpha: 255 if alpha >= 128 else 0)
    ImageDraw.floodfill(assembly, (1700, 950), 128)
    for point in [(14, 990), (800, 5), (9, 430), (1575, 430), (1700, 56), (1700, 600), (1905, 420)]:
        assert assembly.getpixel(point) == 128, f'Disconnected chassis part: {point}'
    # Viewer strings are text, even if they contain HTML.
    injection = '<img src=x onerror="window.injected=true">'
    payload['current']['title'] = injection + '长曲名' * 40
    payload['feed'][-1]['text'] = injection
    expect(page.locator('.current-title')).to_have_text(payload['current']['title'])
    expect(page.locator('#activity-list')).to_contain_text(injection)
    assert page.locator('img').count() == 0 and not page.evaluate('Boolean(window.injected)')
    bounded()
    payload['current']['chart'] = None
    expect(page.locator('.current-song .chart')).to_have_count(0)
    bounded()
    payload['current'] = copy.deepcopy(song)
    payload['feed'] = copy.deepcopy(live['feed'])
    choice = dict(requester='观众甲', mode='SP', chart='SPA', chart_style='red', remaining=45, duration=60)
    for count in [1, 5, 8, 9, 16, 20]:
        payload['pending'] = [dict(choice, candidates=[dict(title=f'{i+1} / 很长的候选曲名 ' * 4, available=i != 1) for i in range(count)])]
        page_count = (count + 4) // 5
        for candidate_page in range(page_count):
            payload['pending'][0]['page'] = candidate_page
            first, last = candidate_page * 5 + 1, min((candidate_page + 1) * 5, count)
            expect(page.locator('.candidate-number')).to_have_text([str(i) for i in range(first, last + 1)])
            if page_count > 1:
                expect(page.locator('.candidate-page')).to_have_text(f'{candidate_page + 1} / {page_count}')
                expect(page.locator('.choice-pagination')).to_contain_text('n 下一页')
                expect(page.locator('.choice-pagination')).to_contain_text('p 上一页')
            else:
                expect(page.locator('.choice-pagination')).to_have_count(0)
            boxes = [item.bounding_box() for item in page.locator('.candidate').all()]
            assert all(abs(a['x'] - b['x']) < 1 and a['y'] + a['height'] <= b['y'] for a, b in zip(boxes, boxes[1:]))
            expect(page.locator('.current-title')).to_be_visible()
            expect(page.locator('#queue-area')).to_be_hidden()
            bounded()
            transparent()
    page.reload()
    expect(page.locator('.candidate-number')).to_have_text(['16', '17', '18', '19', '20'])
    expect(page.locator('.candidate-page')).to_have_text('4 / 4')
    page.screenshot(path=str(shots / 'frame-choices.png'), omit_background=True)
    # A scan travels through the background; neither the panel nor text pulses.
    def scan_at(milliseconds):
        return page.locator('.choice-scan-band').evaluate('''(el, time) => {
            const scan = el.getAnimations().find(a => a.animationName === 'choice-scan');
            scan.pause(); scan.currentTime = time;
            const style = getComputedStyle(el);
            return {position: style.transform, opacity: style.opacity};
        }''', milliseconds)

    def scan_contained():
        # Compare every exterior pixel with the effect hidden. Sample the actual
        # glass geometry, allowing one physical pixel for antialiased edges.
        # Isolate the SVG effect: Chromium can rerasterize overlaid HTML borders
        # at fractional scaling when this SVG layer changes visibility.
        backdrop = page.locator('.chassis, .sidebar > .panel, .sidebar-face > .screen-bezel, .sidebar-face > .screen-glass, .sidebar-face > .screen-highlight')
        backdrop.evaluate_all("els => els.forEach(el => el.style.visibility = 'hidden')")
        geometry = page.locator('.sidebar-face').evaluate('''svg => {
            const box = svg.getBoundingClientRect(), scale = box.width / 290;
            const shape = svg.querySelector('#sidebar-glass-shape');
            const left = Math.floor(box.x) - 2, top = Math.floor(box.y) - 2;
            const width = Math.ceil(box.width) + 4, height = Math.ceil(370 * scale) + 4;
            const outside = [];
            for (let y = 0; y < height; y++) {
                let start = null;
                for (let x = 0; x <= width; x++) {
                    let inside = false;
                    if (x < width) {
                        for (const [dx, dy] of [[0,0],[-1,-1],[1,-1],[-1,1],[1,1]]) {
                            const px = (left + x + .5 + dx - box.x) / scale;
                            const py = (top + y + .5 + dy - box.y) / scale;
                            if (py >= 0 && py <= 370 && shape.isPointInFill(new DOMPoint(px, py))) {
                                inside = true; break;
                            }
                        }
                    }
                    if (x < width && !inside && start === null) start = x;
                    if ((inside || x === width) && start !== null) {
                        outside.push([start, y, x, y + 1]); start = null;
                    }
                }
            }
            return {left, top, width, height, outside};
        }''')
        box = (geometry['left'], geometry['top'], geometry['left'] + geometry['width'], geometry['top'] + geometry['height'])
        exterior = Image.new('L', (geometry['width'], geometry['height']))
        for run in geometry['outside']:
            exterior.paste(255, tuple(run))
        effect = page.locator('.choice-scan-surface')
        scan_at(0)
        effect.evaluate("el => el.style.visibility = 'hidden'")
        baseline = Image.open(io.BytesIO(page.screenshot(omit_background=True))).convert('RGB').crop(box)
        effect.evaluate("el => el.style.removeProperty('visibility')")
        for phase in [0, 200, 600, 1500, 2200, 2500]:
            scan_at(phase)
            frame = Image.open(io.BytesIO(page.screenshot(omit_background=True))).convert('RGB').crop(box)
            difference = ImageChops.difference(baseline, frame)
            leaked = Image.new('RGB', difference.size)
            leaked.paste(difference, mask=exterior)
            # Fractional viewport scaling can round an antialiased channel by 1.
            strongest = max(high for low, high in leaked.getextrema())
            if strongest > 1:
                baseline.save(shots / 'scan-baseline.png')
                frame.save(shots / 'scan-frame.png')
                leaked.save(shots / 'scan-leaked.png')
                exterior.save(shots / 'scan-exterior.png')
            assert strongest <= 1, f'Scan escaped glass at {phase} ms, viewport {page.viewport_size}, pixels {leaked.getbbox()}'
        backdrop.evaluate_all("els => els.forEach(el => el.style.removeProperty('visibility'))")

    scan_contained()
    top = scan_at(600)
    first_scan = Image.open(io.BytesIO(transparent())).convert('RGB').crop((1604, 152, 1872, 522))
    middle = scan_at(1500)
    assert top['position'] != middle['position'] and top['opacity'] == middle['opacity'] == '1'
    expect(page.locator('.choice-prompt')).to_have_css('opacity', '1')
    scan_png = transparent()
    next_scan = Image.open(io.BytesIO(scan_png)).convert('RGB').crop((1604, 152, 1872, 522))
    assert min(ImageStat.Stat(ImageChops.difference(first_scan, next_scan)).mean) > 3
    (shots / 'frame-choices-scan.png').write_bytes(scan_png)
    page.emulate_media(reduced_motion='reduce')
    expect(page.locator('.choice-scan-band')).to_have_css('animation-duration', '8s')
    page.emulate_media(reduced_motion='no-preference')
    for width, height, transform in [(1280, 720, 'matrix(0.666667, 0, 0, 0.666667, 0, 0)'), (960, 540, 'matrix(0.5, 0, 0, 0.5, 0, 0)')]:
        page.set_viewport_size(dict(width=width, height=height))
        expect(page.locator('.stream-frame')).to_have_css('transform', transform)
        bounded()
        scan_contained()
        capture_clear(Image.open(io.BytesIO(page.screenshot(omit_background=True))).convert('RGBA'))
    page.set_viewport_size(dict(width=1920, height=1080))
    expect(page.locator('.stream-frame')).to_have_css('transform', 'matrix(1, 0, 0, 1, 0, 0)')
    payload['pending'][0]['remaining'] = 9
    expect(page.locator('.countdown.urgent b')).to_have_text('09')
    payload['pending'].append(dict(choice, requester='观众乙', candidates=[dict(title=f'Other {i+1}', available=True) for i in range(9)]))
    expect(page.locator('#pending-page')).to_contain_text('1 / 2')
    expect(page.locator('.choice-heading h2')).to_have_text('观众乙，请选择', timeout=9000)
    payload['pending'][0]['page'] = 1
    expect(page.locator('.choice-heading h2')).to_have_text('观众甲，请选择')
    expect(page.locator('.candidate-number')).to_have_text(['6', '7', '8', '9', '10'])
    payload['pending'][1]['page'] = 1
    expect(page.locator('.choice-heading h2')).to_have_text('观众乙，请选择')
    expect(page.locator('.candidate-number')).to_have_text(['6', '7', '8', '9'])
    payload['pending'][1]['page'] = 0
    expect(page.locator('.candidate-number')).to_have_text(['1', '2', '3', '4', '5'])
    payload['pending'] = []
    payload['feed_limit'] = 100
    payload['feed'] = [dict(id=i, kind='chat' if i % 2 else 'event', name='观众', text='很长的消息 ' * 80) for i in range(100)]
    expect(page.locator('#feed-count')).to_have_text('3 / 50')
    expect(page.locator('#event-count')).to_have_text('3 / 50')
    expect(page.locator('#activity-list .activity-row').last).to_have_attribute('data-id', '99')
    expect(page.locator('#event-list .activity-row').last).to_have_attribute('data-id', '98')
    bounded()
    expect(page.locator('.choice-scan-band')).to_have_css('animation-name', 'none')
    expect(page.locator('.choice-scan-surface')).to_be_hidden()
    # Empty queue gives its full height to chat, even during a current request.
    payload['queue'] = []
    expect(page.locator('.queue-panel')).to_be_hidden()
    expect(page.locator('#feed-count')).to_have_text('8 / 50')
    expanded_chat = page.locator('#activity-panel').bounding_box()
    assert expanded_chat['y'] < chat_box['y'] and expanded_chat['height'] == chat_box['height'] + 370
    assert page.locator('.event-panel').bounding_box() == event_box
    bounded()
    (shots / 'frame-chat-expanded.png').write_bytes(transparent())
    # A pending choice must still open when there are no queued songs.
    payload['pending'] = [dict(choice, candidates=[dict(title=f'Choice {i+1}', available=True) for i in range(9)])]
    expect(page.locator('.candidate')).to_have_count(5)
    expect(page.locator('.queue-panel')).to_be_visible()
    expect(page.locator('#feed-count')).to_have_text('3 / 50')
    assert page.locator('.event-panel').bounding_box() == event_box
    bounded()
    payload['pending'] = []
    expect(page.locator('#feed-count')).to_have_text('8 / 50')
    fail = True
    expect(page.locator('.connection-banner')).to_be_visible(timeout=6000)
    expect(page.locator('.current-title')).to_have_count(0)
    expect(page.locator('#activity-list .activity-row')).to_have_count(8)
    assert page.locator('.event-panel').bounding_box() == event_box
    bounded()
    transparent()
    fail = False
    payload = copy.deepcopy(live)
    expect(page.locator('.current-title')).to_have_text('X-DEN')
    expect(page.locator('.connection-banner')).to_be_hidden()
    expect(page.locator('#activity-list .activity-row')).to_have_count(3)
    assert page.locator('#activity-panel').bounding_box() == chat_box
    for size in [(1280, 720), (960, 540)]:
        page.set_viewport_size(dict(width=size[0], height=size[1]))
        expect(page.locator('.stream-frame')).to_have_css('transform', f'matrix({size[0]/1920:g}, 0, 0, {size[0]/1920:g}, 0, 0)' if size[0] == 960 else 'matrix(0.666667, 0, 0, 0.666667, 0, 0)')
        bounded()
    page.set_viewport_size(dict(width=1920, height=1080))
    payload.update(current=None, queue=[], feed=[])
    expect(page.locator('#chat-empty')).to_be_visible()
    expect(page.locator('#event-empty')).to_be_visible()
    expect(page.locator('.queue-panel')).to_be_hidden()
    assert page.locator('.event-panel').bounding_box() == event_box
    expect(page.locator('.current-idle')).to_be_visible()
    page.screenshot(path=str(shots / 'frame-idle.png'), omit_background=True)
    transparent()

    # Actual game selection must render independently of the request queue.
    live_chart = dict(chart=dict(mode='SP', id='SPA'), difficulty='ANOTHER', level='12', style='red',
                      bpm=dict(min=100, max=200), note_count=2000,
                      density=dict(bin_ms=1000, duration_ms=6500, notes=[0, 2, 42, 1], scratch=[0, 0, 2, 1]),
                      radar=dict(notes=180.25, peak=145.50, scratch=65.75, soflan=120, charge=0, chord=150.01))
    game_song = dict(id=11040, title='手动选曲 · Manual selection', artist='テスト Artist', genre='TEST GENRE',
                     game_version=11, charts=[live_chart])
    game = dict(phase='selecting', song=game_song, players=[dict(side=2, chart=live_chart['chart'])],
                lane_order=[dict(side=2, random='off', mirror=False, status='ready', keys=[1,2,3,4,5,6,7])])
    payload['now_playing'] = copy.deepcopy(game)
    expect(page.locator('.current-title')).to_have_text(game_song['title'])
    expect(page.locator('.station-header #request-hint')).to_have_text('点歌 <曲名> [难度]')
    expect(page.locator('.queue-panel #request-hint')).to_have_count(0)
    expect(page.locator('.game-chart .chart, .chart-difficulty, .song-notes')).to_have_count(0)
    expect(page.locator('.song-bpm')).to_have_text('BPM 100–200')
    expect(page.locator('.lane-keys')).to_have_text('1234567')
    expect(page.locator('.lane-caption')).to_have_text('RANDOM OFF')
    expect(page.locator('.lane-keys')).to_have_css('color', 'rgb(6, 16, 24)')
    expect(page.locator('.lane-keys')).to_have_css('text-shadow', 'none')
    bpm_box = page.locator('.song-bpm').bounding_box()
    keys_box = page.locator('.lane-keys').bounding_box()
    assert bpm_box['y'] + bpm_box['height'] < keys_box['y']
    expect(page.locator('.song-credits')).to_have_text('TEST GENRE / テスト Artist')
    expect(page.locator('.radar-axis')).to_have_count(6)
    expect(page.locator('.song-radar svg')).to_have_count(1)
    assert page.locator('.song-radar svg').bounding_box()['height'] == 116
    expect(page.locator('.song-density svg')).to_have_count(1)
    expect(page.locator('.density-heading')).to_contain_text('峰值 42 个/秒')
    expect(page.locator('.density-axis')).to_have_text('0:0000:07')
    expect(page.locator('.song-density svg')).to_have_attribute('data-bin-ms', '5000')
    expect(page.locator('.density-column')).to_have_count(2)
    # 45 native notes in the first five seconds = 9/s. The silent tail is a
    # separate unlit window; the original 1s peak of 42 is still reported above.
    expect(page.locator('.density-column').first).to_have_attribute('data-average', '9.000')
    expect(page.locator('.density-column').last).to_have_attribute('data-average', '0.000')
    assert page.locator('.density-column .density-hot').first.get_attribute('d')
    assert page.locator('.density-column .density-notes').last.get_attribute('d') == ''
    def metadata_bounded():
        bounded()
        outer = page.locator('.frame-current').bounding_box()
        for selector in ['.game-identity', '.game-charts', '.song-densities', '.song-radar']:
            box = page.locator(selector).bounding_box()
            assert box['x'] >= outer['x'] and box['x'] + box['width'] <= outer['x'] + outer['width'], selector
            assert box['y'] >= outer['y'] and box['y'] + box['height'] <= outer['y'] + outer['height'], selector
        boxes = [page.locator(s).bounding_box() for s in ['.game-identity', '.game-charts', '.song-densities', '.song-radar']]
        assert all(a['x'] + a['width'] <= b['x'] for a, b in zip(boxes, boxes[1:]))
        # The outer panel rectangle includes the raised metal shoulder. Check
        # radar content against the actual stepped glass, with a small inset.
        assert page.evaluate('''() => {
            const glass = document.querySelector('.song-face .screen-glass');
            const inverse = glass.getScreenCTM().inverse();
            const pad = 3 * Math.min(innerWidth / 1920, innerHeight / 1080);
            return [...document.querySelectorAll('.song-radar svg, .radar-caption, .radar-axis')].every(el => {
                const b = el.getBoundingClientRect();
                return [b.left - pad, b.right + pad].every(x =>
                    [b.top - pad, b.bottom + pad].every(y =>
                        glass.isPointInFill(new DOMPoint(x, y).matrixTransform(inverse))));
            });
        }'''), 'Radar content crosses the glass outline'
    metadata_bounded()
    (shots / 'frame-game-song.png').write_bytes(transparent())
    payload['now_playing']['phase'] = 'playing'
    order = payload['now_playing']['lane_order'][0]
    order.update(random='random', keys=[3,4,5,2,1,6,7])
    expect(page.locator('.lane-keys')).to_have_text('3452167')
    expect(page.locator('.lane-caption')).to_have_text('RANDOM')
    page.locator('.frame-current').screenshot(path=str(shots / 'frame-random.png'), omit_background=True)
    order.update(random='r_random', mirror=True)
    expect(page.locator('.lane-caption')).to_have_text('R-RANDOM + MIRROR')
    assert page.locator('.lane-caption').evaluate('(el) => el.scrollWidth <= el.clientWidth')
    for state, option in [('dynamic', 's_random'), ('pending', 'random'), ('unavailable', 'random')]:
        order.update(status=state, random=option, keys=None)
        expect(page.locator('.lane-order')).to_have_attribute('data-ready', 'false')
        expect(page.locator('.lane-keys')).to_have_text('———————')
        expect(page.locator('.lane-caption')).to_have_text('S-RANDOM + MIRROR' if state == 'dynamic' else 'RANDOM + MIRROR')
    for invalid in [[1,1,2,3,4,5,6], [1,2,3,4,5,6,8], [1,2,3,4,5,6], ['<img src=x onerror=alert(1)>']*7]:
        order.update(status='ready', keys=invalid)
        expect(page.locator('.lane-order')).to_have_attribute('data-ready', 'false')
    assert page.locator('img').count() == 0
    del payload['now_playing']['lane_order']
    expect(page.locator('.lane-caption')).to_have_text('等待键位数据')
    expect(page.locator('.lane-keys')).to_have_text('———————')
    payload['now_playing'] = copy.deepcopy(game)
    # Longer songs use 10s windows. Divide a partial ending by its own duration,
    # not by ten seconds, and retain the native peak independently of the bars.
    payload['now_playing']['song']['charts'][0]['density'] = dict(
        bin_ms=1000, duration_ms=181500, notes=[2] * 181 + [0], scratch=[0] * 182)
    expect(page.locator('.song-density svg')).to_have_attribute('data-bin-ms', '10000')
    expect(page.locator('.density-column')).to_have_count(19)
    expect(page.locator('.density-column').first).to_have_attribute('data-average', '2.000')
    expect(page.locator('.density-column').last).to_have_attribute('data-average', '1.333')
    payload['now_playing'] = copy.deepcopy(game)
    expect(page.locator('.song-density svg')).to_have_attribute('data-bin-ms', '5000')
    payload['now_playing']['song']['title'] = injection + '长曲名' * 40
    payload['now_playing']['song']['artist'] = injection + '长作者' * 40
    expect(page.locator('.current-title')).to_have_text(payload['now_playing']['song']['title'])
    assert page.locator('img').count() == 0
    metadata_bounded()
    payload['now_playing']['phase'] = 'playing'
    expect(page.locator('.station-header #request-hint')).to_have_text('点歌 <曲名> [难度]')
    # Battle can have distinct difficulties on the two participating sides.
    other_chart = copy.deepcopy(live_chart)
    other_chart.update(chart=dict(mode='SP', id='SPH'), difficulty='HYPER', level='10', style='amber')
    payload['now_playing']['song']['charts'].append(other_chart)
    payload['now_playing']['players'].append(dict(side=1, chart=other_chart['chart']))
    expect(page.locator('.game-chart')).to_have_count(2)
    expect(page.locator('.song-density svg')).to_have_count(2)
    metadata_bounded()
    for size in [(1280, 720), (960, 540)]:
        page.set_viewport_size(dict(width=size[0], height=size[1]))
        metadata_bounded()
    page.set_viewport_size(dict(width=1920, height=1080))
    page.screenshot(path=str(shots / 'frame-density-two-players.png'), omit_background=True)
    payload['now_playing'] = copy.deepcopy(game)
    payload['now_playing']['song']['charts'][0]['chart'] = dict(mode='DP', id='DPA')
    payload['now_playing']['players'][0]['chart'] = dict(mode='DP', id='DPA')
    payload['now_playing']['lane_order'] = [dict(side=i, random='random', mirror=False, status='ready', keys=[3,4,5,2,1,6,7]) for i in [1,2]]
    expect(page.locator('.lane-keys')).to_have_count(2)
    expect(page.locator('.lane-caption')).to_have_text(['1P · RANDOM', '2P · RANDOM'])
    metadata_bounded()
    page.locator('.frame-current').screenshot(path=str(shots / 'frame-random-dp.png'), omit_background=True)
    payload['now_playing']['players'].append(dict(side=1, chart=dict(mode='DP', id='DPA')))
    expect(page.locator('.game-chart')).to_have_count(1)
    expect(page.locator('.lane-keys')).to_have_count(2)
    expect(page.locator('.song-density svg')).to_have_count(1)
    for order in payload['now_playing']['lane_order']:
        order.update(random='r_random', mirror=True, status='pending', keys=None)
    expect(page.locator('.lane-caption')).to_have_text(['1P · R-RANDOM + MIRROR', '2P · R-RANDOM + MIRROR'])
    for caption in page.locator('.lane-caption').all():
        assert caption.evaluate('(el) => el.scrollWidth <= el.clientWidth')
    # Invalid and old DLL data must clear a previously rendered plot.
    for bad in [None, dict(bin_ms=1000, duration_ms=0, notes=[1], scratch=[0]),
                dict(bin_ms=1000, duration_ms=1000, notes=[1], scratch=[2])]:
        payload['now_playing'] = copy.deepcopy(game)
        payload['now_playing']['song']['charts'][0]['density'] = bad
        expect(page.locator('.song-density svg')).to_have_count(0)
        expect(page.locator('.density-empty')).to_have_text('暂无密度数据')
    del payload['now_playing']['song']['charts'][0]['density']
    expect(page.locator('.song-density svg')).to_have_count(0)
    payload['now_playing'] = copy.deepcopy(game)
    expect(page.locator('.song-density svg')).to_have_count(1)
    payload['now_playing'] = copy.deepcopy(game)
    payload['now_playing']['song']['charts'][0].update(bpm=None, note_count=None, radar=None)
    expect(page.locator('.game-chart')).to_have_count(1)
    expect(page.locator('.song-bpm')).to_have_text('BPM —')
    expect(page.locator('.song-radar svg')).to_have_count(0)
    expect(page.locator('.radar-axis b').first).to_have_text('—')
    metadata_bounded()
    fail = True
    expect(page.locator('.current-title')).to_have_count(0, timeout=6000)
    fail = False
    payload['now_playing'] = copy.deepcopy(game)
    expect(page.locator('.current-title')).to_have_text(game_song['title'])
    payload['now_playing'] = dict(phase='idle', song=None, players=[])
    expect(page.locator('.current-idle')).to_be_visible()
    expect(page.locator('.song-radar')).to_have_count(0)
    expect(page.locator('.song-densities')).to_have_count(0)
    assert not errors, errors
    browser.close()
print('PASS: exact 1504x846 16:9 capture at (40,24), thin top rail, raised bottom display, embedded turntable, transparency outside its corner at three scales, contained scan, larger expanding chat, pinned events, live density/metadata, cutout lane order and SP/DP random captions, candidate paging and reload, viewer rotation and focus, escaping, offline recovery, scaling, and empty states.')
print(f'Screenshots: {shots}')
