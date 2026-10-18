"""Windows native acceptance for auxiliary-window-probe (no product UI automation).

Usage: python scripts/auxiliary-window-acceptance.py --binary <exe> --out <new directory>
Records native ownership, desktop z-order, focus, bands and lifecycle identities.
macOS/X11/Wayland use the same probe's stdin protocol with native platform inspection.
"""
import argparse
import ctypes as c
from ctypes import wintypes as w
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import time


def main():
    if sys.platform != "win32":
        raise SystemExit("This inspector requires Windows; run the native probe on other platforms.")
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--without-shadows", action="store_true")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=False)
    user = c.WinDLL("user32", use_last_error=True)
    signatures = {
        "FindWindowW": ([w.LPCWSTR, w.LPCWSTR], w.HWND),
        "GetWindow": ([w.HWND, w.UINT], w.HWND),
        "GetTopWindow": ([w.HWND], w.HWND),
        "GetWindowLongPtrW": ([w.HWND, c.c_int], c.c_ssize_t),
        "IsWindowVisible": ([w.HWND], w.BOOL),
        "IsIconic": ([w.HWND], w.BOOL),
        "GetForegroundWindow": ([], w.HWND),
        "SetForegroundWindow": ([w.HWND], w.BOOL),
        "AllowSetForegroundWindow": ([w.DWORD], w.BOOL),
        "CreateWindowExW": ([w.DWORD, w.LPCWSTR, w.LPCWSTR, w.DWORD,
                             c.c_int, c.c_int, c.c_int, c.c_int,
                             w.HWND, w.HMENU, w.HINSTANCE, w.LPVOID], w.HWND),
        "SetWindowPos": ([w.HWND, w.HWND, c.c_int, c.c_int, c.c_int, c.c_int, w.UINT], w.BOOL),
        "DestroyWindow": ([w.HWND], w.BOOL),
        "PeekMessageW": ([c.POINTER(w.MSG), w.HWND, w.UINT, w.UINT, w.UINT], w.BOOL),
        "TranslateMessage": ([c.POINTER(w.MSG)], w.BOOL),
        "DispatchMessageW": ([c.POINTER(w.MSG)], c.c_ssize_t),
    }
    for name, (arguments, result) in signatures.items():
        function = getattr(user, name)
        function.argtypes, function.restype = arguments, result
    command_line = [str(args.binary.resolve())]
    if args.without_shadows:
        command_line.append("--without-shadows")
    process = subprocess.Popen(command_line, stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                               text=True, encoding="utf-8", errors="replace",
                               creationflags=subprocess.CREATE_NO_WINDOW)
    trace = []
    generations = {}
    presented = {}
    focus_gains = {}
    foreground_permission = {}

    def read_output():
        with (args.out / "native.jsonl").open("w", encoding="utf-8") as output:
            for line in process.stdout:
                trace.append(line.rstrip())
                output.write(line)
                output.flush()
                try:
                    event = json.loads(line)
                    if event.get("kind") == "ready":
                        generations[event["window"]] = event["generation"]
                    elif event.get("kind") == "presented":
                        presented[event["window"]] = event["generation"]
                    elif event.get("kind") == "focus" and event.get("focused"):
                        key = event["window"]
                        focus_gains[key] = focus_gains.get(key, 0) + 1
                    elif event.get("kind") == "foreground-permission":
                        foreground_permission.update(event)
                except (ValueError, KeyError):
                    pass

    reader = threading.Thread(target=read_output, daemon=True)
    reader.start()
    names = {"owner": "NanaUI auxiliary owner", "guide": "NanaUI auxiliary guide",
             "card": "NanaUI auxiliary card", "underlay": "NanaUI auxiliary underlay"}
    records = []
    blocked = []
    outsider = None

    def snapshot():
        message = w.MSG()
        while user.PeekMessageW(c.byref(message), None, 0, 0, 1):
            user.TranslateMessage(c.byref(message))
            user.DispatchMessageW(c.byref(message))
        foreground = user.GetForegroundWindow()
        handles = {key: user.FindWindowW(None, title) for key, title in names.items()}
        wanted = {handle for handle in handles.values() if handle}
        if outsider:
            wanted.add(outsider)
        order = []
        current = user.GetTopWindow(None)
        while wanted and current and current not in order:
            order.append(current)
            wanted.discard(current)
            current = user.GetWindow(current, 2)  # GW_HWNDNEXT
        windows = {}
        for key, title in names.items():
            handle = handles[key]
            windows[key] = {"handle": handle, "visible": bool(user.IsWindowVisible(handle)),
                            "minimized": bool(user.IsIconic(handle)),
                            "owner": user.GetWindow(handle, 4) if handle else None,
                            "topmost": bool(user.GetWindowLongPtrW(handle, -20) & 8),
                            "rank": order.index(handle) if handle in order else len(order)}
        return {"windows": windows, "foreground": foreground, "outsider": outsider,
                "generations": dict(generations), "presented": dict(presented),
                "outsider_rank": order.index(outsider) if outsider in order else len(order)}

    def wait(name, predicate, settle=0.5):
        deadline = time.monotonic() + 30
        stable_since = None
        while time.monotonic() < deadline:
            state = snapshot()
            if predicate(state):
                if stable_since is None:
                    stable_since = time.monotonic()
                if time.monotonic() - stable_since >= settle:
                    records.append({"step": name, **state})
                    return state
            else:
                stable_since = None
            if process.poll() is not None:
                raise AssertionError(f"probe exited: {process.returncode}; {trace[-5:]}")
            time.sleep(0.05)
        raise AssertionError(f"{name}: {state}; {trace[-5:]}")

    def command(value):
        process.stdin.write(value + "\n")
        process.stdin.flush()

    def above(state):
        windows = state["windows"]
        return (windows["underlay"]["handle"] and windows["underlay"]["visible"]
                and windows["underlay"]["rank"] > windows["owner"]["rank"]
                and not windows["underlay"]["owner"] and all(
                   windows[key]["visible"] and windows[key]["rank"] < windows["owner"]["rank"]
                   and windows[key]["owner"] == windows["owner"]["handle"]
                   for key in ("guide", "card")))

    def frames_ready():
        return all(presented.get(key) == generations.get(key) and generations.get(key)
                   for key in (0, 1, 2, 3))

    try:
        wait("hidden-owner-preparation", lambda s: s["windows"]["guide"]["visible"]
             and not s["windows"]["owner"]["visible"] and presented.get(1) == 1)
        command("main")
        first = wait("three-normal-windows", lambda s: frames_ready() and above(s) and
                     all(not v["topmost"] for v in s["windows"].values()))
        identity = first["windows"]["card"]["handle"]
        command("focus-guide")
        wait("guide-focused", lambda s: s["foreground"] == s["windows"]["guide"]["handle"], settle=0)
        owner_focus_gains = focus_gains.get(0, 0)
        command("focus")
        wait("owner-focused-below-auxiliaries", lambda s: above(s) and
             s["foreground"] == s["windows"]["owner"]["handle"] and
             focus_gains.get(0, 0) > owner_focus_gains, settle=0)
        # A separate normal desktop window can cover every non-topmost auxiliary.
        command(f"allow-external {os.getpid()}")
        permission_deadline = time.monotonic() + 5
        while not foreground_permission and time.monotonic() < permission_deadline:
            time.sleep(0.01)
        outsider = user.CreateWindowExW(0, "STATIC", "Auxiliary acceptance other application",
                                       0x10CF0000, 160, 160, 380, 320, None, None, None, None)
        assert outsider, "failed to create independent normal window"
        assert user.SetWindowPos(outsider, None, 0, 0, 0, 0, 0x13)  # NOACTIVATE|NOMOVE|NOSIZE
        # A real application switch activates the other window. A background
        # NOACTIVATE raise alone can be rejected by foreground restrictions.
        user.SetForegroundWindow(outsider)
        if user.GetForegroundWindow() != outsider:
            blocked.append({"step": "other-application-above-normal-windows",
                            "reason": "Windows rejected the external foreground request",
                            "permission": dict(foreground_permission),
                            **snapshot()})
        else:
            wait("other-application-above-normal-windows", lambda s: all(
                s["outsider_rank"] < v["rank"] for v in s["windows"].values()))
        user.AllowSetForegroundWindow(process.pid)
        command("focus")
        wait("return-to-owner", lambda s: above(s) and
             s["foreground"] == s["windows"]["owner"]["handle"], settle=0)
        command("top")
        wait("owner-topmost-inherited", lambda s: above(s) and
             all(v["topmost"] for v in s["windows"].values()))
        command("hide")
        wait("card-hidden", lambda s: not s["windows"]["card"]["visible"])
        command("normal")
        before = wait("hidden-card-demoted", lambda s: all(not v["topmost"] for v in s["windows"].values()))
        card_focus_gains = focus_gains.get(2, 0)
        command("show")
        wait("same-card-shown-without-focus", lambda s: above(s) and
             s["windows"]["card"]["handle"] == identity and
             s["foreground"] != identity and focus_gains.get(2, 0) == card_focus_gains)
        command("minimize")
        wait("owner-minimized", lambda s: s["windows"]["owner"]["minimized"])
        command("restore")
        wait("owner-restored-with-auxiliaries-above", lambda s: above(s) and
             not s["windows"]["owner"]["minimized"])
        command("close")
        wait("card-closed", lambda s: not s["windows"]["card"]["handle"])
        command("top")
        command("reopen")
        wait("card-created-during-owner-level-change", lambda s: presented.get(2) == 2 and above(s) and
             all(v["topmost"] for v in s["windows"].values()))
        command("normal")
        wait("recreated-card-returns-to-normal", lambda s: above(s) and
             all(not v["topmost"] for v in s["windows"].values()))
        command("close-owner")
        assert process.wait(timeout=20) == 0
        wait_closed = snapshot()
        assert not any(v["handle"] for v in wait_closed["windows"].values())
        records.append({"step": "owner-close-releases-all-children", **wait_closed})
        report = {"pass": not blocked, "platform": "windows", "steps": records, "blocked": blocked}
    except Exception as error:
        report = {"pass": False, "error": str(error), "steps": records, "blocked": blocked}
    finally:
        if process.poll() is None:
            command("quit")
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.terminate()
        reader.join(timeout=2)
        if outsider:
            user.DestroyWindow(outsider)
    (args.out / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps({"pass": report["pass"], "steps": len(records), "out": str(args.out)}))
    return 0 if report["pass"] else (2 if "error" not in report else 1)


if __name__ == "__main__":
    raise SystemExit(main())
