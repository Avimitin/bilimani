"""Serve an overlay with a repeating JSON timeline. Python 3.10+, no dependencies.

Examples:
    python scripts/preview-overlay.py --style mecha
    python scripts/preview-overlay.py --style card --speed 2
"""
import argparse
import copy
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import math
from pathlib import Path
import time
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parent.parent


class Timeline:
    def __init__(self, scenario):
        self.duration = float(scenario["duration"])
        if not math.isfinite(self.duration) or self.duration <= 0:
            raise ValueError("duration must be positive and finite")
        self.limit = scenario.get("feed_limit", 10)
        if type(self.limit) is not int or not 1 <= self.limit <= 100:
            raise ValueError("feed_limit must be an integer from 1 to 100")
        self.steps = copy.deepcopy(scenario["steps"])
        self.messages = 0
        previous = 0
        for step in self.steps:
            at = float(step["at"])
            if not math.isfinite(at) or not previous <= at < self.duration:
                raise ValueError("step times must be ordered and within duration")
            step["at"] = previous = at
            patch = step.get("set", {})
            if not isinstance(patch, dict) or {"feed", "feed_limit"}.intersection(patch):
                raise ValueError("set must be an object; use feed/clear_feed to change history")
            for key in ("queue", "pending", "notices"):
                if key in patch and not isinstance(patch[key], list):
                    raise ValueError(f"set.{key} must be an array")
            for entry in step.get("feed", []):
                if entry.get("kind") not in ("chat", "event") or not isinstance(entry.get("text"), str):
                    raise ValueError("feed entries require kind chat/event and text")
            self.messages += len(step.get("feed", []))

    def snapshot(self, elapsed):
        """Pure reconstruction: all browser tabs see the same clock, no poll side effects."""
        cycle, offset = divmod(max(0, elapsed), self.duration)
        state = {
            "version": "preview", "connected": True, "ready": True,
            "status": "模拟直播间已连接", "capacity": 20, "room": None,
            "current": None, "queue": [], "pending": [], "notices": [],
            "now_playing": {"phase": "idle", "song": None, "players": []},
            "feed": [], "feed_limit": self.limit,
        }
        current_since = pending_since = 0
        next_id = int(cycle) * max(1, self.messages) + 1
        for step in self.steps:
            if step["at"] > offset:
                break
            patch = copy.deepcopy(step.get("set", {}))
            state.update(patch)
            if "current" in patch:
                current_since = step["at"]
            if "pending" in patch:
                pending_since = step["at"]
            if step.get("clear_feed"):
                state["feed"] = []
            for entry in step.get("feed", []):
                state["feed"].append({
                    "name": "", **copy.deepcopy(entry), "id": next_id,
                    "at": int(cycle * self.duration + step["at"]),
                })
                next_id += 1
            state["feed"] = state["feed"][-self.limit:]
        if state["current"]:
            current = state["current"]
            current["remaining"] = max(0, math.ceil(current.get("remaining", 0) - (offset - current_since)))
        for choice in state["pending"]:
            choice["remaining"] = max(0, math.ceil(choice.get("remaining", 0) - (offset - pending_since)))
        return state


def handler_for(directory, timeline, clock):
    directory = directory.resolve()

    class Handler(SimpleHTTPRequestHandler):
        extensions_map = {
            **SimpleHTTPRequestHandler.extensions_map,
            ".js": "application/javascript; charset=utf-8",
            ".css": "text/css; charset=utf-8", ".html": "text/html; charset=utf-8",
        }

        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=str(directory), **kwargs)

        def log_message(self, *_):
            pass  # Polling twice a second should not flood the terminal.

        def end_headers(self):
            self.send_header("Cache-Control", "no-store")
            super().end_headers()

        def serve(self, head=False):
            port = self.server.server_port
            hosts = {f"127.0.0.1:{port}", f"localhost:{port}"}
            if self.headers.get("Host") not in hosts or self.headers.get("Origin") not in {None, *(f"http://{h}" for h in hosts)}:
                self.send_error(403)
                return
            path = urlsplit(self.path).path
            if path in ("/api/state", "/api/now-playing"):
                state = timeline.snapshot(clock())
                payload = state if path == "/api/state" else state["now_playing"]
                data = json.dumps(payload, ensure_ascii=False, allow_nan=False).encode("utf-8")
                self.send_response(200)
                self.send_header("Content-Type", "application/json; charset=utf-8")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                if not head:
                    self.wfile.write(data)
                return
            if path in ("/", "/queue"):
                path = "/index.html"
            parts = unquote(path).removeprefix("/").split("/")
            if any(not p or p.startswith(".") or p.endswith((".", " ")) or any(c in p for c in "\\:\x00") for p in parts):
                self.send_error(404)
                return
            asset = directory.joinpath(*parts).resolve()
            if not asset.is_relative_to(directory) or not asset.is_file():
                self.send_error(404)
                return
            # Keep SimpleHTTPRequestHandler's MIME handling and streaming, with
            # an explicit boundary against directory listings and escaping links.
            self.path = path
            if head:
                super().do_HEAD()
            else:
                super().do_GET()

        def do_GET(self):
            self.serve()

        def do_HEAD(self):
            self.serve(head=True)

    return Handler


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--style", default="mecha", help="Style directory name under web/ (default: mecha)")
    parser.add_argument("--scenario", type=Path, default=ROOT / "scripts/fixtures/overlay-loop.json")
    parser.add_argument("--port", type=int, default=32134)
    parser.add_argument("--speed", type=float, default=1, help="Timeline speed multiplier (default: 1)")
    args = parser.parse_args()
    if not 1 <= args.port <= 65535:
        parser.error("port must be from 1 to 65535")
    if not math.isfinite(args.speed) or args.speed <= 0:
        parser.error("speed must be positive and finite")
    directory = ROOT / "web" / args.style
    if not (directory / "index.html").is_file():
        parser.error(f"Missing {directory / 'index.html'}")
    try:
        timeline = Timeline(json.loads(args.scenario.read_text(encoding="utf-8-sig")))
    except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
        parser.error(f"Invalid scenario: {error}")
    started = time.monotonic()
    clock = lambda: (time.monotonic() - started) * args.speed
    try:
        server = ThreadingHTTPServer(("127.0.0.1", args.port), handler_for(directory, timeline, clock))
    except OSError as error:
        parser.error(f"Cannot start preview: {error}; try another --port")
    print(f"Preview: http://127.0.0.1:{args.port}/queue", flush=True)
    print(f"Style: {directory.resolve()}\nScenario: {args.scenario.resolve()}", flush=True)
    print(f"Loop: {timeline.duration:g}s at {args.speed:g}x (real time {timeline.duration / args.speed:g}s). Ctrl+C to stop.", flush=True)
    print("Edit HTML/CSS/JS and refresh the browser. Restart this script after editing the timeline.", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
