#!/usr/bin/env python3
"""Bounded real-PTY smoke test. Dev-only dependency: pyte==0.8.2.

Run from repository root after `cargo build --release`:
  target/verification-venv/bin/python tools/verify_pty.py
Raw ANSI, decoded screens and replay events go to target/pty-evidence/.
Nothing in this script is a dependency of the release binary.
"""
import codecs
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import signal
import struct
import subprocess
import termios
import time

import pyte

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target/pty-evidence"
OUT.mkdir(parents=True, exist_ok=True)
BINARY = ROOT / "target/release/zellij-launchpad-prototype"
master, slave = pty.openpty()
original = termios.tcgetattr(slave)
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))


def child_setup():
    os.setsid()
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)


process = subprocess.Popen(
    [str(BINARY), "--demo"], stdin=slave, stdout=slave, stderr=slave,
    env={**os.environ, "TERM": "xterm-256color", "COLORTERM": "truecolor"},
    preexec_fn=child_setup, cwd=ROOT,
)
screen = pyte.Screen(80, 24)
stream = pyte.Stream(screen)
decoder = codecs.getincrementaldecoder("utf-8")("replace")
events: list[dict] = [{"resize": [80, 24]}]
raw = bytearray()
checks = []


def pump(seconds=0.14):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        ready, _, _ = select.select([master], [], [], max(0, end - time.monotonic()))
        if ready:
            try:
                data = os.read(master, 65536)
            except OSError:
                break
            if not data:
                break
            raw.extend(data)
            text = decoder.decode(data)
            events.append({"write": text})
            stream.feed(text)


def display():
    return "\n".join(screen.display)


def expect(text, label):
    deadline = time.monotonic() + 3
    while text not in display() and time.monotonic() < deadline:
        pump(0.05)
    assert text in display(), f"{label}: missing {text!r}\n{display()}"
    checks.append(label)


def send(data):
    os.write(master, data.encode() if isinstance(data, str) else data)
    pump()


def snapshot(name):
    (OUT / f"{name}.txt").write_text(display() + "\n")
    (OUT / f"{name}.json").write_text(json.dumps(events, ensure_ascii=False))


def resize(w, h):
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", h, w, 0, 0))
    screen.resize(h, w)
    events.append({"resize": [w, h]})
    os.kill(process.pid, signal.SIGWINCH)
    pump(0.25)


def mouse(x, y, button=0, release=False):
    """SGR coordinates are one-based; callers use terminal cells, zero-based."""
    send(f"\x1b[<{button};{x + 1};{y + 1}{'m' if release else 'M'}")


def locate(label):
    for y, line in enumerate(screen.display):
        if label in line:
            return line.index(label), y
    raise AssertionError(f"missing mouse target {label!r}\n{display()}")


def click(label):
    mouse(*locate(label))


def checked(condition, label):
    assert condition, f"{label}\n{display()}"
    checks.append(label)


def verify_mouse():
    checked(all(f"\x1b[?{mode}h".encode() in raw for mode in (1000, 1002, 1003, 1006)),
            "mouse: capture and SGR reporting enabled")
    send("\x1b[15~")
    send("\x15notes")
    mouse(12, 6)
    expect("Enter launches", "mouse: suggestion accepts only")
    checked("Terminal placeholder" not in display() and "~/Projects/research/notes" in display(),
            "mouse: suggestion did not launch")
    mouse(26, 9)
    expect("[ Codex ]", "mouse: tool selects only")
    before = display()
    for button, release in [(0, True), (2, False), (1, False), (32, False), (35, False), (16, False)]:
        mouse(68, 9, button, release)
    mouse(200, 90)
    mouse(0, 0)
    checked(display() == before, "mouse: release/right/middle/drag/move/modifier/blank/out-of-bounds ignored")
    click("Enter ↵")
    expect("Codex · simulated launch accepted", "mouse: explicit launch")
    click("Esc back")
    expect("Launchpad", "mouse: terminal back")
    mouse(15, 14)
    expect("Tab copy", "mouse: history row selects without launch")
    snapshot("11-mouse-history-80x24")
    click("Tab copy")
    expect("Copied to form", "mouse: history copy")
    mouse(15, 14)
    click("Enter replay")
    expect("Terminal placeholder", "mouse: explicit history replay")
    click("Esc back")
    click("F5 reset")
    send("\x15修理/e\u0301👩🏽‍💻")
    mouse(9, 3)  # continuation cell of 理; insert before the complete grapheme
    send("X")
    # pyte normalizes combining accents to NFC (and does not fully model ZWJ emoji).
    expect("修X理/é", "mouse: Unicode cursor placement respects wide and combining cells")
    # Keep the field scrolled while leaving room for the insertion marker.
    send("\x15" + "a" * 80 + "修理")
    mouse(6, 3)
    send("Z")
    checked("Z" in screen.display[3], "mouse: horizontally scrolled input uses visible origin")
    click("F5 reset")
    click("F1 help")
    mouse(5, 3, 65)
    checked("F1 / Esc  Back" not in display(), "mouse: help wheel scrolls")
    snapshot("12-mouse-help")
    click("Esc back")
    expect("Launchpad", "mouse: help close")
    send("\x15notes")
    mouse(10, 5, 65)
    checked("› ~/Projects/notes" in display(), "mouse: suggestion wheel highlights")
    mouse(10, 5, 65)
    mouse(10, 5, 65)
    mouse(10, 5, 65)
    checked("› ~/Projects/team notes" in display(), "mouse: suggestion wheel clamps at end")
    mouse(10, 7)
    expect("Enter launches", "mouse: wheel-highlighted suggestion click accepts")
    click("F5 reset")
    resize(40, 10)
    for _ in range(12):
        mouse(15, 8, 65)
    expect("literal", "mouse: compact history wheel reaches last entry")
    snapshot("13-mouse-history-40x10")
    before = display()
    mouse(0, 8, 64)
    mouse(5, 2, 64)
    checked(display() == before, "mouse: unrelated wheel does not steal focus")
    click("Tab copy")
    expect("Copied", "mouse: compact history copy")
    click("F5")
    mouse(4, 6)
    expect("[ Copilot ]", "mouse: wrapped tool selection")
    click("Enter ↵")
    expect("Terminal placeholder", "mouse: compact explicit launch")
    click("Esc back")
    click("F5")
    resize(120, 36)
    mouse(26, 12)
    expect("[ Codex ]", "mouse: roomy tool geometry")
    snapshot("14-mouse-roomy-120x36")
    # Queue an old-frame click with SIGWINCH: it must not target the new layout.
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    screen.resize(24, 80)
    events.append({"resize": [80, 24]})
    os.kill(process.pid, signal.SIGWINCH)
    os.write(master, b"\x1b[<0;69;10M")
    pump(0.3)
    checked("Terminal placeholder" not in display(), "mouse: queued click during resize ignored")
    resize(30, 8)
    mouse(10, 3)
    mouse(68, 9)
    mouse(10, 3, 65)
    expect("Resize", "mouse: tiny layout has no active mouse targets")
    resize(80, 24)
    click("F5 reset")
    expect("10 events", "mouse: reset restores fixtures")
    snapshot("15-mouse-final-80x24")


