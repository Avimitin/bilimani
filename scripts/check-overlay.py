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
    fail = False

    def state_route(route):
        if fail:
            route.abort()
        else:
            route.fulfill(status=200, content_type="application/json", body=json.dumps(payload, ensure_ascii=False))

    def fixed_layout():
        assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
        assert page.evaluate("document.documentElement.scrollHeight <= innerHeight")
        overlay = page.locator(".overlay").bounding_box()
        assert abs(overlay["height"] - page.viewport_size["height"]) <= 1
        for selector in [".queue-panel", "#activity-panel", ".activity-row", ".candidate", ".queue-row"]:
            for box in page.locator(selector).all():
                rect = box.bounding_box()
                assert rect and rect["y"] >= 0 and rect["y"] + rect["height"] <= overlay["height"], selector
        queue = page.locator(".queue-panel").bounding_box()
        feed = page.locator("#activity-panel").bounding_box()
        assert feed["y"] >= queue["y"] + queue["height"]
        return (queue["height"], feed["height"], overlay["height"])

    page.route("**/api/state", state_route)
    page.goto(args.url + "/queue")
    expect(page.locator(".current-title")).to_have_text("冥")
    expect(page.locator("#current-content .chart")).to_have_css("color", "rgb(238, 146, 152)")
    expect(page.locator(".queue-row .chart").first).to_have_css("color", "rgb(226, 199, 116)")
    expect(page.locator(".activity-row")).to_have_count(10)
    expect(page.locator("#feed-count")).to_have_text("10 / 10")
    expect(page.locator(".activity-row").first).to_have_attribute("data-id", "1")
    assert 1 <= page.locator(".queue-row").count() <= 6
    expect(page.locator("#queue-more")).to_have_text(f"另有 {9 - page.locator('.queue-row').count()} 首等待中")
    assert page.evaluate("getComputedStyle(document.body).backgroundColor") == "rgba(0, 0, 0, 0)"
    geometry = fixed_layout()
    assert min(row.bounding_box()["height"] for row in page.locator(".activity-row").all()) >= 34
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
    assert fixed_layout() == geometry
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
    assert fixed_layout() == geometry
    fail = False
    payload = copy.deepcopy(live)
    expect(page.locator(".current-title")).to_have_text("冥")
    expect(page.locator(".activity-row")).to_have_count(10)

    payload["pending"] = [{"requester": "观众甲", "mode": "SP", "chart": "SPA", "chart_style": "red", "remaining": 9, "duration": 60,
                           "candidates": [{"title": f"候选曲目 {i + 1}", "available": i != 1} for i in range(20)]}]
    page.goto(args.url + "/queue?demo=1")
    expect(page.locator(".current-title")).to_have_text("冥")  # Always live data.
    expect(page.locator(".candidate")).to_have_count(3)
    expect(page.locator("#pending-page")).to_contain_text("1 / 7")
    expect(page.locator(".unavailable-note")).to_have_text("所请求谱面不存在")
    expect(page.locator(".countdown.urgent b")).to_have_text("09")
    expect(page.locator(".queue-row").first).to_be_visible()
    assert fixed_layout() == geometry
    expect(page.locator("#pending-page")).to_contain_text("2 / 7", timeout=9000)
    expect(page.locator(".candidate-number").first).to_have_text("4")
    expect(page.locator(".activity-row")).to_have_count(10)
    page.screenshot(path=str(shots / "candidates.png"), omit_background=True)
    page.set_viewport_size({"width": 320, "height": 800})
    page.wait_for_timeout(150)
    fixed_layout()
    page.screenshot(path=str(shots / "narrow.png"), omit_background=True)

    payload["pending"] = []
    payload["feed_limit"] = 3
    expect(page.locator(".activity-row")).to_have_count(3)
    expect(page.locator("#feed-count")).to_have_text("3 / 3")
    expect(page.locator(".activity-row").first).to_have_attribute("data-id", "8")
    # Profile switch/server restart sends an empty history, clearing the old room.
    payload.update(feed=[], queue=[], current=None)
    expect(page.locator(".activity-row")).to_have_count(0)
    expect(page.locator("#feed-empty")).to_be_visible()
    expect(page.locator("#activity-panel")).to_be_visible()
    fixed_layout()
    assert not errors, errors
    browser.close()
print("PASS: fixed height, mixed persistent history, FIFO limit, refresh, duplicate arrivals, safe text, candidate pages and offline recovery.")
print(f"Screenshots: {shots}")
