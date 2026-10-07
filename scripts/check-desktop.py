"""Exercise native desktop hit testing and resize/paint using an isolated DB.

Windows only; no visible window, game, live connection, or third-party packages.
"""
import argparse
import ctypes as c
from ctypes import wintypes as w
from pathlib import Path
import subprocess
import time
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, default=Path("target/release/chart-requester-config.exe"))
    args = parser.parse_args()
    user = c.WinDLL("user32", use_last_error=True)
    enum_proc = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
    user.EnumWindows.argtypes = [enum_proc, w.LPARAM]
    user.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
    user.GetClassNameW.argtypes = [w.HWND, w.LPWSTR, c.c_int]
    user.GetWindowRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
    user.GetClientRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
    user.SendMessageW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM]
    user.SendMessageW.restype = c.c_ssize_t
    user.PostMessageW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM]
    user.SetWindowPos.argtypes = [w.HWND, w.HWND, c.c_int, c.c_int, c.c_int, c.c_int, w.UINT]
    root = Path("analysis") / ("desktop-window-" + uuid.uuid4().hex)
    root.mkdir(parents=True)
    process = subprocess.Popen([
        str(args.exe.resolve()), "--config", str((root / "test.db").resolve()),
        "--hidden", "--frames", "3600",
    ], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        handles = []

        @enum_proc
        def find(window, _):
            pid = w.DWORD()
            name = c.create_unicode_buffer(256)
            user.GetWindowThreadProcessId(window, c.byref(pid))
            user.GetClassNameW(window, name, len(name))
            if pid.value == process.pid and name.value == "ChartRequesterDesktop":
                handles.append(window)
            return True

        deadline = time.monotonic() + 10
        while not handles and time.monotonic() < deadline:
            assert process.poll() is None, process.communicate()
            user.EnumWindows(find, 0)
            time.sleep(0.02)
        assert handles, "Desktop window was not created"
        window = handles[0]

        def rect():
            result = w.RECT()
            assert user.GetWindowRect(window, c.byref(result))
            return result

        def hit(x, y):
            bounds = rect()
            position = ((bounds.left + x) & 0xffff) | (((bounds.top + y) & 0xffff) << 16)
            return user.SendMessageW(window, 0x0084, 0, position)  # WM_NCHITTEST

        while hit(40, 30) != 2 and time.monotonic() < deadline:
            time.sleep(0.02)  # Wait for egui to publish title/button geometry.
        assert hit(40, 30) == 2, "Title must be HTCAPTION (native drag/double click)"
        assert hit(220, 35) == 1, "Create-room button must remain HTCLIENT"
        assert hit(400, 250) == 1, "Form must remain HTCLIENT"
        for width, height in [(1120, 780), (1000, 680), (1480, 920)]:
            # Negative coordinates also exercise monitors left of the primary.
            user.SendMessageW(window, 0x0231, 0, 0)  # WM_ENTERSIZEMOVE starts repaint timer.
            assert user.SetWindowPos(window, None, -40, 60, width, height, 0x14)
            time.sleep(0.1)
            user.SendMessageW(window, 0x000F, 0, 0)  # WM_PAINT / D3D9 Reset
            user.SendMessageW(window, 0x0232, 0, 0)
            bounds = rect()
            client = w.RECT()
            assert user.GetClientRect(window, c.byref(client))
            assert (bounds.left, bounds.top, bounds.right - bounds.left, bounds.bottom - bounds.top) == (-40, 60, width, height)
            assert (client.left, client.top, client.right, client.bottom) == (0, 0, width, height), "System frame still consumes client space"
            for x, y, expected in [
                (2, 2, 13), (width - 2, 2, 14),
                (2, height - 2, 16), (width - 2, height - 2, 17),
                (2, 200, 10), (width - 2, 200, 11),
                (400, 2, 12), (400, height - 2, 15),
            ]:
                assert hit(x, y) == expected, (x, y, hit(x, y), expected)
            assert hit(width - 32, 40) == 1, "Close button must not drag the window"
            assert hit(width // 2, 30) == 2, "Title gap must allow dragging after resize"
            assert process.poll() is None, process.communicate()
        user.PostMessageW(window, 0x0010, 0, 0)  # WM_CLOSE
        out, error = process.communicate(timeout=5)
        assert process.returncode == 0, (out, error)
        print("PASS: borderless client bounds; native title/button hit testing; all eight resize edges; negative position; three sizes and D3D9 repaint; close")
        print("Artifacts:", root)
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate()


if __name__ == "__main__":
    main()
