#!/usr/bin/env python3
"""A GTK text window for the paste tests: takes the focus, prints "ready" once it has it (the
text view focused on X11, the window activated on Wayland, where a headless compositor only
sends keyboard focus once a keyboard exists), and writes its whole text to the file named by
the first argument on every change. Needs python3-gi with GTK 3."""

import os
import sys

import gi

gi.require_version("Gdk", "3.0")
gi.require_version("Gtk", "3.0")
from gi.repository import Gdk, GLib, Gtk  # noqa: E402

OUT = sys.argv[1]
window = Gtk.Window(title="quickhop paste target")
window.set_default_size(400, 200)
view = Gtk.TextView()
window.add(view)
window.connect("destroy", Gtk.main_quit)


def changed(buffer):
    start, end = buffer.get_bounds()
    with open(OUT, "w", encoding="utf-8") as out:
        out.write(buffer.get_text(start, end, True))


announced = False


def ready():
    global announced
    if not announced:
        announced = True
        print("ready", flush=True)


def focused(_widget, _event):
    ready()
    return False


WAYLAND = os.environ.get("GDK_BACKEND") == "wayland"


def activated(_window, event):
    if WAYLAND and event.new_window_state & Gdk.WindowState.FOCUSED and window.get_mapped():
        ready()
    return False


def take_focus():
    # Without a window manager (Xvfb) nothing else gives the window the keyboard focus.
    window.get_window().focus(Gdk.CURRENT_TIME)
    view.grab_focus()
    return False


view.get_buffer().connect("changed", changed)
view.connect("focus-in-event", focused)
window.connect("window-state-event", activated)
window.show_all()
view.grab_focus()
GLib.timeout_add(200, take_focus)
Gtk.main()
