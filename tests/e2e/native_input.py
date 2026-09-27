#!/usr/bin/env python3
"""OS keyboard/mouse and GTK choosers on a disposable X11 display.

WebDriver observes DOM state/geometry only. XTest supplies all UI input.
Use --xvfb /path/to/Xvfb or --isolated-display inside xvfb-run.
"""
import argparse
import base64
import ctypes as C
import ctypes.util
import hashlib
import json
import os
from pathlib import Path
import platform
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import traceback

from native import Browser, ROOT, free_port, wait_for


class Checks(list):
    def append(self, message):
        super().append(message)
        print("PASS: " + message, flush=True)


class WindowAttributes(C.Structure):
    _fields_ = (
        [(name, C.c_int) for name in ("x", "y", "width", "height", "border_width", "depth")]
        + [("visual", C.c_void_p), ("root", C.c_ulong)]
        + [(name, C.c_int) for name in ("window_class", "bit_gravity", "win_gravity", "backing_store")]
        + [("backing_planes", C.c_ulong), ("backing_pixel", C.c_ulong), ("save_under", C.c_int), ("colormap", C.c_ulong), ("map_installed", C.c_int), ("map_state", C.c_int)]
        + [(name, C.c_long) for name in ("all_event_masks", "your_event_mask", "do_not_propagate_mask")]
        + [("override_redirect", C.c_int), ("screen", C.c_void_p)]
    )


