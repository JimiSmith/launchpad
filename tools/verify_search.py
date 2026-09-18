#!/usr/bin/env python3
"""Bounded live Zellij 0.45.1 PTY verification; pyte is development-only.
Run: target/verification-venv/bin/python tools/verify_search.py
All host state and evidence live under target/zj-<unique id>. No global cleanup.
"""
import argparse
import codecs
import fcntl
import hashlib
import json
import os
from pathlib import Path
import pty
import select
import shutil
import signal
import struct
import subprocess
import termios
import time
import uuid

parser = argparse.ArgumentParser(description='Verify search using an isolated HOME fixture.')
parser.add_argument('--native', action='store_true')
parser.add_argument('--deny', action='store_true')
# Wrappers validate their own CLI before supplying explicit search options.
options = parser.parse_args(globals().get('SEARCH_ARGS'))

import pyte

ROOT = Path(__file__).resolve().parents[1]
NATIVE = options.native
DENY = options.deny
NAME = 'search-' + uuid.uuid4().hex[:8]
OUT = ROOT / 'target' / ('zj-' + NAME[3:])
OUT.mkdir(parents=True)
WASM = ROOT / 'target/wasm32-wasip1/release/launchpad-plugin.wasm'
assert WASM.is_file(), 'Build the release WASM first'
for directory in ('home', 'cache', 'config', 'data', 'runtime', 'sock', 'tmp'):
    (OUT / directory).mkdir(mode=0o700)
env = {k: v for k, v in os.environ.items() if not k.startswith('ZELLIJ') and k != 'TMUX'}
env.update(TERM='xterm-256color', COLORTERM='truecolor', HOME=str(OUT/'home'),
           XDG_CACHE_HOME=str(OUT/'cache'), XDG_CONFIG_HOME=str(OUT/'config'),
           XDG_DATA_HOME=str(OUT/'data'), XDG_RUNTIME_DIR=str(OUT/'runtime'),
           ZELLIJ_SOCKET_DIR=str(OUT/'sock'), TMPDIR=str(OUT/'tmp'), SHELL='/bin/sh',
           TERM_PROGRAM='launchpad-verification')
home = OUT/'home'
env['HOME'] = str(home)
probe = home / ('launchpad-search-probe-' + uuid.uuid4().hex[:8])
(probe/'Projects/research/notes').mkdir(parents=True)
(probe/'team notes/修理').mkdir(parents=True)
(probe/'.archive/hiddenneedle').mkdir(parents=True)
(probe/'deletedneedle').mkdir()
(probe/'deniedneedle').mkdir()
(probe/'deniedneedle').chmod(0)
(probe/'fileneedle').write_text('not a directory')
(probe/'escape').symlink_to(OUT/'tmp', target_is_directory=True)
config = OUT/'config.kdl'
layout = OUT/'layout.kdl'
config.write_text((ROOT/'examples/locked.kdl').read_text() + f'\nsession_name "{NAME}"\n')
shutil.copyfile(ROOT/'examples/launchpad.kdl', layout)
master, slave = pty.openpty()
original = termios.tcgetattr(slave)
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH',24,80,0,0))

def setup():
    os.setsid()
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)

command = ['zellij','--config',str(config),'--layout-string',layout.read_text()]
if NATIVE: command = [str(ROOT/'target/release/zellij-launchpad-prototype')]
child = subprocess.Popen(command, stdin=slave, stdout=slave, stderr=slave,
                         env=env, cwd=ROOT, preexec_fn=setup)
class Screen(pyte.Screen):
    # Zellij probes private DSRs; pyte 0.8.2's handler lacks this keyword.
    # Raw bytes are retained unchanged for xterm.js replay.
    def report_device_status(self, mode, **kwargs):
        if not kwargs.get('private'):
            super().report_device_status(mode)

screen = Screen(80,24)
stream = pyte.Stream(screen)
decoder = codecs.getincrementaldecoder('utf-8')('replace')
raw = bytearray()
events: list[dict] = [{'resize':[80,24]}]
checks = []
# pyte does not consume DCS/APC strings (Zellij's nesting/graphics probes).
# Strip only those for text assertions; preserve original PTY writes for xterm.
filter_state = 'normal'
def feed_emulator(text):
    global filter_state
    output = []
    for char in text:
        if filter_state == 'normal':
            if char == '\x1b': filter_state = 'escape'
            else: output.append(char)
        elif filter_state == 'escape':
            if char in 'P_': filter_state = 'string'
            else:
                output.extend(('\x1b', char))
                filter_state = 'normal'
        elif filter_state == 'string':
            if char == '\x1b': filter_state = 'string_escape'
        elif filter_state == 'string_escape':
            filter_state = 'normal' if char == '\\' else 'string'
    stream.feed(''.join(output))

def pump(seconds=0.18):
    deadline = time.monotonic()+seconds
    while time.monotonic()<deadline:
        ready,_,_ = select.select([master],[],[],max(0,deadline-time.monotonic()))
        if ready:
            try:
                data = os.read(master,65536)
            except OSError:
                break
            if not data:
                break
            raw.extend(data)
            text = decoder.decode(data)
            events.append({'write':text})
            feed_emulator(text)

def display():
    return '\n'.join(screen.display)

