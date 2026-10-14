#!/usr/bin/env python3
"""Run only through Link's isolated runner: tray, resident and crash recovery."""
import argparse
import ctypes
import json
import os
from pathlib import Path
import queue
import subprocess
import threading
import time

from gi.repository import Gio, GLib

ROOT = Path(os.environ.get("ARCADE_E2E_ROOT", "/missing"))
if os.environ.get("ARCADE_E2E_INNER") != "1" or not str(ROOT).startswith("/tmp/arcade-e2e-"):
    raise SystemExit("Use: CARGO_HOME=... python3 ../Arcade-link/tools/e2e.py run -- python3 scripts/tray-resident-e2e.py ...")
for variable in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "ARCADE_HOME"):
    if not Path(os.environ[variable]).is_relative_to(ROOT):
        raise SystemExit(f"{variable} is not isolated")


def wait(check, timeout=8):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        result = check()
        if result:
            return result
        time.sleep(0.02)  # Bounded test synchronization, never app polling.
    raise AssertionError("Timed out waiting for isolated test state")


def window():
    result = subprocess.run(["xdotool", "search", "--name", "^Arcade Tools$"], capture_output=True, text=True)
    return result.stdout.strip().splitlines()[0] if result.returncode == 0 else None


def close_window(identifier):
    # Send WM_DELETE_WINDOW, including on Xvfb without a window manager.
    class Data(ctypes.Union):
        _fields_ = [("b", ctypes.c_char * 20), ("s", ctypes.c_short * 10), ("l", ctypes.c_long * 5)]

    class ClientMessage(ctypes.Structure):
        _fields_ = [("type", ctypes.c_int), ("serial", ctypes.c_ulong), ("send_event", ctypes.c_int),
                    ("display", ctypes.c_void_p), ("window", ctypes.c_ulong), ("message_type", ctypes.c_ulong),
                    ("format", ctypes.c_int), ("data", Data)]

    class Event(ctypes.Union):
        _fields_ = [("client", ClientMessage), ("padding", ctypes.c_long * 24)]

    x11 = ctypes.CDLL("libX11.so.6")
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XInternAtom.restype = ctypes.c_ulong
    x11.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
    x11.XSendEvent.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_long, ctypes.POINTER(Event)]
    x11.XFlush.argtypes = [ctypes.c_void_p]
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    display = x11.XOpenDisplay(None)
    assert display
    event = Event()
    event.client.type = 33
    event.client.display = display
    event.client.window = int(identifier)
    event.client.message_type = x11.XInternAtom(display, b"WM_PROTOCOLS", 0)
    event.client.format = 32
    event.client.data.l[0] = x11.XInternAtom(display, b"WM_DELETE_WINDOW", 0)
    x11.XSendEvent(display, int(identifier), 0, 0, ctypes.byref(event))
    x11.XFlush(display)
    x11.XCloseDisplay(display)


class Watcher:
    XML = '''<node><interface name="org.kde.StatusNotifierWatcher">
      <method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
      <method name="RegisterStatusNotifierHost"><arg type="s" direction="in"/></method>
      <property name="RegisteredStatusNotifierItems" type="as" access="read"/>
      <property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
      <property name="ProtocolVersion" type="i" access="read"/>
      <signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
      <signal name="StatusNotifierItemUnregistered"><arg type="s"/></signal>
      <signal name="StatusNotifierHostRegistered"/>
    </interface></node>'''

    def __init__(self):
        self.items = []
        self.bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        self.loop = GLib.MainLoop()
        threading.Thread(target=self.loop.run, daemon=True).start()
        interface = Gio.DBusNodeInfo.new_for_xml(self.XML).interfaces[0]
        self.object = self.bus.register_object("/StatusNotifierWatcher", interface, self.method, self.property, None)
        self.owner = None

    def start(self):
        self.owner = Gio.bus_own_name_on_connection(self.bus, "org.kde.StatusNotifierWatcher", Gio.BusNameOwnerFlags.NONE, None, None)

    def stop(self):
        if self.owner is not None:
            Gio.bus_unown_name(self.owner)
            self.owner = None

    def method(self, connection, sender, path, interface, method, parameters, invocation):
        if method == "RegisterStatusNotifierItem":
            service = parameters.unpack()[0]
            item = (sender, service) if service.startswith("/") else (service, "/StatusNotifierItem")
            self.items.append(item)
            connection.emit_signal(None, path, interface, "StatusNotifierItemRegistered", GLib.Variant("(s)", (item[0] + item[1],)))
        invocation.return_value(GLib.Variant("()", ()))

    def property(self, connection, sender, path, interface, name):
        if name == "RegisteredStatusNotifierItems":
            return GLib.Variant("as", [service + path for service, path in self.items])
        if name == "IsStatusNotifierHostRegistered":
            return GLib.Variant("b", True)
        return GLib.Variant("i", 0)

    def call(self, item, interface, method, parameters):
        return self.bus.call_sync(item[0], item[1], interface, method, parameters, None, Gio.DBusCallFlags.NONE, 2000, None).unpack()

    def menu(self, item):
        path = self.call(item, "org.freedesktop.DBus.Properties", "Get", GLib.Variant("(ss)", ("org.kde.StatusNotifierItem", "Menu")))[0]
        menu = (item[0], path)
        layout = self.call(menu, "com.canonical.dbusmenu", "GetLayout", GLib.Variant("(iias)", (0, -1, [])))[1]
        nodes = []

        def visit(node):
            if hasattr(node, "unpack"):
                node = node.unpack()
            nodes.append((node[0], node[1]))
            for child in node[2]:
                visit(child)
        visit(layout)
        return menu, nodes

    def click(self, item, label):
        menu, nodes = self.menu(item)
        identifier = next(identifier for identifier, properties in nodes if properties.get("label") == label)
        self.call(menu, "com.canonical.dbusmenu", "Event", GLib.Variant("(isvu)", (identifier, "clicked", GLib.Variant("s", ""), 0)))


