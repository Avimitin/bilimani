"""Browser checks against the real preview server (requires Playwright).

Run the overlay_preview example first. Screenshots stay in analysis/.
Use --browser to select a local Chrome executable.
"""
import argparse
import copy
import json
import pathlib
import sys

root = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(root / "reference/overlay-preview-tools"))
from playwright.sync_api import sync_playwright, expect

parser = argparse.ArgumentParser()
parser.add_argument("--url", default="http://127.0.0.1:32133")
parser.add_argument("--browser")
args = parser.parse_args()
shots = root / "analysis/overlay-history"
shots.mkdir(parents=True, exist_ok=True)
with sync_playwright() as p:
    browser = p.chromium.launch(headless=True, **({"executable_path": args.browser} if args.browser else {}))
    page = browser.new_page(viewport={"width": 480, "height": 800}, device_scale_factor=1)
    errors = []
    page.on("pageerror", lambda error: errors.append(str(error)))
    page.goto(args.url)
    expect(page).to_have_url(args.url + "/queue")
    expect(page.locator(".connection-banner")).to_be_visible()
    expect(page.locator(".current-title")).to_have_count(0)
    expect(page.locator("button, input")).to_have_count(0)
    expect(page.locator(".overlay")).to_have_attribute("data-queue-state", "collapsed")
    assert page.locator(".queue-panel").bounding_box()["height"] <= 40
    expect(page.locator("#activity-panel")).to_have_attribute("data-feed-state", "collapsed")

    def activity(i):
        return {"id": i, "at": 1, "kind": "event" if i % 3 == 0 else "chat", "name": f"观众 {i}",
                "text": "已加入队列：冥 [SPA] — 观众甲" if i % 3 == 0 else "今晚手感不错！" if i % 2 else "很长的一条普通弹幕，" * 25}

    live = {
        "connected": True, "ready": True, "status": "弹幕已连接", "capacity": 20,
        "current": {"token": 1, "title": "冥", "requester": "观众甲", "mode": "SP", "chart": "SPA", "chart_style": "red", "remaining": 590, "duration": 600},
        "queue": [{"token": i + 2, "title": "很长的曲名 / " * 8, "requester": "观众乙", "mode": "SP", "chart": "SPH", "chart_style": "amber"} for i in range(9)],
        "pending": [], "notices": [], "feed_limit": 10, "feed": [activity(i) for i in range(1, 11)]
    }
    payload = copy.deepcopy(live)
    payload["feed"] = []
    fail = False

    def state_route(route):
        if fail:
            route.abort()
        else:
            route.fulfill(status=200, content_type="application/json", body=json.dumps(payload, ensure_ascii=False))

    def fixed_layout():
        page.wait_for_function("![...document.querySelector('.overlay').getAnimations(), ...document.querySelector('#activity-panel').getAnimations()].some(a => a.playState === 'running')")
        assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
        assert page.evaluate("document.documentElement.scrollHeight <= innerHeight")
        overlay = page.locator(".overlay").bounding_box()
        assert abs(overlay["height"] - page.viewport_size["height"]) <= 1
        for selector in [".queue-panel", "#activity-panel", ".activity-row", ".candidate", ".queue-row"]:
            for box in page.locator(selector).all():
                if not box.is_visible():
                    continue
                rect = box.bounding_box()
                assert rect and rect["y"] >= 0 and rect["y"] + rect["height"] <= overlay["height"], selector
        queue = page.locator(".queue-panel").bounding_box()
        feed = page.locator("#activity-panel").bounding_box()
        assert feed["y"] >= queue["y"] + queue["height"]
        return (queue["height"], feed["height"], overlay["height"])

    page.route("**/api/state", state_route)
    page.goto(args.url + "/queue")
    expect(page.locator(".current-title")).to_have_text("冥")
    # No empty slots: only the header remains, then the background grows with
    # actual arrivals up to the available height. A full feed stays bounded.
    expect(page.locator(".connection-banner")).to_be_hidden()
    expect(page.locator("#activity-panel")).to_have_attribute("data-feed-state", "collapsed")
    heights = [fixed_layout()[1]]
    assert heights[0] <= 40
    page.screenshot(path=str(shots / "feed-empty.png"), omit_background=True)
    for count in range(1, 11):
        payload["feed"] = [activity(i) for i in range(1, count + 1)]
        expect(page.locator(".activity-row")).to_have_count(count)
        heights.append(fixed_layout()[1])
        if count in [1, 3]:
            page.screenshot(path=str(shots / f"feed-{count}.png"), omit_background=True)
    assert all(before < after for before, after in zip(heights[:8], heights[1:9])), heights
    assert heights[-1] >= heights[-2] and heights[-1] < page.viewport_size["height"]
    expect(page.locator("#current-content .chart")).to_have_css("color", "rgb(238, 146, 152)")
    expect(page.locator(".queue-row .chart").first).to_have_css("color", "rgb(226, 199, 116)")
    expect(page.locator(".activity-row")).to_have_count(10)
    expect(page.locator("#feed-count")).to_have_text("10 / 10")
    expect(page.locator(".activity-row").first).to_have_attribute("data-id", "1")
    assert 1 <= page.locator(".queue-row").count() <= 6
    expect(page.locator("#queue-more")).to_have_text(f"另有 {9 - page.locator('.queue-row').count()} 首等待中")
    assert page.evaluate("getComputedStyle(document.body).backgroundColor") == "rgba(0, 0, 0, 0)"
    geometry = fixed_layout()
    chat_rows = page.locator('.activity-row[data-kind="chat"]')
    event_rows = page.locator('.activity-row[data-kind="event"]')
    assert min(row.bounding_box()["height"] for row in chat_rows.all()) >= 34
    assert max(row.bounding_box()["height"] for row in event_rows.all()) <= 24
    expect(chat_rows.first.locator(".activity-text")).to_have_css("font-size", "16px")
    expect(event_rows.first.locator(".activity-text")).to_have_css("font-size", "12px")
    page.screenshot(path=str(shots / "history.png"), omit_background=True)

    page.evaluate("window.previousTitle = document.querySelector('.current-title'); window.previousFeed = document.querySelector('.activity-row')")
    payload["current"]["remaining"] = 589
    expect(page.locator(".remaining b")).to_have_text("09:49")
    assert page.evaluate("window.previousTitle === document.querySelector('.current-title')")
    assert page.evaluate("window.previousFeed === document.querySelector('.activity-row')")
    # No timer expiry, even after the old popup's six-second duration.
    page.wait_for_timeout(7000)
    expect(page.locator(".activity-row")).to_have_count(10)
    assert page.evaluate("window.previousFeed === document.querySelector('.activity-row')")
    payload["feed"].append(activity(11))
    expect(page.locator(".activity-row").last).to_have_attribute("data-id", "11")
    expect(page.locator(".activity-row")).to_have_count(10)
    expect(page.locator(".activity-row").first).to_have_attribute("data-id", "2")
    assert fixed_layout() == geometry
    page.reload()
    expect(page.locator(".activity-row").first).to_have_attribute("data-id", "2")

    # Same-time, identical messages remain distinct arrivals.
    payload["feed"] = [{"id": i, "at": 2, "kind": "chat", "name": "同一观众", "text": "晚上好"} for i in [12, 13]]
    expect(page.locator(".activity-row")).to_have_count(2)
    expect(page.locator(".activity-row").last).to_have_attribute("data-id", "13")
    shorter_geometry = fixed_layout()
    assert shorter_geometry[0] == geometry[0] and shorter_geometry[1] < geometry[1]
    injection = '<img src=x onerror="window.injected=true">'
    payload["feed"][0]["text"] = injection
    payload["feed"][1].update(kind="event", text=injection)
    payload["current"].update(title=injection, chart="EXPERT+", chart_style="purple")
    expect(page.locator(".current-title")).to_have_text(injection)
    expect(page.locator("#current-content .chart")).to_have_css("color", "rgb(199, 161, 232)")
    expect(page.locator(".activity-row").first).to_contain_text(injection)
    expect(page.locator(".activity-row").last).to_have_text("✓" + injection)
    assert page.locator("img").count() == 0 and not page.evaluate("Boolean(window.injected)")

    fail = True
    expect(page.locator(".connection-banner")).to_contain_text("等待游戏连接", timeout=8000)
    expect(page.locator(".queue-row")).to_have_count(0)
    expect(page.locator(".current-title")).to_have_count(0)
    expect(page.locator(".activity-row")).to_have_count(2)
    assert fixed_layout()[0] <= 40
    fail = False
    payload = copy.deepcopy(live)
    expect(page.locator(".current-title")).to_have_text("冥")
    expect(page.locator(".activity-row")).to_have_count(10)

    # Empty waiting queues retain just the current song, then collapse to the
    # banner when it finishes. Both transitions animate and keep all chat rows.
    page.evaluate("""() => {
      window.queueTransitions = [];
      document.querySelector('.overlay').addEventListener('transitionrun', event => {
        if (event.propertyName === 'grid-template-rows') window.queueTransitions.push(event.propertyName);
      });
    }""")
    payload["queue"] = []
    expect(page.locator(".overlay")).to_have_attribute("data-queue-state", "current")
    assert 125 <= fixed_layout()[0] <= 140
    expect(page.locator(".current-title")).to_be_visible()
    expect(page.locator("#waiting-area")).to_be_hidden()
    payload["current"] = None
    expect(page.locator(".overlay")).to_have_attribute("data-queue-state", "collapsed")
    assert fixed_layout()[0] <= 40
    assert page.evaluate("window.queueTransitions.length") >= 2
    expect(page.locator("#queue-area")).to_have_attribute("aria-hidden", "true")
    expect(page.locator(".activity-row")).to_have_count(10)
    for row in chat_rows.all():
        assert float(row.evaluate("el => getComputedStyle(el).paddingTop").removesuffix("px")) >= 5
        assert float(row.evaluate("el => getComputedStyle(el).paddingBottom").removesuffix("px")) >= 5
        assert abs(row.bounding_box()["height"] - 46) <= 1
    for row in event_rows.all():
        assert float(row.evaluate("el => getComputedStyle(el).paddingTop").removesuffix("px")) == 0
        assert abs(row.bounding_box()["height"] - 24) <= 1
    page.screenshot(path=str(shots / "queue-collapsed.png"), omit_background=True)

    pending = {"requester": "观众甲", "mode": "SP", "chart": "SPA", "chart_style": "red", "remaining": 45, "duration": 60,
               "candidates": [{"title": title, "available": i != 1} for i, title in enumerate([
                   "AA", "AA -rebuild-", "A", "A MINSTREL ～ver. short-scape～", "A Tale Hidden In The Abyss"
               ])]}
    payload["pending"] = [copy.deepcopy(pending)]
    # Pending choices must reopen even with no current or waiting songs.
    expect(page.locator(".overlay")).to_have_attribute("data-queue-state", "expanded")
    expect(page.locator(".candidate")).to_have_count(5)
    expect(page.locator(".choice-prompt")).to_have_text("请在弹幕发送编号选歌")
    expect(page.locator(".choice-help")).to_contain_text("1–5")
    expect(page.locator("#queue-area")).to_be_hidden()
    expect(page.locator(".current-panel")).to_be_hidden()
    expect(page.locator("#pending-page")).to_be_hidden()
    expect(page.locator(".unavailable-note")).to_have_text("所请求谱面不存在")
    assert fixed_layout() == geometry
    page.screenshot(path=str(shots / "choices-five.png"), omit_background=True)
    payload["current"] = copy.deepcopy(live["current"])
    payload["queue"] = copy.deepcopy(live["queue"])
    page.evaluate("window.previousChoice = document.querySelector('.choice-panel')")
    payload["pending"][0]["remaining"] = 9
    expect(page.locator(".countdown.urgent b")).to_have_text("09")
    assert page.evaluate("window.previousChoice === document.querySelector('.choice-panel')")

    def all_choices_fit(count):
        expect(page.locator(".candidate")).to_have_count(count)
        assert page.locator(".candidate-number").all_text_contents() == [str(i) for i in range(1, count + 1)]
        assert fixed_layout() == geometry
        panel = page.locator(".queue-panel").bounding_box()
        for selector in [".candidate", ".candidate-number", ".choice-help"]:
            for item in page.locator(selector).all():
                box = item.bounding_box()
                assert box and box["x"] >= panel["x"] and box["x"] + box["width"] <= panel["x"] + panel["width"]
                assert box["y"] >= panel["y"] and box["y"] + box["height"] <= panel["y"] + panel["height"]

    # Every candidate stays on screen, including the configured maximum of 20.
    for width in [480, 320]:
        page.set_viewport_size({"width": width, "height": 800})
        for count in [1, 3, 5, 8, 9, 16, 20]:
            payload["pending"][0]["candidates"] = [{"title": f"候选 {i + 1} / " + "很长的曲名 " * 8, "available": True} for i in range(count)]
            all_choices_fit(count)
        page.screenshot(path=str(shots / f"choices-twenty-{width}.png"), omit_background=True)
    page.wait_for_timeout(6500)
    all_choices_fit(20)
    expect(page.locator("#pending-page")).to_be_hidden()

    # Multiple viewers rotate whole lists. Reordering a snapshot keeps the
    # currently visible viewer, while completion restores the live queue.
    payload["pending"].append(dict(copy.deepcopy(pending), requester="观众乙", remaining=30))
    expect(page.locator("#pending-page")).to_contain_text("1 / 2")
    payload["pending"].reverse()
    expect(page.locator("#pending-page")).to_contain_text("2 / 2")
    expect(page.locator(".choice-heading h2")).to_have_text("观众甲，请选择")
    all_choices_fit(20)
    expect(page.locator(".choice-heading h2")).to_have_text("观众乙，请选择", timeout=9000)
    all_choices_fit(5)
    expect(page.locator(".activity-row")).to_have_count(10)
    page.screenshot(path=str(shots / "choices-multiple-narrow.png"), omit_background=True)

    payload["pending"] = [dict(copy.deepcopy(pending), requester=injection)]
    payload["pending"][0]["candidates"][0]["title"] = injection
    expect(page.locator(".candidate-title").first).to_have_text(injection)
    assert page.locator("img").count() == 0 and not page.evaluate("Boolean(window.injected)")

    payload["pending"] = []
    expect(page.locator("#pending-area")).to_be_hidden()
    expect(page.locator("#queue-area")).to_be_visible()
    expect(page.locator(".current-title")).to_have_text("冥")
    expect(page.locator(".queue-row").first).to_be_visible()
    page.screenshot(path=str(shots / "choices-restored.png"), omit_background=True)
    payload["feed_limit"] = 3
    expect(page.locator(".activity-row")).to_have_count(3)
    expect(page.locator("#feed-count")).to_have_text("3 / 3")
    expect(page.locator(".activity-row").first).to_have_attribute("data-id", "8")
    # A single event uses a slim row; chat keeps its taller card and larger type.
    payload["feed"] = [activity(3)]
    expect(page.locator(".activity-row")).to_have_count(1)
    event_height = fixed_layout()[1]
    payload["feed"] = [activity(1)]
    expect(page.locator(".activity-row")).to_have_attribute("data-kind", "chat")
    assert fixed_layout()[1] > event_height
    # Profile switch/server restart sends an empty history, clearing the old room.
    payload.update(feed=[], queue=[], current=None)
    expect(page.locator(".activity-row")).to_have_count(0)
    expect(page.locator("#activity-panel")).to_have_attribute("data-feed-state", "collapsed")
    expect(page.locator(".feed-body")).to_have_attribute("aria-hidden", "true")
    expect(page.locator("#activity-panel")).to_be_visible()
    expect(page.locator(".overlay")).to_have_attribute("data-queue-state", "collapsed")
    empty_geometry = fixed_layout()
    assert empty_geometry[0] <= 40 and empty_geometry[1] <= 40
    page.emulate_media(reduced_motion="reduce")
    payload["queue"] = copy.deepcopy(live["queue"])
    expect(page.locator(".overlay")).to_have_attribute("data-queue-state", "expanded")
    fixed_layout()
    assert page.evaluate("document.querySelector('.overlay').getAnimations().length") == 0
    assert not errors, errors
    browser.close()
print("PASS: animated queue and growing history, large chat text and slim events, reduced motion, bounded layout, persistent history, all 1–20 candidates, viewer rotation and offline recovery.")
print(f"Screenshots: {shots}")
