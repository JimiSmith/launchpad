#!/usr/bin/env python3
"""Isolated native/CLI integration tests. Requires Zellij and development-only pyte."""
import argparse
import codecs
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import shutil
import signal
import struct
import subprocess
import sys
import termios
import tempfile
import time
import uuid

import pyte

ROOT = Path(__file__).resolve().parents[1]
ZELLIJ = shutil.which('zellij')
BINARY = ROOT / 'target/release/zellij-launchpad'


class Screen(pyte.Screen):
    def report_device_status(self, mode, **kwargs):
        if not kwargs.get('private'):
            super().report_device_status(mode)


class Session:
    def __init__(self, floating=False, probe=False, only=False, outside=False):
        self.name = 'native-' + uuid.uuid4().hex[:8]
        self.out = ROOT / 'target' / self.name
        for name in ['home', 'bin', 'cache', 'config', 'data', 'state', 'runtime', 'sock']:
            (self.out / name).mkdir(parents=True, mode=0o700)
        self.home = self.out / 'home'
        self.cwd = self.home / "space 修理 literal"
        if outside:
            self.cwd = self.out / "outside HOME"
        self.cwd.mkdir()
        self.log = self.out / 'executions.jsonl'
        self.socket_dir = tempfile.TemporaryDirectory(prefix='launchpad-')
        self.env = {k: v for k, v in os.environ.items() if not k.startswith('ZELLIJ') and k != 'TMUX'}
        self.env.update(HOME=str(self.home), PATH=str(self.out / 'bin'), SHELL='/bin/false',
                        TERM='xterm-256color', COLORTERM='truecolor',
                        ZELLIJ_SOCKET_DIR=self.socket_dir.name,
                        **{'XDG_' + key + '_HOME': str(self.out / value) for key, value in
                           [('CONFIG', 'config'), ('CACHE', 'cache'), ('DATA', 'data'), ('STATE', 'state')]},
                        XDG_RUNTIME_DIR=str(self.out / 'runtime'))
        (self.out / 'bin/zellij').symlink_to(ZELLIJ)
        (self.out / 'bin/zellij-launchpad').symlink_to(BINARY)
        for name in ['default-shell', 'neighbor', 'fixture']:
            self.fixture(name)
        config = self.out / 'config.kdl'
        config.write_text((ROOT / 'examples/locked.kdl').read_text() +
                          f'\nsession_name "{self.name}"\ndefault_shell "{self.out}/bin/default-shell"\n')
        native_config = self.out / 'config/zellij-launchpad/config.toml'
        native_config.parent.mkdir()
        native_config.write_text('''[[commands]]
id = "fixture"
label = "Fixture"
executable = "fixture"
arguments = ["a b", "", "$HOME", ";", "$(touch NO_EXPANSION)"]
[[commands]]
id = "missing"
executable = "missing"
''')
        command = str(self.out / 'bin/fixture') if probe else str(BINARY)
        pane = f'pane name="launchpad" focus=true command={json.dumps(command)} cwd={json.dumps(str(self.cwd), ensure_ascii=False)}'
        if floating:
            pane += ' width=120 height=30'
        neighbor = f'pane name="neighbor" command="{self.out}/bin/neighbor"'
        layout = (f'layout {{ tab {{ {neighbor}; floating_panes {{ {pane}; }}; }}; }}' if floating
                  else f'layout {{ pane split_direction="vertical" {{ {neighbor}; {pane}; }}; }}')
        if only:
            layout = f'layout {{ {pane}; }}'
        self.master, self.slave = pty.openpty()
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack('HHHH', 36, 160, 0, 0))
        def setup():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)
        self.child = subprocess.Popen([ZELLIJ, '--config', str(config), '--layout-string', layout],
                                     stdin=self.slave, stdout=self.slave, stderr=self.slave,
                                     env=self.env, cwd=self.home, preexec_fn=setup)
        self.screen = Screen(160, 36)
        self.stream = pyte.Stream(self.screen)
        self.decoder = codecs.getincrementaldecoder('utf-8')('replace')
        self.raw = bytearray()
        self.filter_state = 'normal'

    def fixture(self, name):
        path = self.out / 'bin' / name
        path.write_text(f'''#!{Path(sys.executable).resolve()}
import json, os, sys
with open({str(self.log)!r}, 'a') as f:
    f.write(json.dumps(dict(exe=os.path.basename(sys.argv[0]), argv=sys.argv[1:], cwd=os.getcwd(), pid=os.getpid())) + '\\n')
print('FIXTURE_READY', flush=True)
for line in sys.stdin:
    if line.startswith('exit'):
        sys.exit(int(line.split()[1]))
''')
        path.chmod(0o700)

    def cli(self, *args, check=True):
        result = subprocess.run([ZELLIJ, '--session', self.name, *args], env=self.env,
                                cwd=self.home, capture_output=True, text=True, timeout=8)
        if check:
            assert result.returncode == 0, (args, result.stdout, result.stderr)
        return result

    def panes(self):
        return json.loads(self.cli('action', 'list-panes', '--all', '--json').stdout)

    def pump(self, seconds=.1):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if not select.select([self.master], [], [], max(0, end-time.monotonic()))[0]:
                continue
            try:
                data = os.read(self.master, 65536)
            except OSError:
                break
            self.raw.extend(data)
            output = []
            for char in self.decoder.decode(data):
                if self.filter_state == 'normal':
                    if char == '\x1b': self.filter_state = 'escape'
                    else: output.append(char)
                elif self.filter_state == 'escape':
                    if char in 'P_': self.filter_state = 'string'
                    else:
                        output.extend(('\x1b', char))
                        self.filter_state = 'normal'
                elif self.filter_state == 'string':
                    if char == '\x1b': self.filter_state = 'string_escape'
                elif self.filter_state == 'string_escape':
                    self.filter_state = 'normal' if char == '\\' else 'string'
            self.stream.feed(''.join(output))

    def expect(self, text):
        end = time.monotonic() + 12
        while text not in self.display() and time.monotonic() < end:
            self.pump()
        assert text in self.display(), text + '\n' + self.display()

    def display(self):
        return '\n'.join(self.screen.display)

    def send(self, text):
        os.write(self.master, text.encode())
        self.pump(.2)

    def records(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def close(self):
        (self.out / 'screen.txt').write_text(self.display())
        (self.out / 'session.ansi').write_bytes(self.raw)
        self.cli('kill-session', self.name, check=False)
        try:
            self.child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.child.kill()
            self.child.wait(timeout=3)
        os.close(self.master)
        os.close(self.slave)
        self.socket_dir.cleanup()


def probe():
    for floating in [False, True]:
        s = Session(floating=floating, probe=True)
        try:
            s.expect('FIXTURE_READY')
            s.pump(.5)
            panes = s.panes()
            origin = next(p for p in panes if p['title'] == 'launchpad')
            neighbor = next(p for p in panes if p['title'] == 'neighbor')
            pane_id = 'terminal_' + str(origin['id'])
            s.cli('action', 'focus-pane-id', str(neighbor['id']))
            s.cli('action', 'rename-tab', '--tab-id', str(origin['tab_id']), 'Probe')
            result = s.cli('action', 'new-pane', '--in-place', '--close-replaced-pane',
                          '--pane-id', pane_id, '--close-on-exit', '--cwd', str(s.cwd), '--', 'absent', check=False)
            s.pump(.3)
            assert any(p['id'] == origin['id'] for p in s.panes())
            print('missing-command result:', result.returncode, repr(result.stdout), repr(result.stderr))
            result = s.cli('action', 'new-pane', '--in-place', '--close-replaced-pane',
                          '--pane-id', pane_id, '--cwd', str(s.cwd))
            s.pump(.4)
            after = s.panes()
            assert not any(p['id'] == origin['id'] for p in after)
            assert any(p['id'] == neighbor['id'] for p in after)
            replacement = next(p for p in after if p['id'] != neighbor['id'] and not p['is_plugin'])
            assert replacement['is_floating'] == floating
            assert any(r['exe'] == 'default-shell' and r['cwd'] == str(s.cwd) for r in s.records())
            s.cli('action', 'write-chars', '--pane-id', str(replacement['id']), 'exit 17\n')
            s.pump(.4)
            assert [p['id'] for p in s.panes() if not p['is_plugin']] == [neighbor['id']]
            print('PASS CLI replacement', 'floating' if floating else 'tiled', s.out)
        finally:
            s.close()


def native():
    for floating in [False, True]:
        for selected, changed in [('Shell', False), ('Shell', True), ('Fixture', True), ('missing', False)]:
            s = Session(floating=floating)
            try:
                s.expect('HOME indexed')
                origin = next(p for p in s.panes() if p['title'] == 'launchpad')
                neighbor = next(p for p in s.panes() if p['title'] == 'neighbor')
                target = s.cwd
                if changed:
                    target = s.home / "changed 修理 it's literal; $HOME"
                    target.mkdir()
                    s.send('\x10\x15\x1b[200~' + '~/'+ target.name + '\x1b[201~\x14')
                if selected != 'Shell':
                    s.send('\x14' + ('\x1b[C' if selected == 'Fixture' else '\x1b[C\x1b[C'))
                s.cli('action', 'focus-pane-id', str(neighbor['id']))
                s.cli('action', 'write', '--pane-id', str(origin['id']), '13')
                s.pump(.6)
                if selected == 'missing':
                    s.cli('action', 'focus-pane-id', str(origin['id']))
                    s.expect('Zellij did not create')
                    assert any(p['id'] == origin['id'] for p in s.panes())
                    assert not list((s.out / 'state/zellij-launchpad/history.d').glob('*.json'))
                    s.fixture('missing')
                    s.send('\r')
                    s.pump(.6)
                after = s.panes()
                assert not any(p['id'] == origin['id'] for p in after), s.display()
                assert any(p['id'] == neighbor['id'] for p in after)
                launched = next(p for p in after if p['id'] != neighbor['id'] and not p['is_plugin'])
                assert launched['is_floating'] == floating
                assert launched['tab_name'] == target.name + ' · ' + selected, launched
                record = next(r for r in s.records() if r['exe'] == ('default-shell' if selected == 'Shell' else selected.lower()))
                assert record['cwd'] == str(target), record
                if selected == 'Fixture':
                    assert record['argv'] == ['a b', '', '$HOME', ';', '$(touch NO_EXPANSION)'], record
                state = json.loads((s.out / 'state/zellij-launchpad/history.json').read_text())
                assert state['entries'][0]['path'] == str(target)
                assert state['entries'][0]['tool'] == selected.lower()
                s.cli('action', 'write-chars', '--pane-id', str(launched['id']), 'exit ' + ('17' if floating else '0') + '\n')
                s.pump(.4)
                assert [p['id'] for p in s.panes() if not p['is_plugin']] == [neighbor['id']]
                print('PASS native', selected, 'changed-cwd' if changed else 'initial-cwd', 'floating' if floating else 'tiled', s.out, flush=True)
            finally:
                s.close()


def recovery_cases():
    s = Session()
    try:
        s.expect('HOME indexed')
        (s.out / 'bin/default-shell').unlink()
        s.send('\r')
        s.expect('Default shell failed')
        assert not json.loads((s.out / 'state/zellij-launchpad/history.json').read_text())['entries']
        panes = [p for p in s.panes() if not p['is_plugin']]
        assert len(panes) == 2 and all(p['tab_name'] == 'Tab #1' for p in panes)
        s.fixture('default-shell')
        s.send('\r')
        s.expect('FIXTURE_READY')
        s.pump(.5)
        assert any(r['exe'] == 'default-shell' for r in s.records())
        print('PASS default-shell rejection rollback and explicit retry', flush=True)
    finally:
        s.close()
    s = Session()
    try:
        s.expect('HOME indexed')
        origin = next(p for p in s.panes() if p['title'] == 'launchpad')
        s.cwd.rmdir()
        s.send('\r')
        s.expect('Invoking directory unavailable')
        assert any(p['id'] == origin['id'] and not p['is_plugin'] for p in s.panes())
        assert all(r['exe'] == 'neighbor' for r in s.records())
        s.cwd.mkdir()
        s.send('\x1b[15~\r')
        s.pump(.6)
        assert any(r['exe'] == 'default-shell' and r['cwd'] == str(s.cwd) for r in s.records())
        print('PASS deleted invoking cwd and F5 retry', flush=True)
    finally:
        s.close()


def history_and_layout_cases():
    s = Session()
    try:
        s.expect('HOME indexed')
        s.send('\x14\x1b[C\r')
        s.pump(.5)
        launched = next(p for p in s.panes() if not p['is_plugin'] and p['title'] != 'neighbor')
        s.cli('action', 'write-chars', '--pane-id', str(launched['id']), 'exit 0\n')
        s.pump(.3)
        # Exercise the shipped native layout and the same shared state directory.
        s.cli('action', 'new-tab', '--layout', str(ROOT / 'examples/launchpad.kdl'))
        s.expect('HOME indexed')
        s.send('\x12\t')  # copy the remembered path + command from history
        s.expect('~/space')
        s.expect('[ Fixture ]')
        s.send('\x12\x1b[3~')
        s.pump(.2)
        assert not json.loads((s.out / 'state/zellij-launchpad/history.json').read_text())['entries']
        s.cli('action', 'new-tab', '--layout', str(ROOT / 'examples/configured.kdl'))
        s.expect('HOME indexed')
        print('PASS shipped layouts, history reopen/copy/delete', flush=True)
    finally:
        s.close()
    s = Session(only=True, outside=True)
    try:
        s.expect('HOME indexed')
        s.send('\r')
        s.pump(.5)
        assert any(r['exe'] == 'default-shell' and r['cwd'] == str(s.cwd) for r in s.records())
        s.send('exit 17\n')
        deadline = time.monotonic() + 5
        while s.child.poll() is None and time.monotonic() < deadline:
            s.pump(.1)
        assert s.child.poll() is not None, 'last pane exit must end the session'
        print('PASS outside-HOME invoking cwd and last-pane session exit', flush=True)
    finally:
        s.close()


def terminal_cases():
    # Run the real native terminal adapter on a PTY with a read-only fake CLI.
    # No live server or real tool command is reachable in these checks.
    for ending in ['quit', 'signal']:
        out = ROOT / 'target' / ('native-terminal-' + uuid.uuid4().hex[:8])
        for name in ['home/notes', 'bin', 'config', 'state']:
            (out / name).mkdir(parents=True)
        fake = out / 'bin/zellij'
        fake.write_text('''#!/bin/sh
case "$3" in
--version) echo "zellij 0.45.0";;
action) echo '[{"id":7,"is_plugin":false,"tab_id":3,"tab_name":"Original"}]';;
esac
''')
        fake.chmod(0o700)
        env = dict(os.environ, HOME=str(out / 'home'), PATH=str(out / 'bin'),
                   ZELLIJ_SESSION_NAME='fake-native-terminal', ZELLIJ_PANE_ID='7',
                   XDG_CONFIG_HOME=str(out / 'config'), XDG_STATE_HOME=str(out / 'state'),
                   TERM='xterm-256color')
        s = Session.__new__(Session)
        s.out = out
        s.master, s.slave = pty.openpty()
        original = termios.tcgetattr(s.slave)
        fcntl.ioctl(s.slave, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 80, 0, 0))
        s.screen = Screen(80, 24)
        s.screen.write_process_input = lambda text: os.write(s.master, text.encode())
        s.stream = pyte.Stream(s.screen)
        s.decoder = codecs.getincrementaldecoder('utf-8')('replace')
        s.raw = bytearray()
        s.filter_state = 'normal'
        def setup():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)
        child = subprocess.Popen([str(BINARY)], stdin=s.slave, stdout=s.slave, stderr=s.slave,
                                 env=env, cwd=out / 'home', preexec_fn=setup)
        try:
            s.expect('HOME indexed')
            s.send('\x15\x1b[200~notes\x1b[201~')
            s.expect('~/notes/')
            s.send('\x1bOP')  # F1
            s.expect('Quit Launchpad')
            s.send('\x1b')
            s.send('\x1b[15~')  # F5
            s.expect('HOME indexed')
            # Exercise the native backend at compact dimensions and restore it.
            for width, height in [(30, 8), (100, 30)]:
                fcntl.ioctl(s.slave, termios.TIOCSWINSZ, struct.pack('HHHH', height, width, 0, 0))
                s.screen.resize(height, width)
                os.kill(child.pid, signal.SIGWINCH)
                s.pump(.2)
            s.expect('Launchpad')
            # Native mouse coordinates are delivered directly by Crossterm.
            y, line = next((y, line) for y, line in enumerate(s.screen.display) if 'F1 help' in line)
            x = line.index('F1 help')
            s.send(f'\x1b[<0;{x+1};{y+1}M\x1b[<0;{x+1};{y+1}m')
            s.expect('Quit Launchpad')
            if ending == 'quit':
                s.send('\x11')
            else:
                os.kill(child.pid, signal.SIGTERM)
                s.pump(.2)
            assert child.wait(timeout=3) == 0
            s.pump(.1)
            assert termios.tcgetattr(s.slave) == original, 'raw terminal mode was not restored'
            assert b'\x1b[?1049l' in s.raw, 'alternate screen was not left'
            print('PASS terminal input/paste/mouse/resize and cleanup:', ending, flush=True)
        finally:
            (out / 'session.ansi').write_bytes(s.raw)
            (out / 'screen.txt').write_text(s.display())
            if child.poll() is None:
                child.kill()
                child.wait(timeout=3)
            os.close(s.master)
            os.close(s.slave)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--probe', action='store_true')
    args = parser.parse_args()
    if args.probe:
        probe()
    else:
        terminal_cases()
        native()
        recovery_cases()
        history_and_layout_cases()