try:
    pump(0.3)
    expect("Launchpad", "startup in real PTY")
    expect("2d ago", "all ten initial events visible at 80x24")
    snapshot("01-initial-80x24")
    send("\x15notes")
    expect("~/Projects/notes/", "bare notes fuzzy search")
    expect("research/notes", "ambiguous nested result has full path")
    snapshot("02-notes-search")
    send("\x1b[B\r")
    expect("Enter launches", "Enter accepts highlighted completion only")
    assert "Terminal placeholder" not in display()
    send("\x14\x1b[C\x1b[C")
    expect("[ Codex ]", "Ctrl+T and tool arrows")
    snapshot("03-tool-selected")
    send("\r")
    expect("Codex · simulated launch accepted", "simulated launch")
    expect("/home/demo/Projects/notes", "completion resolves actual cwd")
    snapshot("04-simulated-terminal")
    send("\r\r")
    expect("Terminal placeholder", "duplicate Enter remains frozen")
    send("\x1b")
    expect("Launchpad", "return from placeholder")
    send("\x12\r")
    expect("Codex · simulated launch accepted", "history replay retains tool")
    send("\x1b")
    send("\x12\t")
    expect("Copied to form", "history Tab copies without launch")
    send("\x10\x15~/restricted\r")
    expect("Permission denied", "invalid directory inline error")
    snapshot("05-permission-error")
    send("\x1b[15~")  # F5 reset
    send("\x1b[17~")  # F6 Copilot unavailable
    send("\x12" + "\x1b[B" * 5 + "\r")
    expect("Copilot is unavailable", "unavailable recent never falls back")
    snapshot("06-unavailable-history")
    send("\t\x14\x1b[C\r")
    expect("Shell · simulated launch accepted", "explicit available replacement")
    send("\x1b")
    send("\x10\x15~/Projects/it's literal; $HOME\r")
    expect("it's literal; $HOME", "shell-looking filename kept literal")
    expect("Terminal placeholder", "literal fixture launch")
    send("\x1b")
    send("\x10\x15~/Projects/修理")
    expect("修理", "Unicode input in real terminal stream")
    snapshot("07-unicode-input")
    send("\x1b[15~")
    resize(120, 36)
    expect("10 events", "expanded 120x36 layout")
    snapshot("08-expanded-120x36")
    resize(40, 12)
    send("\x12\x1b[F")
    expect("literal", "40x12 scroll reaches last history entry")
    snapshot("09-narrow-40x12")
    send("\x1bOP\x1b[F")  # F1 then End
    expect("memory only", "help scroll reaches last line at 40x12")
    send("\x1b")
    resize(30, 8)
    expect("Resize", "compact guard")
    send("\r")
    assert "Terminal placeholder" not in display()
    snapshot("10-compact-30x8")
    resize(80, 24)
    expect("Launchpad", "restore from compact never launched")
    keyboard_count = len(checks) + 1  # includes the original Ctrl+Q cleanup check below
    verify_mouse()
    send("\x11")
    process.wait(timeout=3)
    pump(0.1)
    assert process.returncode == 0
    assert termios.tcgetattr(slave) == original, "raw-mode terminal flags were not restored"
    assert b"\x1b[?1049l" in raw, "alternate screen not restored"
    assert b"\x1b[?2004l" in raw, "bracketed paste not disabled"
    checks.append("Ctrl+Q exits zero and restores termios, alternate screen and bracketed paste")
    checked(all(f"\x1b[?{mode}l".encode() in raw for mode in (1000, 1002, 1003, 1006)),
            "mouse: capture disabled at exit")
    report = {"passed": len(checks), "checks": checks, "exit_code": process.returncode,
              "keyboard_checks": keyboard_count, "mouse_checks": len(checks) - keyboard_count,
              "mouse_capture_restored": True,
              "termios_restored": True, "raw_bytes": len(raw), "binary": str(BINARY)}
    (OUT / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
finally:
    (OUT / "session.ansi").write_bytes(raw)
    (OUT / "session.json").write_text(json.dumps(events, ensure_ascii=False))
    if process.poll() is None:
        process.terminate()
        process.wait(timeout=3)
    os.close(master)
    os.close(slave)