def check(condition, label):
    assert condition, label+'\n'+display()
    checks.append(label)

def expect(text, label, timeout=5):
    deadline=time.monotonic()+timeout
    while text not in display() and time.monotonic()<deadline:
        pump(0.1)
    check(text in display(),label)

def send(data):
    os.write(master,data.encode() if isinstance(data,str) else data)
    pump()

def snapshot(name):
    (OUT/(name+'.txt')).write_text(display()+'\n')
    (OUT/(name+'.json')).write_text(json.dumps(events,ensure_ascii=False))

def resize(w,h):
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',h,w,0,0))
    screen.resize(h,w)
    events.append({'resize':[w,h]})
    os.kill(child.pid,signal.SIGWINCH)
    pump(0.4)

def mouse(x,y,button=0,release=False):
    send(f'\x1b[<{button};{x+1};{y+1}{"m" if release else "M"}')

def locate(label):
    # Controls can also be mentioned in help prose; target the footer last.
    for y,line in reversed(list(enumerate(screen.display))):
        if label in line:
            return line.index(label),y
    raise AssertionError('missing mouse target '+label+'\n'+display())

def click(label):
    x,y=locate(label)
    mouse(x,y)
    mouse(x,y,release=True)

def cli(*args):
    return subprocess.run(['zellij','--session',NAME,*args],env=env,cwd=ROOT,
                          capture_output=True,text=True,check=True,timeout=10).stdout


success = False
try:
    if not NATIVE:
        # Permission prompts do not contain Launchpad and must be handled first.
        expect('permission', 'host requests permission', timeout=45)
        snapshot('00-permissions')
        send('n' if DENY else 'y')
    expect('Launchpad', 'real dashboard loads', timeout=45)
    if DENY:
        expect('denied', 'permission denial is visible')
        check('· 0 events' in display(), 'denial never invents history')
        send('\x15notes\r')
        check('Terminal placeholder' not in display(), 'denied search cannot launch')
        snapshot('denied')
    else:
        check('· 0 events' in display(), 'real startup history is empty')
        # Explicit path validation works without waiting for the full home index.
        send('\x15')
        send('\x1b[200~~/' + probe.name + '/team notes/修理\x1b[201~')
        send('\r')
        expect('simulated launch accepted', 'real Unicode directory validates')
        send('\x1b')
        expect('HOME indexed', 'controlled index completes')
        send('\x15nts')
        expect('research/notes/', 'bare fuzzy query finds nested real directories')
        snapshot('01-search')
        click('research/notes/')
        check('Terminal placeholder' not in display(), 'real directory mouse completion never launches')
        send('\r')
        expect('simulated launch accepted', 'accepted directory revalidates')
        send('\x1b')
        send('\x15hiddenneedle')
        check('hiddenneedle/' not in display(), 'hidden directories omitted by default')
        send('\x15.archive/hiddenneedle')
        expect('hiddenneedle/', 'dot component enables hidden results')
        send('\x15fileneedle')
        check('fileneedle/' not in display(), 'files never become candidates')
        send('\x15~/' + probe.name + '/escape\r')
        expect('symlinks', 'symlink escape rejected')
        send('\x15~/' + probe.name + '/deniedneedle\r')
        expect('unavailable', 'unreadable directory rejected')
        send('\x15deletedneedle')
        expect('deletedneedle/', 'deletable candidate found')
        (probe/'deletedneedle').rmdir()
        send('\t')
        expect('unavailable', 'stale completion revalidated')
        send('\x15/etc\r')
        expect('under HOME', 'absolute outside path rejected')
        snapshot('02-error')
        send('\x1b[15~')
        expect('0 events', 'F5 clears simulated history and refreshes HOME')
        send('\x15nts')
        expect('research/notes/', 'F5 searches real HOME again')
        resize(200,36)
        check(all(not line[:20].strip() and not line[180:].strip() for line in screen.display),
              'UI stays centered at 160 columns')
        resize(30,8)
        expect('Resize to at least', 'compact guard preserved')
        send('\r')
        check('No launch' in display(), 'compact mode blocks launch')
        resize(80,24)
        snapshot('03-final')
    send('\x11')
    if not NATIVE:
        records=json.loads(cli('action','list-panes','--all','--json'))
        check(not any(p.get('plugin_url') == 'file:'+str(WASM) for p in records),
              'readback confirms plugin pane closed')
    else:
        deadline=time.monotonic()+5
        while child.poll() is None and time.monotonic()<deadline: pump(0.1)
        check(child.poll()==0, 'native exits cleanly')
    success=True
finally:
    snapshot('last')
    (OUT/'session.ansi').write_bytes(raw)
    if not NATIVE:
        subprocess.run(['zellij','--session',NAME,'kill-session',NAME],env=env,
                       capture_output=True,timeout=10)
    pump(0.3)
    if child.poll() is None: child.terminate()
    child.wait(timeout=5)
    os.close(master)
    os.close(slave)
    (probe/'deniedneedle').chmod(0o700)
    shutil.rmtree(probe)
    report={'success':success, 'checks':checks, 'passed':len(checks),
            'native':NATIVE, 'denied':DENY, 'evidence':str(OUT)}
    (OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))
