"""Optional browser checks against the real preview server (requires Playwright).

Run `cargo run --example overlay_preview` first. Screenshots stay in analysis/.
The Windows browser executable can be supplied with --browser; otherwise use
the Chromium installation managed by Playwright.
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
shots = root / "analysis/overlay-compact"
shots.mkdir(parents=True, exist_ok=True)
with sync_playwright() as p:
    browser = p.chromium.launch(headless=True, **({"executable_path": args.browser} if args.browser else {}))
    page = browser.new_page(viewport={"width": 880, "height": 800}, device_scale_factor=1)
    errors = []
    page.on("pageerror", lambda error: errors.append(str(error)))
    page.goto(args.url)
    expect(page).to_have_url(args.url + "/queue")
    expect(page.locator(".connection-banner")).to_be_visible()
    expect(page.locator(".current-title")).to_have_count(0)
    expect(page.locator("button, input")).to_have_count(0)
    live = {
        "connected": True, "ready": True, "status": "弹幕已连接", "capacity": 20,
        "current": {"token": 1, "title": "冥", "requester": "观众甲", "mode": "SP", "chart": "SPA", "chart_style": "red", "remaining": 590, "duration": 600},
        "queue": [{"token": i + 2, "title": "很长的曲名 / " * 8, "requester": "观众乙", "mode": "SP", "chart": "SPH", "chart_style": "amber"} for i in range(9)],
        "pending": [{"requester": f"观众{i}", "mode": "SP", "chart": "SPA", "chart_style": "red", "remaining": 47, "duration": 60,
                     "candidates": [{"title": "AA", "available": True}, {"title": "AA -rebuild-", "available": False}]} for i in range(3)],
        "notices": [{"at": 1, "text": "已加入队列"}]
    }
    payload = copy.deepcopy(live)
    fail = False
    def state_route(route):
        if fail:
            route.abort()
        else:
            route.fulfill(status=200, content_type="application/json", body=json.dumps(payload, ensure_ascii=False))
    page.route("**/api/state", state_route)
    page.set_viewport_size({"width": 480, "height": 800})
    page.goto(args.url + "/queue")
    expect(page.locator(".current-title")).to_have_text("冥")
    expect(page.locator("#current-content .chart")).to_have_css("color", "rgb(238, 146, 152)")
    expect(page.locator(".queue-row .chart").first).to_have_css("color", "rgb(226, 199, 116)")
    expect(page.locator(".queue-row")).to_have_count(6)
    expect(page.locator("#queue-more")).to_have_text("另有 3 首等待中")
    assert page.evaluate("getComputedStyle(document.body).backgroundColor") == "rgba(0, 0, 0, 0)"
    assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
    panel = page.locator(".queue-panel").bounding_box()
    popup = page.locator("#queue-popup").bounding_box()
    assert panel["height"] < 480, "Queue should use a compact layout"
    assert popup["y"] >= panel["y"] + panel["height"], "Popup must sit below the queue"
    expect(page.locator(".queue-banner")).to_contain_text("点歌 <曲名> [难度]")
    page.evaluate("window.previousTitle = document.querySelector('.current-title')")
    payload["current"]["remaining"] = 589
    expect(page.locator(".remaining b")).to_have_text("09:49")
    assert page.evaluate("window.previousTitle === document.querySelector('.current-title')"), "Timer caused a full rebuild"
    page.screenshot(path=str(shots / "queue.png"), omit_background=True)
    payload["current"]["chart"] = "EXPERT+"
    payload["current"]["chart_style"] = "purple"
    expect(page.locator("#current-content .chart")).to_have_text("EXPERT+")
    expect(page.locator("#current-content .chart")).to_have_css("color", "rgb(199, 161, 232)")
    injection = '<img src=x onerror="window.injected=true">'
    payload["current"]["title"] = injection
    expect(page.locator(".current-title")).to_have_text(injection)
    assert page.locator("img").count() == 0
    assert not page.evaluate("Boolean(window.injected)")
    fail = True
    expect(page.locator(".connection-banner").first).to_contain_text("等待游戏连接", timeout=8000)
    expect(page.locator(".queue-row")).to_have_count(0)
    expect(page.locator(".current-title")).to_have_count(0)
    fail = False
    payload = copy.deepcopy(live)
    expect(page.locator(".current-title")).to_have_text("冥")

    page.set_viewport_size({"width": 480, "height": 800})
    page.goto(args.url + "/queue?demo=1")
    expect(page.locator(".current-title")).to_have_text(live["current"]["title"])  # Old demo query still uses live state.
    expect(page.locator(".choice-panel")).to_have_count(1)
    expect(page.locator("#pending-page")).to_contain_text("1 / 3")
    expect(page.locator(".unavailable-note").first).to_have_text("所请求谱面不存在")
    expect(page.locator("#pending-page")).to_contain_text("2 / 3", timeout=9000)
    payload["pending"] = payload["pending"][:1]
    payload["pending"][0]["remaining"] = 9
    expect(page.locator(".countdown.urgent b")).to_have_text("09")
    page.screenshot(path=str(shots / "interaction.png"), omit_background=True)
    payload["pending"][0]["candidates"] = [{"title": f"候选曲目 {i+1}", "available": True} for i in range(20)]
    expect(page.locator(".candidate")).to_have_count(20)
    assert page.locator(".choice-panel").bounding_box()["height"] < 880
    page.set_viewport_size({"width": 320, "height": 960})
    assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
    payload["pending"] = []
    payload["notices"] = []
    expect(page.locator("#queue-popup")).to_be_hidden()
    expect(page.locator(".notice")).to_have_count(0)
    payload["notices"] = [{"at": 2, "text": "新的点歌已加入队列"}]
    expect(page.locator("#queue-popup")).to_be_visible()
    expect(page.locator(".notice")).to_have_count(1)
    expect(page.locator("#queue-popup")).to_be_hidden(timeout=9000)
    page.wait_for_timeout(600)
    expect(page.locator("#queue-popup")).to_be_hidden()
    payload["notices"] = [{"at": 3, "text": "新的点歌已加入队列"}]
    expect(page.locator("#queue-popup")).to_be_visible()
    assert not errors, errors
    browser.close()
print("PASS: compact queue, bottom popup, notice expiry, live-only page, safe text, timers, pagination and offline recovery.")
print(f"Screenshots: {shots}")
