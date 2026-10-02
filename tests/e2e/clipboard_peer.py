#!/usr/bin/python3
"""External GTK text editor for the isolated OS input journey.

Only initial contents and read-only evidence use files. Clipboard operations
are GTK's normal Ctrl+C/V bindings, triggered by XTest in native_input.py.
"""
import sys
from pathlib import Path

import gi

gi.require_version("Gdk", "3.0")
gi.require_version("Gtk", "3.0")
from gi.repository import Gdk, Gtk


source, evidence, selection = map(Path, sys.argv[1:])
window = Gtk.Window(title="Lyrnova clipboard peer")
window.set_default_size(600, 300)
view = Gtk.TextView()
buffer = view.get_buffer()


def record(buffer):
    evidence.write_text(buffer.get_text(*buffer.get_bounds(), True), encoding="utf-8")


buffer.connect("changed", record)
buffer.set_text(source.read_text(encoding="utf-8"))
clipboard = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)


def observe_clipboard(clipboard, event):
    clipboard.request_text(lambda clipboard, text, data: selection.write_text(text or "", encoding="utf-8"), None)


clipboard.connect("owner-change", observe_clipboard)
window.add(view)
window.connect("destroy", Gtk.main_quit)
window.show_all()
view.grab_focus()
Gtk.main()
