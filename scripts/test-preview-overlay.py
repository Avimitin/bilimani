"""Regression checks for timeline replay and the standalone preview HTTP server."""
import copy
from http.server import ThreadingHTTPServer
import json
import math
from pathlib import Path
import runpy
import tempfile
import threading
import unittest
from urllib.error import HTTPError
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parent.parent
preview = runpy.run_path(str(ROOT / "scripts/preview-overlay.py"))
Timeline, handler_for = preview["Timeline"], preview["handler_for"]


class PreviewTests(unittest.TestCase):
    def setUp(self):
        self.scenario = {
            "duration": 10, "feed_limit": 2,
            "steps": [
                {"at": 0, "feed": [{"kind": "chat", "name": "观众", "text": str(i)} for i in range(3)]},
                {"at": 2, "set": {"current": {"remaining": 5}, "pending": [{"remaining": 5}]},
                 "feed": [{"kind": "event", "text": "加入队列"}]},
                {"at": 4, "clear_feed": True},
            ],
        }

    def test_replay_countdowns_and_loop_reset_are_independent_of_polling(self):
        source = copy.deepcopy(self.scenario)
        timeline = Timeline(source)
        first = timeline.snapshot(3.2)
        self.assertEqual(first["current"]["remaining"], 4)
        self.assertEqual(first["pending"][0]["remaining"], 4)
        self.assertEqual([m["id"] for m in first["feed"]], [3, 4])
        first["feed"].clear()
        self.assertEqual(len(timeline.snapshot(3.2)["feed"]), 2)
        self.assertEqual(timeline.snapshot(9)["current"]["remaining"], 0)
        self.assertEqual(timeline.snapshot(9)["feed"], [])
        self.assertIsNone(timeline.snapshot(10)["current"])
        self.assertEqual(timeline.snapshot(10)["pending"], [])
        self.assertEqual([m["id"] for m in timeline.snapshot(13.2)["feed"]], [7, 8])
        # Jumping many cycles, or refreshing/opening another tab, adds no duplicates.
        self.assertEqual(timeline.snapshot(23.2), timeline.snapshot(23.2))
        self.assertEqual(source, self.scenario)

    def test_invalid_timing_and_feed_are_rejected_before_startup(self):
        for duration in (0, -1, float("nan"), float("inf")):
            with self.assertRaises(ValueError):
                Timeline({**self.scenario, "duration": duration})
        for steps in ([{"at": 10}], [{"at": 4}, {"at": 2}], [{"at": 0, "feed": [{"kind": "unknown", "text": "x"}]}]):
            with self.assertRaises(ValueError):
                Timeline({**self.scenario, "steps": steps})

    def test_both_shipped_timelines_replay_with_bounded_history(self):
        for name in ("overlay-loop.json", "overlay-demo.json"):
            timeline = Timeline(json.loads((ROOT / "scripts/fixtures" / name).read_text(encoding="utf-8-sig")))
            for at in range(math.ceil(timeline.duration * 2)):
                state = timeline.snapshot(at)
                json.dumps(state, allow_nan=False)
                self.assertLessEqual(len(state["feed"]), state["feed_limit"])
                self.assertEqual(len({m["id"] for m in state["feed"]}), len(state["feed"]))

    def test_http_assets_refresh_and_all_apis_follow_the_same_clock(self):
        timeline = Timeline(json.loads((ROOT / "scripts/fixtures/overlay-loop.json").read_text(encoding="utf-8-sig")))
        now = [14]
        with tempfile.TemporaryDirectory() as work:
            directory = Path(work)
            (directory / "index.html").write_text("first", encoding="utf-8")
            (directory / ".private").write_text("private", encoding="utf-8")
            server = ThreadingHTTPServer(("127.0.0.1", 0), handler_for(directory, timeline, lambda: now[0]))
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            base = f"http://127.0.0.1:{server.server_port}"
            try:
                def get(path):
                    return urlopen(base + path, timeout=2)
                with get("/queue") as response:
                    self.assertEqual(response.read(), b"first")
                    self.assertEqual(response.headers["Cache-Control"], "no-store")
                (directory / "index.html").write_text("changed", encoding="utf-8")
                with get("/queue") as response:
                    self.assertEqual(response.read(), b"changed")
                with get("/api/state") as response:
                    state = json.load(response)
                with get("/api/now-playing") as response:
                    self.assertEqual(json.load(response), state["now_playing"])
                with get("/api/lane-counts") as response:
                    counts = json.load(response)
                    self.assertEqual(counts['song_id'], state['now_playing']['song']['id'])
                    self.assertEqual(counts['charts'][0]['lane_counts'], state['now_playing']['song']['charts'][0]['lane_counts'])
                with get("/api/lane-order") as response:
                    order = json.load(response)
                    self.assertEqual(order['lane_order'], state['now_playing']['lane_order'])
                    self.assertEqual(order['players'], state['now_playing']['players'])
                self.assertEqual(len(state["pending"][0]["candidates"]), 9)
                with urlopen(Request(base + "/api/state", method="HEAD"), timeout=2) as response:
                    self.assertEqual(response.read(), b"")
                now[0] = 42
                with get("/api/state") as response:
                    self.assertEqual(json.load(response)["feed"], [])
                for path, field in [('lane-counts', 'charts'), ('lane-order', 'lane_order')]:
                    with get('/api/' + path) as response:
                        cleared = json.load(response)
                        self.assertIsNone(cleared['song_id'])
                        self.assertEqual(cleared[field], [])
                for path in ("/.private", "/%2e%2e/README.md", "/missing"):
                    with self.assertRaises(HTTPError) as error:
                        get(path)
                    self.assertEqual(error.exception.code, 404)
            finally:
                server.shutdown()
                server.server_close()
                thread.join()


if __name__ == "__main__":
    unittest.main()
