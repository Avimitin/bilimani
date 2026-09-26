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
shots = root / "analysis/overlay"
shots.mkdir(parents=True, exist_ok=True)
with sync_playwright() as p:
    browser = p.chromium.launch(headless=True, **({"executable_path": args.browser} if args.browser else {}))
    page = browser.new_page(viewport={"width": 1280, "height": 1150}, device_scale_factor=1)
    errors = []
    page.on("pageerror", lambda error: errors.append(str(error)))
    page.goto(args.url)
    expect(page.locator(".current-title")).to_have_text("AA -rebuild-")
    expect(page.locator("#preview-label")).to_contain_text("示例数据")
    page.screenshot(path=str(shots / "preview.png"), full_page=True)
    page.locator("#preview-toggle").click()
    expect(page.locator("#preview-label")).to_contain_text("实时内容")
    expect(page.locator(".current-title")).to_have_count(0)

    live = {
        "connected": True, "ready": True, "status": "弹幕已连接", "capacity": 20,
        "current": {"token": 1, "title": "冥", "requester": "观众甲", "mode": "SP", "chart": "SPA", "remaining": 590, "duration": 600},
        "queue": [{"token": i + 2, "title": "很长的曲名 / " * 8, "requester": "观众乙", "mode": "SP", "chart": "SPH"} for i in range(9)],
        "pending": [{"requester": f"观众{i}", "mode": "SP", "chart": "SPA", "remaining": 47, "duration": 60,
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
    page.set_viewport_size({"width": 520, "height": 800})
    page.goto(args.url + "/queue")
    expect(page.locator(".current-title")).to_have_text("冥")
    expect(page.locator(".queue-row")).to_have_count(6)
    expect(page.locator("#queue-more")).to_have_text("另有 3 首等待中")
    assert page.evaluate("getComputedStyle(document.body).backgroundColor") == "rgba(0, 0, 0, 0)"
    assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
    page.evaluate("window.previousTitle = document.querySelector('.current-title')")
    payload["current"]["remaining"] = 589
    expect(page.locator(".remaining b")).to_have_text("09:49")
    assert page.evaluate("window.previousTitle === document.querySelector('.current-title')"), "Timer caused a full rebuild"
    page.screenshot(path=str(shots / "queue.png"), omit_background=True)
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

    page.set_viewport_size({"width": 520, "height": 960})
    page.goto(args.url + "/interaction")
    expect(page.locator(".choice-panel")).to_have_count(2)
    expect(page.locator("#pending-page")).to_contain_text("1 / 2")
    expect(page.locator(".unavailable-note").first).to_have_text("所请求谱面不存在")
    expect(page.locator("#pending-page")).to_contain_text("2 / 2", timeout=9000)
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
    expect(page.locator("#interaction-empty")).to_be_visible()
    expect(page.locator(".notice")).to_have_count(0)
    assert not errors, errors
    browser.close()
print("PASS: preview/live switch, transparent overlays, safe text, timers, pagination, long titles, offline recovery and empty states.")
print(f"Screenshots: {shots}")
