"""Record the real OBS page with a timed, local-only JSON scenario.

Requires Python Playwright >= 1.63, its FFmpeg download, and an H.264 encoder
(system FFmpeg or imageio-ffmpeg). No game or preview server is needed.
"""
import argparse
import copy
import json
import math
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import time
from urllib.parse import urlsplit

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "reference/overlay-preview-tools"))
from playwright.sync_api import sync_playwright


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scenario", type=pathlib.Path, default=ROOT / "scripts/fixtures/overlay-demo.json")
    parser.add_argument("--output", type=pathlib.Path, default=ROOT / "analysis/overlay-demo.mp4")
    chrome = pathlib.Path("C:/Program Files/Google/Chrome/Application/chrome.exe")
    parser.add_argument("--browser", default=str(chrome) if chrome.is_file() else None)
    parser.add_argument("--ffmpeg", help="Path to FFmpeg with the libx264 encoder")
    parser.add_argument("--background", default="#20252b", help="Solid RGB background for MP4, e.g. #20252b")
    args = parser.parse_args()
    scenario = json.loads(args.scenario.read_text(encoding="utf-8-sig"))
    duration = float(scenario["duration"])
    width, height = scenario.get("width", 480), scenario.get("height", 800)
    steps = scenario["steps"]
    if not math.isfinite(duration) or duration <= 0:
        parser.error("duration must be a positive number of seconds")
    if any(not isinstance(size, int) or size < 2 or size % 2 for size in [width, height]):
        parser.error("width and height must be positive even integers")
    previous = 0
    for step in steps:
        at = float(step["at"])
        if not math.isfinite(at) or not previous <= at < duration:
            parser.error("step times must be ordered and fall within the recording duration")
        previous = at
        if any(item.get("kind") not in ("chat", "event") for item in step.get("feed", [])):
            parser.error("feed records must have kind chat or event")
    if not re.fullmatch(r"#[0-9a-fA-F]{6}", args.background):
        parser.error("background must be a six-digit RGB hex color")
    if args.output.suffix.lower() != ".mp4":
        parser.error("output must have the .mp4 extension")

    ffmpeg = args.ffmpeg or shutil.which("ffmpeg")
    if not ffmpeg:
        try:
            import imageio_ffmpeg
        except ImportError:
            parser.error("Install imageio-ffmpeg or supply --ffmpeg to encode MP4")
        ffmpeg = imageio_ffmpeg.get_ffmpeg_exe()

    state = {
        "connected": True, "ready": True, "status": "弹幕已连接", "capacity": 20,
        "current": None, "queue": [], "pending": [], "notices": [],
        "feed_limit": max(1, min(100, int(scenario.get("feed_limit", 10)))), "feed": [],
    }
    step_index = 0
    next_id = 1
    started = None
    current_since = pending_since = 0
    applied = []

    def snapshot():
        nonlocal step_index, next_id, current_since, pending_since
        elapsed = 0 if started is None else time.monotonic() - started
        while started is not None and step_index < len(steps) and float(steps[step_index]["at"]) <= elapsed:
            step = steps[step_index]
            patch = copy.deepcopy(step.get("set", {}))
            state.update(patch)
            if "current" in patch:
                current_since = float(step["at"])
            if "pending" in patch:
                pending_since = float(step["at"])
            if step.get("clear_feed"):
                state["feed"] = []
            for message in step.get("feed", []):
                state["feed"].append({**message, "id": next_id, "at": int(elapsed * 1000)})
                next_id += 1
            state["feed"] = state["feed"][-state["feed_limit"]:]
            applied.append({"at": step["at"], "applied_at": round(elapsed, 3), "feed_count": len(state["feed"])})
            print(f"{elapsed:5.1f}s: step {step_index + 1}/{len(steps)}, {len(state['feed'])} records", flush=True)
            step_index += 1
        result = copy.deepcopy(state)
        if result["current"]:
            song = result["current"]
            song["remaining"] = max(0, math.ceil(song["remaining"] - (elapsed - current_since)))
        for choice in result["pending"]:
            choice["remaining"] = max(0, math.ceil(choice["remaining"] - (elapsed - pending_since)))
        return result

    # Fulfill every browser request locally, including the production JS polls.
    # The synthetic origin never reaches a live room, DLL, or network service.
    assets = {
        "/queue": ("index.html", "text/html; charset=utf-8"),
        "/overlay.css": ("overlay.css", "text/css; charset=utf-8"),
        "/overlay.js": ("overlay.js", "application/javascript; charset=utf-8"),
    }

    def route_request(route):
        url = urlsplit(route.request.url)
        if url.netloc != "overlay.test":
            route.abort()
        elif url.path == "/api/state":
            route.fulfill(json=snapshot())
        elif url.path in assets:
            name, mime = assets[url.path]
            route.fulfill(path=ROOT / "web/card" / name, content_type=mime)
        else:
            route.fulfill(status=404, body="Not found")

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="overlay-recording-") as work, sync_playwright() as playwright:
        browser = playwright.chromium.launch(headless=True, **({"executable_path": args.browser} if args.browser else {}))
        page = browser.new_page(viewport={"width": width, "height": height}, device_scale_factor=1, reduced_motion="no-preference")
        errors = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        page.route("**/*", route_request)
        page.goto("http://overlay.test/queue")
        page.add_style_tag(content=f"html, body {{ background: {args.background}; }}")
        page.wait_for_timeout(500)
        recording = pathlib.Path(work) / "recording.webm"
        page.screencast.start(path=recording, size={"width": width, "height": height}, quality=100)
        started = time.monotonic()
        print(f"Recording {duration:g}s at {width}x{height}...", flush=True)
        # Playwright's wait pumps browser events so 500ms state polls continue.
        while time.monotonic() - started < duration:
            page.wait_for_timeout(min(250, max(1, (duration - (time.monotonic() - started)) * 1000)))
        page.screencast.stop()
        browser.close()
        if errors:
            raise RuntimeError("Page errors: " + "; ".join(errors))
        if step_index != len(steps):
            raise RuntimeError("Recording ended before all steps were polled; leave at least 0.5s after the final step")
        subprocess.run([
            ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-i", str(recording),
            "-an", "-c:v", "libx264", "-preset", "medium", "-crf", "18",
            "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(args.output),
        ], check=True)
    args.output.with_suffix(".timeline.json").write_text(json.dumps({
        "scenario": str(args.scenario.resolve()), "duration": duration,
        "width": width, "height": height, "steps": applied,
    }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"Saved: {args.output.resolve()}")


if __name__ == "__main__":
    main()
