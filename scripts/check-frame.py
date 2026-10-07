"""Check the stream frame with local fixtures. Requires Playwright and Pillow.

Run against overlay_preview, or supply --url for a static asset server.
Screenshots stay in analysis/stream-frame/.
"""
import argparse
import copy
import io
import pathlib

from PIL import Image
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
            queue=[dict(song, token=i+2, title=title, requester=name, chart=chart, chart_style=tone) for i, (title, name, chart, tone) in enumerate([
                ('sakura storm', '春日来信', 'SPH', 'amber'), ('Somnidiscotheque', '夜航星', 'SPA', 'red'),
                ('INFINITAS', '像素海', 'SPL', 'purple'), ('冥', 'GreenTea', 'SPA', 'red'), ('雪月花', '月下', 'SPH', 'amber')])],
            pending=[], feed_limit=10, feed=[
                dict(id=1, kind='chat', name='春日来信', text='晚上好！今天也来听歌了。'),
                dict(id=2, kind='event', text='已加入队列：sakura storm [SPH]'),
                dict(id=3, kind='chat', name='夜航星', text='这个配置好帅，绿色好有氛围'),
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
    chart_box = page.locator('.current-song .chart').bounding_box()
    assert 0 <= chart_box['x'] - title_box['x'] - title_box['width'] <= 20
    expect(page.locator('#activity-list .activity-row')).to_have_count(4)
    expect(page.locator('#event-list .activity-row')).to_have_count(3)
    expect(page.locator('#queue-more')).to_have_text('另有 1 首等待中')
    assert page.locator('.game-window').bounding_box() == dict(x=24, y=40, width=1536, height=864)

    def bounded():
        assert page.evaluate('document.documentElement.scrollWidth <= innerWidth && document.documentElement.scrollHeight <= innerHeight')
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

    def transparent():
        png = page.screenshot(omit_background=True)
        shot = Image.open(io.BytesIO(png)).convert('RGBA')
        # Every pixel of the game aperture must be fully transparent.
        assert shot.crop((24, 40, 1560, 904)).getchannel('A').getextrema() == (0, 0)
        assert shot.getpixel((1600, 500))[3] == 255
        return png

    bounded()
    (shots / 'frame-live.png').write_bytes(transparent())
    # Viewer strings are text, even if they contain HTML.
    injection = '<img src=x onerror="window.injected=true">'
    payload['current']['title'] = injection + '长曲名' * 40
    payload['feed'][0]['text'] = injection
    expect(page.locator('.current-title')).to_have_text(payload['current']['title'])
    expect(page.locator('#activity-list')).to_contain_text(injection)
    assert page.locator('img').count() == 0 and not page.evaluate('Boolean(window.injected)')
    bounded()
    payload['current']['chart'] = None
    expect(page.locator('.current-song .chart')).to_have_text('SP · 当前难度')
    bounded()
    payload['current'] = copy.deepcopy(song)
    payload['feed'] = copy.deepcopy(live['feed'])
    choice = dict(requester='观众甲', mode='SP', chart='SPA', chart_style='red', remaining=45, duration=60)
    for count in [1, 5, 8, 9, 16, 20]:
        payload['pending'] = [dict(choice, candidates=[dict(title=f'{i+1} / 很长的候选曲名 ' * 4, available=i != 1) for i in range(count)])]
        expect(page.locator('.candidate')).to_have_count(count)
        expect(page.locator('.current-title')).to_be_visible()
        expect(page.locator('#queue-area')).to_be_hidden()
        bounded()
        transparent()
    page.screenshot(path=str(shots / 'frame-choices.png'), omit_background=True)
    payload['pending'][0]['remaining'] = 9
    expect(page.locator('.countdown.urgent b')).to_have_text('09')
    payload['pending'].append(dict(choice, requester='观众乙', candidates=[dict(title='AA', available=True)]))
    expect(page.locator('#pending-page')).to_contain_text('1 / 2')
    expect(page.locator('.choice-heading h2')).to_have_text('观众乙，请选择', timeout=9000)
    payload['pending'] = []
    payload['feed_limit'] = 100
    payload['feed'] = [dict(id=i, kind='chat' if i % 2 else 'event', name='观众', text='很长的消息 ' * 80) for i in range(100)]
    expect(page.locator('#feed-count')).to_have_text('4 / 50')
    expect(page.locator('#event-count')).to_have_text('3 / 50')
    expect(page.locator('#activity-list .activity-row').last).to_have_attribute('data-id', '99')
    expect(page.locator('#event-list .activity-row').last).to_have_attribute('data-id', '98')
    bounded()
    fail = True
    expect(page.locator('.connection-banner')).to_be_visible(timeout=6000)
    expect(page.locator('.current-title')).to_have_count(0)
    expect(page.locator('#activity-list .activity-row')).to_have_count(4)
    bounded()
    transparent()
    fail = False
    payload = copy.deepcopy(live)
    expect(page.locator('.current-title')).to_have_text('X-DEN')
    expect(page.locator('.connection-banner')).to_be_hidden()
    for size in [(1280, 720), (960, 540)]:
        page.set_viewport_size(dict(width=size[0], height=size[1]))
        expect(page.locator('.stream-frame')).to_have_css('transform', f'matrix({size[0]/1920:g}, 0, 0, {size[0]/1920:g}, 0, 0)' if size[0] == 960 else 'matrix(0.666667, 0, 0, 0.666667, 0, 0)')
        bounded()
    page.set_viewport_size(dict(width=1920, height=1080))
    payload.update(current=None, queue=[], feed=[])
    expect(page.locator('#chat-empty')).to_be_visible()
    expect(page.locator('#event-empty')).to_be_visible()
    expect(page.locator('#queue-empty')).to_be_visible()
    expect(page.locator('.current-idle')).to_be_visible()
    page.screenshot(path=str(shots / 'frame-idle.png'), omit_background=True)
    transparent()
    assert not errors, errors
    browser.close()
print('PASS: frame geometry, transparent aperture, live data, bounded history, 1–20 choices, rotation, escaping, offline recovery, scaling, and empty states.')
print(f'Screenshots: {shots}')
