#!/usr/bin/env python3
"""Bounded live Zellij 0.45.1 PTY verification; pyte is development-only.
Run: target/verification-venv/bin/python tools/verify_zellij.py
All host state and evidence live under target/zj-<unique id>. No global cleanup.
"""
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

import pyte

ROOT = Path(__file__).resolve().parents[1]
NAME = 'lp-' + uuid.uuid4().hex[:8]
OUT = ROOT / 'target' / ('zj-' + NAME[3:])
OUT.mkdir(parents=True)
WASM = ROOT / 'target/wasm32-wasip1/release/zellij-launchpad.wasm'
assert WASM.is_file(), 'Build the release WASM first'
for directory in ('home', 'cache', 'config', 'data', 'runtime', 'sock', 'tmp'):
    (OUT / directory).mkdir(mode=0o700)
env = {k: v for k, v in os.environ.items() if not k.startswith('ZELLIJ') and k != 'TMUX'}
env.update(TERM='xterm-256color', COLORTERM='truecolor', HOME=str(OUT/'home'),
           XDG_CACHE_HOME=str(OUT/'cache'), XDG_CONFIG_HOME=str(OUT/'config'),
           XDG_DATA_HOME=str(OUT/'data'), XDG_RUNTIME_DIR=str(OUT/'runtime'),
           ZELLIJ_SOCKET_DIR=str(OUT/'sock'), TMPDIR=str(OUT/'tmp'), SHELL='/bin/sh',
           TERM_PROGRAM='launchpad-verification')
config = OUT/'config.kdl'
layout = OUT/'layout.kdl'
config.write_text((ROOT/'examples/locked.kdl').read_text() + f'\nsession_name "{NAME}"\n')
# The dashboard has no built-in directories: seed a disposable HOME instead.
FIXTURES = ('Projects/launchpad', 'Projects/notes', 'Projects/research/notes',
            'Projects/service/api', 'Projects/team notes', 'Projects/修理',
            'Projects/café', "Projects/it's literal; $HOME", 'Documents')
for fixture in FIXTURES:
    (OUT/'home'/fixture).mkdir(parents=True, exist_ok=True)
# Four configured commands after the built-in Shell. Nothing is ever spawned:
# simulate_launch validates the directory and records the attempt in memory.
SETTINGS = ('simulate_launch "true"; commands "claude,codex,copilot,hermes"; '
            'command_claude "claude"; label_claude "Claude"; '
            'command_codex "codex"; label_codex "Codex"; '
            'command_copilot "copilot"; label_copilot "Copilot"; '
            'command_hermes "hermes"; label_hermes "Hermes";')
layout.write_text((ROOT/'examples/launchpad.kdl').read_text()
                  .replace('.wasm"', '.wasm" { ' + SETTINGS + ' }'))
master, slave = pty.openpty()
original = termios.tcgetattr(slave)
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH',24,80,0,0))

def setup():
    os.setsid()
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)

command = ['zellij','--config',str(config),'--layout-string',layout.read_text()]
child = subprocess.Popen(command, stdin=slave, stdout=slave, stderr=slave,
                         env=env, cwd=str(OUT/'home'/'Projects'), preexec_fn=setup)
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