class Input:
    def __init__(self, browser, bookmarks):
        self.browser = browser
        self.bookmarks = bookmarks
        self.x = C.CDLL(ctypes.util.find_library("X11"))
        self.xt = C.CDLL(ctypes.util.find_library("Xtst"))
        pointer, window, integer = C.c_void_p, C.c_ulong, C.c_int

        def bind(lib, name, result, *args):
            fn = getattr(lib, name)
            fn.restype, fn.argtypes = result, list(args)

        bind(self.x, "XOpenDisplay", pointer, C.c_char_p)
        bind(self.x, "XCloseDisplay", integer, pointer)
        bind(self.x, "XDefaultRootWindow", window, pointer)
        bind(self.x, "XQueryTree", integer, pointer, window, C.POINTER(window), C.POINTER(window), C.POINTER(C.POINTER(window)), C.POINTER(C.c_uint))
        bind(self.x, "XFetchName", integer, pointer, window, C.POINTER(C.c_void_p))
        bind(self.x, "XGetWindowAttributes", integer, pointer, window, C.POINTER(WindowAttributes))
        bind(self.x, "XFree", integer, pointer)
        bind(self.x, "XTranslateCoordinates", integer, pointer, window, window, integer, integer, C.POINTER(integer), C.POINTER(integer), C.POINTER(window))
        bind(self.x, "XRaiseWindow", integer, pointer, window)
        bind(self.x, "XSetInputFocus", integer, pointer, window, integer, window)
        bind(self.x, "XStringToKeysym", window, C.c_char_p)
        bind(self.x, "XKeysymToKeycode", C.c_ubyte, pointer, window)
        bind(self.x, "XKeycodeToKeysym", window, pointer, C.c_ubyte, integer)
        bind(self.x, "XSync", integer, pointer, integer)
        bind(self.xt, "XTestQueryExtension", integer, pointer, *([C.POINTER(integer)] * 4))
        bind(self.xt, "XTestFakeKeyEvent", integer, pointer, C.c_uint, integer, window)
        bind(self.xt, "XTestFakeButtonEvent", integer, pointer, C.c_uint, integer, window)
        bind(self.xt, "XTestFakeMotionEvent", integer, pointer, integer, integer, integer, window)
        self.display = self.x.XOpenDisplay(None)
        if not self.display:
            raise RuntimeError("Cannot open the isolated X11 display")
        values = [integer() for _ in range(4)]
        if not self.xt.XTestQueryExtension(self.display, *[C.byref(v) for v in values]):
            raise RuntimeError("Display has no XTest extension")
        self.root = self.x.XDefaultRootWindow(self.display)
        self.app = None

    def windows(self, parent=None):
        root, ancestor, children, count = C.c_ulong(), C.c_ulong(), C.POINTER(C.c_ulong)(), C.c_uint()
        self.x.XQueryTree(self.display, parent or self.root, C.byref(root), C.byref(ancestor), C.byref(children), C.byref(count))
        ids = list(children[:count.value])
        if children:
            self.x.XFree(children)
        found = []
        for wid in ids:
            attributes = WindowAttributes()
            if not self.x.XGetWindowAttributes(self.display, wid, C.byref(attributes)) or attributes.map_state != 2:
                continue
            name = C.c_void_p()
            self.x.XFetchName(self.display, wid, C.byref(name))
            if name.value:
                found.append((wid, C.string_at(name).decode(errors="replace")))
                self.x.XFree(name)
            found.extend(self.windows(wid))
        return found

    def window(self, title):
        return next((wid for wid, name in self.windows() if name == title), None)

    def activate(self, wid):
        self.x.XRaiseWindow(self.display, wid)
        self.x.XSetInputFocus(self.display, wid, 1, 0)
        self.sync()

    def sync(self):
        self.x.XSync(self.display, 0)
        time.sleep(0.08)

    def key(self, key, *modifiers):
        def code(name):
            symbol = ord(name) if len(name) == 1 else self.x.XStringToKeysym(name.encode())
            result = self.x.XKeysymToKeycode(self.display, symbol)
            if not result:
                raise ValueError(f"Key unavailable: {name}")
            return result, symbol
        keycode, symbol = code(key)
        mods = list(modifiers)
        if self.x.XKeycodeToKeysym(self.display, keycode, 0) != symbol:
            if self.x.XKeycodeToKeysym(self.display, keycode, 1) != symbol:
                raise ValueError(f"Key outside the supported keyboard levels: {key}")
            mods.append("Shift_L")
        codes = [code(mod)[0] for mod in dict.fromkeys(mods)]
        try:
            for mod in codes:
                self.xt.XTestFakeKeyEvent(self.display, mod, 1, 0)
            self.xt.XTestFakeKeyEvent(self.display, keycode, 1, 0)
            self.xt.XTestFakeKeyEvent(self.display, keycode, 0, 0)
        finally:
            for mod in reversed(codes):
                self.xt.XTestFakeKeyEvent(self.display, mod, 0, 0)
            self.x.XSync(self.display, 0)
        time.sleep(0.012)

    def type(self, text):
        for char in text:
            self.key("Return" if char == "\n" else char)
        self.sync()

    def click(self, selector):
        self.activate(self.app)
        previous = None
        def stable():
            nonlocal previous
            rect = self.browser.script("""
                const n = [...document.querySelectorAll(arguments[0])].find(n => n.getClientRects().length && !n.disabled);
                if (!n) return null;
                const r = n.getBoundingClientRect();
                const x = r.x + r.width / 2, y = r.y + r.height / 2;
                if (!n.contains(document.elementFromPoint(x, y))) return null;
                return {x, y, scale: devicePixelRatio};
            """, selector)
            settled = rect is not None and rect == previous
            previous = rect
            return rect if settled else None
        rect = wait_for(stable, "visible, stable control: " + selector)
        x, y, child = C.c_int(), C.c_int(), C.c_ulong()
        self.x.XTranslateCoordinates(self.display, self.app, self.root, 0, 0, C.byref(x), C.byref(y), C.byref(child))
        self.xt.XTestFakeMotionEvent(self.display, -1, round(x.value + rect["x"] * rect["scale"]), round(y.value + rect["y"] * rect["scale"]), 0)
        self.xt.XTestFakeButtonEvent(self.display, 1, 1, 0)
        self.xt.XTestFakeButtonEvent(self.display, 1, 0, 0)
        self.sync()

    def chooser(self, title, path=None):
        print(f"Chooser: {title} ({'cancel' if path is None else 'accept'})", flush=True)
        wid = wait_for(lambda: self.window(title), title)
        assert self.browser.script("return document.readyState") == "complete", "WebView stays responsive while GTK dialog is open"
        self.activate(wid)
        if path is None:
            self.key("Escape")
        else:
            # GTK bookmarks belong to this disposable profile. Selection still
            # goes through the real chooser and OS keyboard events.
            folder = path if path.is_dir() else path.parent
            self.key(str(self.bookmarks.index(folder) + 1), "Alt_L")
            time.sleep(0.5)
            if path.is_file():
                self.key("l", "Control_L")
                self.sync()
                self.key("a", "Control_L")
                self.type(str(path))
                time.sleep(0.5)
            self.key("Return")
            # Folder selection navigates to the typed directory first. File
            # selection may accept immediately. Wait for GTK to finish loading.
            time.sleep(0.8)
            if self.window(title):
                self.key("Return")
        wait_for(lambda: not self.window(title), f"{title} closed")
        self.activate(self.app)


