#!/usr/bin/env python3
"""Isolated real-PTY cleanup probes. Fault injection exists only in Rust tests."""
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import termios
import time

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target/pty-evidence"
OUT.mkdir(parents=True, exist_ok=True)
result = subprocess.run(
    ["cargo", "test", "--locked", "--bin", "zellij-launchpad-prototype",
     "--no-run", "--message-format=json"], cwd=ROOT, check=True, capture_output=True, text=True,
)
artifacts = [json.loads(line) for line in result.stdout.splitlines() if line.startswith("{")]
test_binary = next(a["executable"] for a in artifacts
                   if a.get("reason") == "compiler-artifact" and a.get("profile", {}).get("test")
                   and a.get("executable"))
checks = []
for mode in ("error", "panic", "mouse-quit"):
    master, slave = pty.openpty()
    original = termios.tcgetattr(slave)
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    command = ([str(ROOT / "target/release/zellij-launchpad-prototype")] if mode == "mouse-quit" else
               [test_binary, "terminal::tests::session_cleanup_probe", "--exact", "--ignored", "--nocapture"])
    def child_setup():
        os.setsid()
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)
    child = subprocess.Popen(command, stdin=slave, stdout=slave, stderr=slave, cwd=ROOT,
                             env={**os.environ, "TERM": "xterm-256color", "LAUNCHPAD_CLEANUP_PROBE": mode},
                             preexec_fn=child_setup)
    raw = bytearray()
    sent = False
    deadline = time.monotonic() + 5
    try:
        while time.monotonic() < deadline:
            ready, _, _ = select.select([master], [], [], 0.05)
            if ready:
                raw.extend(os.read(master, 65536))
            if mode == "mouse-quit" and not sent and b"Ctrl+Q quit" in raw:
                # Footer Quit is right-aligned, at zero-based x=67..76, y=23.
                os.write(master, b"\x1b[<0;69;24M")
                sent = True
            if child.poll() is not None and not ready:
                break
        assert child.wait(timeout=1) == 0, (mode, raw.decode(errors="replace"))
        assert termios.tcgetattr(slave) == original, mode
        for number in (1000, 1002, 1003, 1006, 1015, 1049, 2004):
            assert f"\x1b[?{number}h".encode() in raw, (mode, number, "not enabled")
            assert f"\x1b[?{number}l".encode() in raw, (mode, number, "not disabled")
        if mode != "mouse-quit":
            assert b"1 passed" in raw
        if mode == "panic":
            assert b"intentional cleanup probe" in raw
        checks.append({"mode": mode, "passed": True, "termios_restored": True,
                       "capture_restored": True, "raw_bytes": len(raw)})
    finally:
        (OUT / f"cleanup-{mode}.ansi").write_bytes(raw)
        if child.poll() is None:
            child.kill()
            child.wait(timeout=2)
        os.close(master)
        os.close(slave)
report = {"passed": len(checks), "checks": checks}
(OUT / "cleanup-report.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, indent=2))