success=False
try:
    expect('permission','actual WASM requests its host permissions',timeout=45)
    snapshot('00-permissions')
    send('y')
    expect('Launchpad','actual WASM loads after the grant',timeout=45)
    expect('HOME indexed','worker finishes the disposable HOME index',timeout=60)
    snapshot('01-initial-80x24')
    send('\x15notes')
    expect('~/Projects/research/notes','embedded fuzzy results')
    snapshot('02-notes')
    send('\x1b[B')
    send('\r')
    expect('Enter launches','completion is separate from launch')
    check('Launch suppressed' not in display(),'completion never launches')
    send('\x14')
    send('\x1b[C')
    expect('[ Claude ]','locked mode delivers Ctrl+T and arrows')
    send('\r')
    expect('Launch suppressed (simulate_launch): Claude','selected tool is validated, not spawned')
    snapshot('03-suppressed')
    send('\x1b')
    expect('01 Directory','keyboard dismisses the launch notice')
    send('\x12')
    send('\t')
    expect('Copied to form','history copy without launch')
    send('\x12')
    send('\r')
    expect('Launch suppressed','history replay')
    send('\x1b')
    send('\x1bOP')
    expect('Launchpad / help','F1 help')
    snapshot('04-help')
    send('\x1b')
    send('\x10\x15~/Projects/missing\r')
    expect('Directory unavailable','invalid path preserved')
    snapshot('05-invalid')
    send('\x1b[15~')
    expect('[ Shell ]','F5 resets the form to Shell')
    send('\x15nts')
    expect('~/Projects/notes','noncontiguous fuzzy matching in WASM')
    send('\x1b[15~')
    send('\x15notes')
    mouse(12,6)
    mouse(12,6,release=True)
    expect('Enter launches','mouse suggestion completes without launch')
    check('Launch suppressed' not in display(),'suggestion click is completion only')
    mouse(26,9)
    mouse(26,9,release=True)
    expect('[ Codex ]','mouse tool selection')
    before=display()
    mouse(0,0)  # a real drag starts with a press; orphan motion is a host click
    mouse(68,9,button=32)  # drag/hold, not click
    mouse(68,9,release=True)
    check(display()==before,'mouse hold/release cannot launch')
    click('Enter ↵')
    expect('Launch suppressed (simulate_launch): Codex','explicit mouse form launch')
    send('\x1b')
    expect('01 Directory','mouse launch notice dismissed')
    click('01 Codex')
    expect('Tab copy','mouse history selection without launch')
    click('Tab copy')
    expect('Copied to form','mouse copy into form')
    click('01 Codex')
    click('Enter replay')
    expect('Launch suppressed','explicit mouse history replay')
    send('\x1b')
    click('F5 reset')
    send('\x15')
    send('\x1b[200~~/Projects/修理/e\u0301\n\t\x1b[201~')
    expect('~/Projects/修理/é','bracketed Unicode paste is literal')
    check(screen.buffer[3][77].data=='│','Unicode preserves input right border at column 78')
    mouse(18,3) # continuation of first CJK character: must snap before 修
    mouse(18,3,release=True)
    send('X')
    expect('~/Projects/X修理/é','mouse CJK continuation places grapheme-safe caret')
    snapshot('06-unicode-80x24')
    resize(120,36)
    expect('~/Projects/X修理/é','wide resize preserves state')
    check(screen.buffer[4][117].data=='│','wide layout right border stays aligned')
    snapshot('07-wide-120x36')
    click('F5 reset')
    # F5 also clears the in-memory attempts, so seed one row for the wheel.
    send('\x15~/Projects/notes\r')
    expect('Launch suppressed','a recent row exists for the wheel checks')
    send('\x1b')
    # SDK wheel events contain a count but no position: send hover first.
    mouse(10,0,button=35)
    before=display()
    mouse(10,0,button=65)
    check(display()==before,'wheel on heading does not change focus')
    mouse(10,20,button=35)
    mouse(10,20,button=65)
    expect('Enter replay','wheel over history focuses only that section')
    snapshot('08-wheel-history')
    click('F1 help')
    before=display()
    mouse(10,8,button=35)
    mouse(10,8,button=65)
    check(display()!=before and 'Launchpad / help' in display(),'help wheel scroll')
    click('Esc back')
    expect('01 Directory','mouse exits scrolled help')
    resize(40,10)
    expect('01 Directory','narrow interactive layout')
    snapshot('09-narrow-40x10')
    resize(30,8)
    expect('Resize to at least','small guard')
    send('\r')
    check('No launch' in display(),'small guard blocks launch')
    snapshot('10-guard-30x8')
    resize(80,24)
    send('\x1b[15~')
    # Prove host interception and recovery, rather than claiming normal-mode parity.
    send('\x07')  # unlock
    send('\x14')  # configured host tab mode, NOT tools focus
    send('\x1b')
    send('\x07')  # lock again
    check('[ Shell ]' in display(),'normal mode host shortcut does not select tools')
    send('\x14')
    send('\x1b[C')
    expect('[ Claude ]','locked mode restores domain shortcut delivery')
    send('\x1b[15~')
    panes_before=cli('action','list-panes','--all','--json')
    (OUT/'panes-before.json').write_text(panes_before)
    before_records=json.loads(panes_before)
    launchpads=[p for p in before_records if p.get('plugin_url')=='file:'+str(WASM)]
    shell_ids={p['id'] for p in before_records if not p['is_plugin']}
    check(len(launchpads)==1 and len(shell_ids)==1,'host manifest confirms exact WASM and surviving shell identity')
    snapshot('11-before-quit')
    click('Ctrl+Q quit')
    pump(0.5)
    check('Launchpad' not in display() and child.poll() is None,'Quit closes only the plugin, session survives')
    panes_after=cli('action','list-panes','--all','--json')
    (OUT/'panes-after.json').write_text(panes_after)
    after_records=json.loads(panes_after)
    check(not any(p.get('plugin_url')=='file:'+str(WASM) for p in after_records)
          and {p['id'] for p in after_records if not p['is_plugin']}==shell_ids,
          'host readback confirms plugin removed and same ordinary pane survives')
    send("printf 'SURVIVOR_OK\\n'\r")
    expect('SURVIVOR_OK','ordinary shell pane remains usable')
    snapshot('12-surviving-shell')
    reopened=cli('action','launch-plugin','--configuration','simulate_launch=true','file:'+str(WASM)).strip()
    (OUT/'reopened-plugin.txt').write_text(reopened+'\n')
    expect('Launchpad','WASM reloads in split alongside surviving ordinary pane')
    snapshot('13-split-before-key-quit')
    send('\x11')
    pump(0.3)
    records=json.loads(cli('action','list-panes','--all','--json'))
    (OUT/'panes-after-key-quit.json').write_text(json.dumps(records,indent=2))
    check(not any(p.get('plugin_url')=='file:'+str(WASM) for p in records)
          and {p['id'] for p in records if not p['is_plugin']}==shell_ids,
          'Ctrl+Q closes only own split plugin pane, preserving shell in same tab')
    send('\x07') # unlock, then host Quit remains possible
    send('\x11')
    deadline=time.monotonic()+5
    while child.poll() is None and time.monotonic()<deadline: pump(0.1)
    check(child.poll()==0,'Ctrl+G then Ctrl+Q exits the host normally')
    success=True
finally:
    snapshot('last')
    (OUT/'session.ansi').write_bytes(raw)
    # Only this exact session in its private socket directory is touched.
    subprocess.run(['zellij','--session',NAME,'kill-session',NAME],env=env,
                   capture_output=True,timeout=10)
    pump(0.3)
    if child.poll() is None:
        child.terminate()
    child.wait(timeout=5)
    restored=termios.tcgetattr(slave)==original
    os.close(master)
    os.close(slave)
    report={'success':success,'checks':checks,'passed':len(checks),
            'host':subprocess.check_output(['zellij','--version'],text=True).strip(),
            'wasm':str(WASM),'sha256':hashlib.sha256(WASM.read_bytes()).hexdigest(),
            'command':command,'evidence':str(OUT),'termios_restored':restored}
    (OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))
