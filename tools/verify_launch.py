#!/usr/bin/env python3
"""Real own-pane replacement on Zellij 0.45.1; only owned executable fixtures.
Run with target/verification-venv/bin/python. No installed agents can be reached.
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
import pyte

ROOT = Path(__file__).resolve().parents[1]
WASM = ROOT / 'target/wasm32-wasip1/release/launchpad-plugin.wasm'
ARTIFACT_HASH = hashlib.sha256(WASM.read_bytes()).hexdigest()
ZELLIJ = shutil.which('zellij')
python = shutil.which('python3')
assert ZELLIJ and python, 'Zellij and Python are required'
PYTHON = str(Path(python).resolve())
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--tool', choices=['Shell', 'Claude', 'Codex', 'Copilot', 'Hermes'], default='Shell')
parser.add_argument('--layout', choices=['only', 'tiled', 'floating'], default='only')
parser.add_argument('--deny', action='store_true')
parser.add_argument('--missing', action='store_true')
parser.add_argument('--race', action='store_true')
parser.add_argument('--different-cwd', action='store_true')
parser.add_argument('--exit-code', type=int, choices=[0, 17], default=0)
args = parser.parse_args()
if args.race and args.layout == 'only': parser.error('--race requires a neighbor')
if args.missing and args.tool == 'Shell': parser.error('--missing requires an agent command')
OUT = ROOT / 'target/real-launch' / ('live-' + uuid.uuid4().hex[:8])
OUT.mkdir(parents=True)
NAME = OUT.name[-8:]
for d in ['home', 'bin', 'cache', 'config', 'data', 'runtime', 'sock', 'tmp']:
    (OUT/d).mkdir(mode=0o700)
home = OUT/'home'
cwd = home/"space 修理 it's literal; $HOME"
cwd.mkdir()
log = OUT/'executions.jsonl'
exit_log = OUT/'exits.jsonl'
fixture = f'''#!{PYTHON}
import os, sys, json
with open({str(log)!r}, 'a') as f:
    f.write(json.dumps({{"exe": os.path.basename(sys.argv[0]), "argv": sys.argv[1:], "cwd": os.getcwd(), "pid": os.getpid()}}) + '\\n')
print('FIXTURE_READY ' + os.path.basename(sys.argv[0]), flush=True)
for line in sys.stdin:
    if line.strip() == 'exit':
        with open({str(exit_log)!r}, 'a') as f:
            f.write(json.dumps({{"exe": os.path.basename(sys.argv[0]), "pid": os.getpid(), "exit_code": {args.exit_code}}}) + '\\n')
        sys.exit({args.exit_code})
    print('FIXTURE_INPUT ' + line.strip(), flush=True)
'''
for name in ['default-shell', 'neighbor', 'claude', 'codex', 'copilot', 'hermes']:
    if args.missing and name == args.tool.lower():
        continue
    p = OUT/'bin'/name
    p.write_text(fixture)
    p.chmod(0o700)
env = {k:v for k,v in os.environ.items() if not k.startswith('ZELLIJ') and k != 'TMUX'}
env.update(HOME=str(home), PATH=str(OUT/'bin'), SHELL='/bin/false', TERM='xterm-256color',
           COLORTERM='truecolor', XDG_CACHE_HOME=str(OUT/'cache'), XDG_CONFIG_HOME=str(OUT/'config'),
           XDG_DATA_HOME=str(OUT/'data'), XDG_RUNTIME_DIR=str(OUT/'runtime'),
           ZELLIJ_SOCKET_DIR=str(OUT/'sock'), TMPDIR=str(OUT/'tmp'))
config = OUT/'config.kdl'
config.write_text((ROOT/'examples/locked.kdl').read_text() +
                  f'\nsession_name "{NAME}"\ndefault_shell "{OUT}/bin/default-shell"\n')
plugin = f'pane name="launchpad" focus=true {{ plugin location="file:{WASM}"; }}'
neighbor = f'pane name="neighbor" command="{OUT}/bin/neighbor"'
if args.layout == 'only':
    layout = f'layout {{ pane {{ plugin location="file:{WASM}"; }}; }}'
elif args.layout == 'tiled':
    layout = f'layout {{ pane split_direction="vertical" {{ {neighbor}; {plugin}; }}; }}'
else:
    plugin = plugin.replace('focus=true', 'focus=true width=120 height=30')
    layout = f'layout {{ tab {{ {neighbor}; floating_panes {{ {plugin}; }}; }}; }}'
(OUT/'layout.kdl').write_text(layout)
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH',36,160,0,0))
def setup():
    os.setsid()
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)
child = subprocess.Popen([ZELLIJ, '--config', str(config), '--layout-string', layout],
                         stdin=slave, stdout=slave, stderr=slave, env=env,
                         cwd=OUT/'tmp' if args.different_cwd else home, preexec_fn=setup)
class Screen(pyte.Screen):
    def report_device_status(self, mode, **kwargs):
        if not kwargs.get('private'): super().report_device_status(mode)
screen = Screen(160,36)
stream = pyte.Stream(screen)
decoder = codecs.getincrementaldecoder('utf-8')('replace')
raw = bytearray()
events: list[dict] = [{'resize':[160,36]}]
checks = []
filter_state = 'normal'
def feed_emulator(text):
    # Preserve all bytes for xterm.js, but omit DCS/APC unsupported by pyte.
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
def pump(seconds=.1):
    end = time.monotonic()+seconds
    while time.monotonic()<end:
        if select.select([master],[],[],max(0,end-time.monotonic()))[0]:
            try: data = os.read(master,65536)
            except OSError: break
            if not data: break
            raw.extend(data)
            text=decoder.decode(data)
            events.append({'write':text})
            feed_emulator(text)
def display(): return '\n'.join(screen.display)
def check(value, label):
    assert value, label+'\n'+display()
    checks.append(label)
def expect(text, label, timeout=45):
    end=time.monotonic()+timeout
    while text not in display() and time.monotonic()<end: pump()
    check(text in display(),label)
def send(text):
    os.write(master,text.encode())
    pump(.2)
def cli(*command):
    result = subprocess.run([ZELLIJ,'--session',NAME,*command],env=env,cwd=home,
                            capture_output=True,text=True,timeout=10)
    assert result.returncode == 0, f'{command}: {result.stderr} {result.stdout}'
    return result.stdout
def panes(name):
    result=json.loads(cli('action','list-panes','--all','--json'))
    (OUT/(name+'.json')).write_text(json.dumps(result,indent=2))
    return result
def records():
    return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
def snapshot(name):
    (OUT/(name+'.txt')).write_text(display())
    (OUT/(name+'.json')).write_text(json.dumps(events))
success=False
try:
    expect('permission','permission prompt')
    expect('Execute actions as the user', 'host asks for run-action permission')
    expect('Start new terminals and plugins', 'host asks for terminal opening')
    snapshot('00-permissions')
    send('n' if args.deny else 'y')
    if args.deny:
        expect('denied','denial visible')
        before=panes('panes-before')
        send('\r\r\x1b[17~\r')
        pump(.5)
        after=panes('panes-after')
        check(before == after,'denial retains exact pane list')
        check(all(r['exe']=='neighbor' for r in records()),'denial executes no launch')
        check('Terminal placeholder' not in display(),'denial never simulates success')
    else:
        expect('HOME indexed','real worker ready')
        send('\x15')
        send("\x1b[200~~/space 修理 it's literal; $HOME\x1b[201~")
        send('\x14')
        for _ in range(['Shell','Claude','Codex','Copilot','Hermes'].index(args.tool)):
            send('\x1b[C')
        before=panes('panes-before')
        plugin_before=next(p for p in before if p.get('plugin_url') == 'file:'+str(WASM))
        snapshot('01-before')
        if args.race:
            neighbor_before=next(p for p in before if p.get('title') == 'neighbor')
            cli('action','focus-pane-id', 'terminal_'+str(neighbor_before['id']))
            focused=panes('panes-focused-neighbor')
            check(next(p for p in focused if not p['is_plugin'] and p['id']==neighbor_before['id'])['is_focused'],
                  'neighbor focused before asynchronous validation')
            # 0.45.1 lists both layers' selected panes as focused even when
            # floats are hidden. Prove actual keyboard focus before submitting.
            send('neighbor-focus-proof\r')
            expect('FIXTURE_INPUT neighbor-focus-proof', 'neighbor receives actual keyboard input', timeout=5)
            cli('action','send-keys','--pane-id','plugin_'+str(plugin_before['id']),'Enter')
        else:
            send('\r')
        expected='default-shell' if args.tool=='Shell' else args.tool.lower()
        if args.missing:
            expect('did not accept', 'asynchronous spawn failure is visible on original dashboard', timeout=8)
            missing=panes('panes-missing')
            check({(p['is_plugin'],p['id']) for p in missing} ==
                  {(p['is_plugin'],p['id']) for p in before},
                  'missing executable leaves original plugin and neighbors, no held error pane')
            check(not any(r['exe']==expected for r in records()),
                  'missing executable is not probed or substituted')
            snapshot('02-missing')
            # Install only our inert fixture; recovery must need an explicit submit.
            executable=OUT/'bin'/expected
            executable.write_text(fixture)
            executable.chmod(0o700)
            pump(.5)
            check(not any(r['exe']==expected for r in records()), 'spawn failure does not auto-retry')
            cli('action','send-keys','--pane-id','plugin_'+str(plugin_before['id']),'Enter')
        end=time.monotonic()+8
        while not any(r['exe']==expected for r in records()) and time.monotonic()<end: pump()
        check(any(r['exe']==expected for r in records()), 'selected executable actually starts')
        pump(.2)
        after=panes('panes-after')
        check(not any(p.get('plugin_url')=='file:'+str(WASM) for p in after),'originating plugin is removed')
        check(len(before)==len(after),'pane count unchanged')
        old_ids={(p['is_plugin'],p['id']) for p in before}
        replacement=next(p for p in after if (p['is_plugin'],p['id']) not in old_ids)
        check(replacement['terminal_command']==(str(OUT/'bin/default-shell') if args.tool=='Shell' else expected),
              'host command readback matches selected launch mapping')
        geometry=['pane_x','pane_y','pane_rows','pane_columns','is_floating','tab_id','tab_position']
        check(all(replacement[k]==plugin_before[k] for k in geometry),'replacement preserves pane geometry and tab')
        for old in before:
            if not old['is_plugin']:
                new=next(p for p in after if not p['is_plugin'] and p['id']==old['id'])
                stable=['id','terminal_command','pane_command','pane_cwd',*geometry]
                check(all(new.get(k)==old.get(k) for k in stable),'neighbor identity, command, cwd and geometry unchanged')
        check('Terminal placeholder' not in display(),'real request never renders a fake terminal')
        launched=[r for r in records() if r['exe']==expected]
        check(len(launched)==1,'exactly one OS launch')
        check(launched[0]['argv']==[],'no command flags')
        check(launched[0]['cwd']==str(cwd),'literal Unicode and metacharacter cwd')
        check(replacement['pane_cwd']==str(cwd),'host readback confirms literal cwd')
        if not replacement['is_focused'] or (args.layout=='floating' and args.race):
            cli('action','focus-pane-id','terminal_'+str(replacement['id']))
        expect('FIXTURE_READY '+expected, 'replacement renders actual fixture output', timeout=5)
        send('hello\r')
        expect('FIXTURE_INPUT hello','fixture remains interactive',timeout=5)
        snapshot('02-running')
        pump(.5)
        check(len([r for r in records() if r['exe']==expected])==1,'no duplicate launches after progress and input')
        send('exit\r')
        pump(.5)
        exits=[json.loads(line) for line in exit_log.read_text().splitlines()]
        check(exits == [{'exe':expected, 'pid':launched[0]['pid'], 'exit_code':args.exit_code}],
              f'launched fixture exits with requested status {args.exit_code}')
        if args.layout == 'only':
            end=time.monotonic()+5
            while child.poll() is None and time.monotonic()<end: pump()
            check(child.poll()==0, 'last pane exit ends the Zellij client cleanly')
            readback=subprocess.run([ZELLIJ,'--session',NAME,'action','list-panes','--all','--json'],
                                    env=env,cwd=home,capture_output=True,text=True,timeout=10)
            (OUT/'panes-exited-session.json').write_text(json.dumps({
                'returncode':readback.returncode,'stdout':readback.stdout,'stderr':readback.stderr},indent=2))
            check(readback.returncode==1 and readback.stderr.strip()=='There is no active session!',
                  'host confirms session gone, with no held pane or dashboard')
        else:
            exited=panes('panes-exited')
            check(not any(p.get('plugin_url')=='file:'+str(WASM) for p in exited),'exiting command cannot resurrect dashboard')
            check(not any(not p['is_plugin'] and p['id']==replacement['id'] for p in exited),
                  'exited command pane closes instead of remaining held')
            neighbors=[p for p in before if p != plugin_before]
            check({(p['is_plugin'],p['id']) for p in exited} == {(p['is_plugin'],p['id']) for p in neighbors},
                  'after exit only original neighbors remain')
            for old in neighbors:
                new=next(p for p in exited if (p['is_plugin'],p['id'])==(old['is_plugin'],old['id']))
                check(all(new.get(k)==old.get(k) for k in ['terminal_command','pane_command','pane_cwd','tab_id','is_floating']),
                      'neighbor command, cwd, tab and layout type survive exit')
            pump(.5)
            check(panes('panes-settled')==exited, 'settled pane list remains unchanged after exit')
        snapshot('02-after')
    check(hashlib.sha256(WASM.read_bytes()).hexdigest()==ARTIFACT_HASH,
          'fresh isolated host loaded the unchanged artifact')
    success=True
finally:
    snapshot('last')
    (OUT/'session.ansi').write_bytes(raw)
    subprocess.run([ZELLIJ,'--session',NAME,'kill-session',NAME],env=env,capture_output=True,timeout=10)
    pump(.2)
    if child.poll() is None: child.terminate()
    child.wait(timeout=5)
    os.close(master);os.close(slave)
    report={'success':success,'checks':checks,'passed':len(checks),'case':vars(args),
            'evidence':str(OUT),'sha256':ARTIFACT_HASH}
    (OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))