def metrics(pid):
    stat = Path(f"/proc/{pid}/stat").read_text().split(") ", 1)[1].split()
    ticks = int(stat[11]) + int(stat[12])
    rss = next(int(line.split()[1]) for line in Path(f"/proc/{pid}/status").read_text().splitlines() if line.startswith("VmRSS:"))
    return ticks, rss / 1024


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--screenshot", type=Path, help="Save the isolated Tools window as a user-guide screenshot")
    parser.add_argument("--tools", type=Path, default=Path("target/release/arcade-tools"))
    parser.add_argument("--link", type=Path, default=Path("target/link-cli/debug/arcade-link"))
    parser.add_argument("--seconds", type=int, default=30)
    parser.add_argument("--check-budgets", action="store_true", help="Fail on the 40 MiB RSS and zero-CPU-ticks performance targets")
    parser.add_argument("--expect-login", action="store_true", help="Check the generated login entry of an installed/release build")
    parser.add_argument("--check-ui", action="store_true", help="Verify debug GUI reports and live tray/hotkey settings through native clicks")
    args = parser.parse_args()
    tools, link = args.tools.resolve(), args.link.resolve()
    state = Path(os.environ["ARCADE_HOME"]) / ".local/share/arcade-tools"
    state.mkdir(parents=True, exist_ok=True)
    smoke = Path(os.environ["ARCADE_HOME"]) / "tray-ui.jsonl"
    if args.check_ui:
        os.environ["ARCADE_TOOLS_SMOKE_LOG"] = str(smoke)
    (state / "tray-preferences.json").write_text(json.dumps({"hostIcons": True, "startAtLogin": args.expect_login, "shortcutsEnabled": True}))
    fixture = ROOT / "find-mock.json"
    fixture.write_text(json.dumps({"version": "1", "methods": ["app.menu", "app.menu.invoke", "app.settings", "app.restart"],
                                   "menu": [{"id": "private", "title": "Private mode", "kind": "toggle", "enabled": True, "checked": True}]}))
    logs = (ROOT / "tray-tools.log").open("w+")
    children = []
    host = Watcher()

    def start(command, **kwargs):
        process = subprocess.Popen(command, stderr=logs, **kwargs)
        children.append(process)
        return process

    def status(app):
        result = subprocess.run([str(link), "status", app], capture_output=True, text=True, timeout=3)
        return json.loads(result.stdout) if result.returncode == 0 else None

    def receive(lines, expected, timeout=2):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            line, arrived = lines.get(timeout=max(0.01, end - time.monotonic()))
            if line == expected:
                return arrived
        raise AssertionError(f"Watcher did not report {expected}")

    def report():
        latest = {}
        if smoke.exists():
            for line in smoke.read_text().splitlines():
                try:
                    record = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if record.get("event") == "rendered":
                    latest = record["report"]
        return latest

    def click_control(identifier=None, label=None):
        for _ in range(8):
            control = wait(lambda: next((control for control in report().get("controls", [])
                if control["width"] > 0 and ((identifier and control.get("id") == identifier)
                or (label and control.get("label") == label))), None))
            if 0 <= control["y"] <= 985:
                subprocess.run(["xdotool", "mousemove", "--window", str(window()), str(int(control["x"] + control["width"] / 2)), str(int(control["y"] + control["height"] / 2)), "click", "1"], check=True)
                return
            direction = "5" if control["y"] > 985 else "4"
            subprocess.run(["xdotool", "mousemove", "--window", str(window()), "900", "500", "click", "--repeat", "4", "--delay", "50", direction], check=True)
        raise AssertionError("Could not bring the isolated GUI control into view")

    try:
        mock = start([str(link), "mock", "--as", "find", "--actions", str(fixture)], stdout=subprocess.DEVNULL)
        wait(lambda: status("find"))
        watcher = start([str(link), "tray", "find"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
        lines = queue.Queue()
        def read():
            for line in watcher.stdout:
                lines.put((line.strip(), time.monotonic()))
        threading.Thread(target=read, daemon=True).start()
        receive(lines, "own")
        process = start([str(tools), "--background"], stdout=subprocess.DEVNULL)
        wait(lambda: status("tools"))
        receive(lines, "hosted")
        assert window() is None, "Background startup created a webview"
        print(f"Background startup RSS={metrics(process.pid)[1]:.3f} MiB", flush=True)
        index = Path(os.environ["ARCADE_HOME"]) / "cache/arcade/tools/shortcuts-index.json"
        wait(lambda: index.exists())
        assert json.loads(index.read_text())["schema"] == 1
        print("PASS: mock peer goes own → hosted; resident startup has no window; index published", flush=True)
        if args.expect_login:
            login = Path(os.environ["ARCADE_HOME"]) / "config/autostart/arcade-tools.desktop"
            wait(login.exists)
            entry = login.read_text()
            assert "Arcade Tools managed login entry" in entry and str(tools) in entry
            assert "--background" in entry
            print("PASS: start at login points to this build inside the disposable profile", flush=True)

        host.start()
        item = wait(lambda: host.items and host.items[-1])
        count = len(host.items)
        host.stop()
        time.sleep(0.2)
        host.start()
        wait(lambda: len(host.items) > count)
        menu, nodes = host.menu(item)
        assert any(p.get("label") == "Arcade Find" for _, p in nodes)
        host.click(item, "Private mode")
        manifest = Path(os.environ["ARCADE_HOME"]) / "apps/arcade.find.json"
        wait(lambda: json.loads(manifest.read_text())["menu"][0]["checked"] is False)
        wait(lambda: any(p.get("label") == "Private mode" and p.get("toggle-state") == 0 for _, p in host.menu(item)[1]))
        print("PASS: late SNI host and re-registration; app toggle updates cached menu", flush=True)

        mock.kill()
        mock.wait()
        wait(lambda: any("(not running)" in p.get("label", "") for _, p in host.menu(item)[1]))
        host.click(item, "Start Arcade Find")
        wait(lambda: status("find"))
        print("PASS: Start launches stopped peer in background and activates it", flush=True)

        overlay = tools.with_name("arcade-tools-overlay")
        if overlay.is_file():
            # Exercise the real X11 registration, including the packaged-launcher
            # path, without registering anything on the user's display.
            subprocess.run(["xdotool", "key", "--clearmodifiers", "super+slash"], check=True)
            overlay_window = wait(lambda: subprocess.run(["xdotool", "search", "--onlyvisible", "--name", "^Arcade Tools Shortcuts$"], capture_output=True, text=True).stdout.strip())
            assert overlay_window
            subprocess.run([str(overlay), "--hide"], check=True, timeout=3)
            wait(lambda: not subprocess.run(["xdotool", "search", "--onlyvisible", "--name", "^Arcade Tools Shortcuts$"], capture_output=True, text=True).stdout.strip())
            subprocess.run([str(tools), "--show-shortcuts"], check=True, timeout=3)
            wait(lambda: subprocess.run(["xdotool", "search", "--onlyvisible", "--name", "^Arcade Tools Shortcuts$"], capture_output=True, text=True).stdout.strip())
            subprocess.run([str(overlay), "--hide"], check=True, timeout=3)
            print("PASS: Super+/ and --show-shortcuts launch the companion overlay", flush=True)

        host.call(item, "org.kde.StatusNotifierItem", "Activate", GLib.Variant("(ii)", (0, 0)))
        identifier = wait(window)
        if args.check_ui:
            subprocess.run(["xdotool", "windowsize", identifier, "940", "1020"], check=True)
            wait(lambda: len(report().get("apps", [])) == 7)
            click_control(identifier="tab-settings")
            wait(lambda: report().get("tab") == "settings")
            click_control(label="Host Arcade app icons")
            receive(lines, "own")
            click_control(label="Host Arcade app icons")
            receive(lines, "hosted")
            click_control(label="Keep its own icon: Arcade Find")
            receive(lines, "own")
            click_control(label="Keep its own icon: Arcade Find")
            receive(lines, "hosted")
            click_control(identifier="overlay-shortcut")
            subprocess.run(["xdotool", "key", "--clearmodifiers", "ctrl+a", "type", "--clearmodifiers", "--delay", "1", "Ctrl+Alt+/"], check=True)
            click_control(label="Apply")
            wait(lambda: json.loads((state / "tray-preferences.json").read_text()).get("shortcut") == "Ctrl+Alt+/")
            subprocess.run(["xdotool", "key", "--clearmodifiers", "ctrl+alt+slash"], check=True)
            wait(lambda: subprocess.run(["xdotool", "search", "--onlyvisible", "--name", "^Arcade Tools Shortcuts$"], capture_output=True, text=True).stdout.strip())
            subprocess.run([str(overlay), "--hide"], check=True, timeout=3)
            print("PASS: seven-app GUI renders; live hosting/exclusion and hotkey rebind work through Settings", flush=True)
        if args.screenshot:
            args.screenshot.parent.mkdir(parents=True, exist_ok=True)
            subprocess.run(["import", "-window", identifier, str(args.screenshot.resolve())], check=True, timeout=15)
        time.sleep(1)
        close_window(identifier)
        wait(lambda: window() is None)
        time.sleep(3)
        before_ticks, before_rss = metrics(process.pid)
        time.sleep(args.seconds)
        after_ticks, after_rss = metrics(process.pid)
        ticks = after_ticks - before_ticks
        print(f"Resident after window close: RSS={after_rss:.3f} MiB (start {before_rss:.3f}); CPU ticks={ticks} over {args.seconds}s", flush=True)
        if after_rss > 40:
            mappings = Path(f"/proc/{process.pid}/smaps").read_text().splitlines()
            resident = []
            name = ""
            for line in mappings:
                if line and line[0] in "0123456789abcdef" and "-" in line.split()[0]:
                    name = line
                if line.startswith("Rss:"):
                    resident.append((int(line.split()[1]), name))
            print("Largest resident mappings:", sorted(resident, reverse=True)[:12], flush=True)
        started = time.monotonic()
        host.call(item, "org.kde.StatusNotifierItem", "Activate", GLib.Variant("(ii)", (0, 0)))
        identifier = wait(window)
        print(f"PASS: warm Open recreates window in {(time.monotonic()-started)*1000:.3f} ms", flush=True)
        duplicate = start([str(tools)], stdout=subprocess.DEVNULL)
        assert duplicate.wait(timeout=5) == 0
        assert process.poll() is None and window() == identifier
        print("PASS: a second foreground launch activates the resident instance", flush=True)
        close_window(identifier)
        wait(lambda: window() is None)
        killed = time.monotonic()
        process.kill()
        process.wait()
        own = receive(lines, "own", timeout=1)
        elapsed = own - killed
        assert elapsed < 1
        assert status("find"), "Mock app depended on Tools staying alive"
        print(f"PASS: Tools killed → own tray in {elapsed*1000:.3f} ms; peer remains running", flush=True)
        # With Link disabled the native single-instance channel still opens and
        # quits Tools. No endpoint or peer lifecycle depends on the webview.
        (state / "preferences.json").write_text(json.dumps({"linkEnabled": False}))
        process = start([str(tools), "--background"], stdout=subprocess.DEVNULL)
        wait(lambda: len(host.items) > count + 1)
        duplicate = start([str(tools)], stdout=subprocess.DEVNULL)
        assert duplicate.wait(timeout=5) == 0
        wait(window)
        assert process.poll() is None
        subprocess.run([str(tools), "--quit"], check=True, timeout=5)
        assert process.wait(timeout=5) == 0
        print("PASS: Link-off native activation and explicit quit", flush=True)
        if args.check_budgets:
            assert after_rss <= 40, f"Resident RSS {after_rss:.3f} MiB exceeds 40 MiB"
            assert ticks == 0, f"Resident consumed {ticks} CPU ticks"
        elif after_rss > 40 or ticks:
            print(f"BUDGET NOT MET: RSS={after_rss:.3f}/40 MiB, CPU ticks={ticks}/0. Use --check-budgets for the performance gate.", flush=True)
    finally:
        host.stop()
        host.loop.quit()
        for child in children:
            if child.poll() is None:
                child.terminate()
        for child in children:
            try:
                child.wait(timeout=2)
            except subprocess.TimeoutExpired:
                child.kill()
        subprocess.run([str(link), "quit", "find"], capture_output=True, timeout=3)
        logs.seek(0)
        contents = logs.read()
        if contents:
            print(contents[-5000:], flush=True)
        logs.close()


if __name__ == "__main__":
    main()
