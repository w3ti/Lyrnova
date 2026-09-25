#!/usr/bin/env python3
"""Exercise the built UI in WebKitGTK with synthetic Tauri IPC (no user repository).

Run after `npm run build --prefix ui` in a graphical Linux session:
    python3 ui/tests/git-review-smoke.py
Requires PyGObject, GTK 3 and WebKitGTK 4.1. Writes a preview to /tmp.
"""
import functools
import http.server
import json
from pathlib import Path
import sys
import threading

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import GLib, Gtk, WebKit2

ROOT = Path(__file__).resolve().parents[1]
if not (ROOT / "dist/index.html").is_file():
    sys.exit("Build the frontend first: npm run build --prefix ui")


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass


server = http.server.ThreadingHTTPServer(
    ("127.0.0.1", 0), functools.partial(QuietHandler, directory=str(ROOT / "dist"))
)
threading.Thread(target=server.serve_forever, daemon=True).start()
result = {"ok": False}
manager = WebKit2.UserContentManager()
manager.register_script_message_handler("testResult")
manager.add_script(WebKit2.UserScript.new(
    (ROOT / "tests/git-review-smoke.js").read_text(),
    WebKit2.UserContentInjectedFrames.TOP_FRAME,
    WebKit2.UserScriptInjectionTime.START, None, None,
))
view = WebKit2.WebView.new_with_user_content_manager(manager)
window = Gtk.Window(title="Lyrnova — teste de revisão Git")
window.set_default_size(1440, 900)
window.add(view)


def on_result(_manager, message):
    value = json.loads(message.get_js_value().to_string())
    if value.get("snapshot"):
        def snapshot_ready(webview, task, _data):
            surface = webview.get_snapshot_finish(task)
            surface.write_to_png("/tmp/lyrnova-git-review.png")
        view.get_snapshot(WebKit2.SnapshotRegion.VISIBLE, WebKit2.SnapshotOptions.NONE,
                          None, snapshot_ready, None)
        return
    result.update(value)
    GLib.idle_add(Gtk.main_quit)


manager.connect("script-message-received::testResult", on_result)
window.connect("destroy", lambda *_: Gtk.main_quit() if Gtk.main_level() else None)
GLib.timeout_add_seconds(45, lambda: (Gtk.main_quit(), False)[1])
window.show_all()
view.load_uri(f"http://127.0.0.1:{server.server_port}/")
Gtk.main()
server.shutdown()
window.destroy()
print(json.dumps(result, ensure_ascii=False))
sys.exit(0 if result["ok"] else 1)