def exercise(browser, inputs, root, config, package, report):
    browser.until('!!document.querySelector(\'[data-action="create-project"]\')', "welcome ready")
    inputs.app = wait_for(lambda: inputs.window("Lyrnova"), "app X11 window")
    inputs.activate(inputs.app)
    inputs.key("o", "Control_L")
    inputs.chooser("Abrir projeto no Lyrnova")
    assert not (config / "projects.json").exists()
    report["checks"].append("Ctrl+O opens real GTK folder chooser; Escape preserves empty profile")

    inputs.click('[data-action="create-project"]')
    browser.until('document.querySelector("#project-dialog").open', "create dialog")
    inputs.type("native-project")
    inputs.key("Tab")
    assert browser.script('return document.activeElement.id') == "new-project-git"
    inputs.key("Tab")
    inputs.key("Tab")
    inputs.key("Return")
    inputs.chooser("Escolher pasta para o novo projeto")
    browser.until('!document.querySelector("#project-form button[type=submit]").disabled', "cancel releases create form")
    assert not (root / "native-project").exists()
    inputs.click('#project-form button[type="submit"]')
    inputs.chooser("Escolher pasta para o novo projeto", root)
    workspace = root / "native-project"
    browser.until('document.querySelector("#project-name").textContent === "native-project"', "created project")
    browser.until('!document.querySelector("#project-dialog").open', "creation closes modal")
    assert (workspace / ".git").is_dir()
    assert (workspace / "README.md").is_file()
    report["checks"].append("Tab/Enter navigates create form; folder cancel retries; creation closes modal and initializes Git")

    inputs.click('[data-file="README.md"]')
    browser.until('!!document.querySelector("#source-editor textarea.inputarea")', "Monaco ready")
    inputs.click('#source-editor .view-lines')
    inputs.key("a", "Control_L")
    content = "native keyboard 917\n"
    inputs.type(content)
    browser.until('document.querySelector("#editor-workspace").dataset.saveState === "dirty"', "native typing changes draft")
    inputs.key("s", "Control_L")
    wait_for(lambda: (workspace / "README.md").read_text() == content, "Ctrl+S writes exact bytes")
    inputs.type("unsaveddraft")
    inputs.key("z", "Control_L")
    browser.until('document.querySelector("#editor-workspace").dataset.saveState === "clean"', "Ctrl+Z restores saved contents")
    inputs.type("preserved draft")
    inputs.key("k", "Control_L", "Shift_L")
    browser.until('!document.querySelector("#source-editor .view-lines").textContent.includes("preserved")', "Ctrl+Shift+K remains Monaco delete-line")
    assert browser.script('return document.querySelector("#command-palette").hidden')
    inputs.key("z", "Control_L")
    browser.until('document.querySelector("#source-editor .view-lines").textContent.includes("preserved")', "undo restores deleted line")
    inputs.key("k", "Control_L")
    browser.until('!document.querySelector("#command-palette").hidden', "Ctrl+K opens palette")
    inputs.key("Escape")
    browser.until('document.querySelector("#command-palette").hidden', "Escape closes palette")
    assert (workspace / "README.md").read_text() == content
    report["checks"].append("mouse opens Monaco; OS typing, Ctrl+S, Ctrl+Z and Ctrl+K/Escape work without implicit save")

    inputs.click('#terminal-output .xterm-screen')
    inputs.type("printf native-terminal > native-terminal.txt\n")
    wait_for(lambda: (workspace / "native-terminal.txt").exists(), "native terminal input")
    assert (workspace / "native-terminal.txt").read_text() == "native-terminal"
    inputs.type("sleep 30\n")
    inputs.key("c", "Control_L")
    inputs.type("printf interrupted > interrupt.txt\n")
    wait_for(lambda: (workspace / "interrupt.txt").exists(), "Ctrl+C returns terminal prompt")
    inputs.key("d", "Control_L")
    browser.until('document.querySelector("#terminal-output").dataset.state === "exited"', "Ctrl+D closes shell")
    report["checks"].append("xterm receives OS keyboard input; Ctrl+C interrupts foreground command; Ctrl+D exits shell")

    other = root / "other project"
    inputs.click('#source-editor .view-lines')
    inputs.key("o", "Control_L")
    inputs.chooser("Abrir projeto no Lyrnova", other)
    browser.until('document.querySelector("#project-name").textContent === "other project"', "open project with spaces")
    inputs.key("o", "Control_L")
    inputs.chooser("Abrir projeto no Lyrnova", workspace)
    browser.until('document.querySelector("#source-editor .view-lines")?.textContent.includes("preserved")', "draft restored after project switch")
    assert (workspace / "README.md").read_text() == content
    report["checks"].append("real folder chooser switches projects with spaces and restores unsaved draft on return")

    inputs.click('[data-activity="settings"]')
    inputs.click('[data-settings-target="settings-plugins"]')
    def not_installed():
        state_path = config / "plugins.json"
        state = json.loads(state_path.read_text()) if state_path.exists() else {}
        plugin_id = "io.github.w3ti.lyrnova.tool.e2e"
        assert plugin_id not in state.get("installed", {}), "Cancelled review installed the fixture"
        assert plugin_id not in state.get("enabled", []), "Cancelled review enabled the fixture"
        assert plugin_id not in state.get("grants", {}), "Cancelled review granted fixture permissions"
    inputs.click('[data-action="select-plugin-package"]')
    inputs.chooser("Selecionar pacote de plugin do Lyrnova")
    browser.until('document.querySelector("#plugin-install-status").textContent.includes("cancelada")', "cancel plugin file chooser")
    not_installed()
    inputs.click('[data-action="select-plugin-package"]')
    inputs.chooser("Selecionar pacote de plugin do Lyrnova", package)
    browser.until('document.querySelector("#plugin-review-dialog").open', "real package staged for review")
    inputs.click('#plugin-review-dialog [data-action="cancel-plugin-review"]')
    browser.until('!document.querySelector("#plugin-review-dialog").open', "cancel review")
    not_installed()
    inputs.click('[data-action="select-plugin-package"]')
    inputs.chooser("Selecionar pacote de plugin do Lyrnova", package)
    browser.until('document.querySelector("#plugin-review-dialog").open', "review again")
    permissions = browser.script('return document.querySelectorAll("#plugin-permission-list input").length')
    assert permissions == 3
    inputs.click('#plugin-permission-list label:first-child input')
    assert browser.script('return document.querySelector("#plugin-review-confirm").disabled')
    not_installed()
    inputs.click('#plugin-permission-list label:first-child input')
    inputs.click('#plugin-review-confirm')
    browser.until('!document.querySelector("#plugin-review-dialog").open', "install after explicit grants")
    state = json.loads((config / "plugins.json").read_text())
    assert state["installed"]["io.github.w3ti.lyrnova.tool.e2e"] == "0.1.0"
    assert "io.github.w3ti.lyrnova.tool.e2e" not in state["enabled"]
    report["checks"].append("native package chooser cancel/accept; review cancellation and mandatory grants; installation remains disabled")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    display = parser.add_mutually_exclusive_group(required=True)
    display.add_argument("--xvfb", help="Start this Xvfb binary on a private display and D-Bus")
    display.add_argument("--isolated-display", action="store_true", help="Caller guarantees DISPLAY is disposable (e.g. xvfb-run)")
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/lyrnova")
    parser.add_argument("--fixture", type=Path, default=ROOT / "target/debug/examples/e2e_fixture")
    parser.add_argument("--driver", default="tauri-driver")
    parser.add_argument("--native-driver", default="WebKitWebDriver")
    parser.add_argument("--output", type=Path, default=ROOT / "target/e2e-input")
    args = parser.parse_args()
    if args.xvfb:
        with tempfile.TemporaryDirectory(prefix="lyrnova-display-", ignore_cleanup_errors=True) as display_root:
            env = os.environ.copy()
            env.update(XDG_RUNTIME_DIR=display_root, NO_AT_BRIDGE="1", GTK_USE_PORTAL="0", XDG_CURRENT_DESKTOP="X-Generic", GIO_USE_VFS="local")
            env.update(XDG_CONFIG_HOME=display_root + "/config", XDG_DATA_HOME=display_root + "/data", XDG_CACHE_HOME=display_root + "/cache", GSETTINGS_BACKEND="memory")
            read_fd, write_fd = os.pipe()
            server = subprocess.Popen([args.xvfb, "-displayfd", str(write_fd), "-screen", "0", "1440x1000x24", "-nolisten", "tcp"], pass_fds=(write_fd,), env=env)
            os.close(write_fd)
            try:
                if not select.select([read_fd], [], [], 15)[0]:
                    raise RuntimeError("Xvfb did not publish a display")
                display_number = os.read(read_fd, 32).decode().strip()
                if not display_number.isdigit():
                    raise RuntimeError("Xvfb failed to start")
                env["DISPLAY"] = ":" + display_number
                forwarded = ["--binary", str(args.binary.resolve()), "--fixture", str(args.fixture.resolve()), "--driver", args.driver, "--native-driver", args.native_driver, "--output", str(args.output.resolve())]
                return subprocess.call(["dbus-run-session", "--", sys.executable, str(Path(__file__).resolve()), "--isolated-display", *forwarded], env=env)
            finally:
                os.close(read_fd)
                server.terminate()
                try:
                    server.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait(timeout=5)
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=True)
    report = {"ok": False, "checks": Checks(), "platform": platform.platform(), "inputMode": "XTest OS events; read-only WebDriver observations", "display": os.environ.get("DISPLAY"), "startedAt": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
    with tempfile.TemporaryDirectory(prefix="lyrnova-input-") as temporary:
        root = Path(temporary)
        packaging = root / "package"
        packaging.mkdir()
        (packaging / ".lyrnova-e2e-fixture").touch()
        subprocess.run([str(args.fixture.resolve()), str(packaging)], check=True)
        package = packaging / "e2e-plugin.tar.zst"
        package.with_name(package.name + ".json").write_text(json.dumps({"asset": package.name, "sha256": hashlib.sha256(package.read_bytes()).hexdigest()}))
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        env.update(XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), XDG_CACHE_HOME=str(root / "cache"), GDK_BACKEND="x11", GDK_SCALE="1", GDK_DPI_SCALE="1", GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_NOSYSTEM="1")
        config = root / "config/io.github.w3ti.lyrnova"
        other = root / "other project"
        other.mkdir()
        (other / "README.md").write_text("other project\n")
        bookmarks = [root, other, packaging, root / "native-project"]
        gtk_config = root / "config/gtk-3.0"
        gtk_config.mkdir(parents=True)
        (gtk_config / "bookmarks").write_text("".join(path.as_uri() + f" Fixture {index}\n" for index, path in enumerate(bookmarks, 1)))
        port, native_port = free_port(), free_port()
        while native_port == port:
            native_port = free_port()
        browser = Browser(f"http://127.0.0.1:{port}", f"http://127.0.0.1:{native_port}")
        inputs = None
        with (args.output / "driver.log").open("w") as log:
            process = subprocess.Popen([args.driver, "--native-driver", shutil.which(args.native_driver) or args.native_driver, "--port", str(port), "--native-port", str(native_port)], cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                def ready():
                    if process.poll() is not None:
                        raise RuntimeError("Driver exited; see driver.log")
                    try:
                        browser.request("GET", "/status")
                        return True
                    except OSError:
                        return False
                wait_for(ready, "WebDriver startup")
                report["capabilities"] = browser.start(args.binary.resolve())
                print("WebDriver session ready", flush=True)
                inputs = Input(browser, bookmarks)
                exercise(browser, inputs, root, config, package, report)
                (args.output / "success.png").write_bytes(base64.b64decode(browser.command("GET", "/screenshot")))
                report["ok"] = True
            except Exception as error:
                report.update(error=str(error) or type(error).__name__, traceback=traceback.format_exc())
                if inputs:
                    report["windows"] = inputs.windows()
                    print(json.dumps({"error": str(error), "windows": report["windows"]}), flush=True)
                    if shutil.which("import"):
                        try:
                            subprocess.run(["import", "-window", "root", str(args.output / "failure-desktop.png")], timeout=5, check=False)
                        except (OSError, subprocess.TimeoutExpired) as capture_error:
                            report["desktopCaptureError"] = str(capture_error)
                    # Unblock WebKit before capturing evidence if a GTK chooser
                    # remained open. This cannot affect the user's display.
                    inputs.key("Escape")
                try:
                    (args.output / "failure.png").write_bytes(base64.b64decode(browser.command("GET", "/screenshot")))
                    (args.output / "failure.html").write_text(browser.script("return document.documentElement.outerHTML"))
                except Exception as capture_error:
                    report["captureError"] = str(capture_error)
            finally:
                try:
                    browser.close()
                except Exception as close_error:
                    report.update(ok=False, closeError=str(close_error))
                if inputs:
                    inputs.x.XCloseDisplay(inputs.display)
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                    process.wait(timeout=5)
                except ProcessLookupError:
                    pass
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)
    (args.output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(report, ensure_ascii=False, indent=2), flush=True)
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
