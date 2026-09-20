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
WASM = ROOT / 'target/wasm32-wasip1/release/zellij-launchpad.wasm'
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
parser.add_argument('--during-index', action='store_true')
parser.add_argument('--different-cwd', action='store_true')
parser.add_argument('--commands-case', choices=['fixtures', 'shell-only', 'agreed', 'literals', 'invalid', 'many', 'review-ui'], default='fixtures')
parser.add_argument('--refresh', action='store_true')
parser.add_argument('--initial-cwd', action='store_true', help='One Enter from the untouched invoking directory')
parser.add_argument('--reset-remount', action='store_true', help='Enter then F5 in one input batch; verify cancelled remount and explicit retry')
parser.add_argument('--cwd-case', choices=['ordinary', 'home', 'outside', 'hidden', 'ignored', 'symlink', 'long', 'deleted', 'ambiguous', 'invalid-bytes'], default='ordinary')
parser.add_argument('--startup-redraw', action='store_true', help='One recorded viewport redraw before the unchanged permission assertion')
parser.add_argument('--exit-code', type=int, choices=[0, 17], default=0)
args = parser.parse_args()
if args.race and args.layout == 'only': parser.error('--race requires a neighbor')
if args.missing and args.tool == 'Shell': parser.error('--missing requires an agent command')
if args.reset_remount and (not args.initial_cwd or args.tool != 'Shell' or args.layout != 'only' or args.during_index or args.cwd_case not in ['ordinary', 'deleted']):
    parser.error('--reset-remount requires initial-cwd, Shell, only layout, ordinary/deleted cwd and settled indexing')
OUT = ROOT / 'target/real-launch' / ('live-' + uuid.uuid4().hex[:8])
OUT.mkdir(parents=True)
NAME = OUT.name[-8:]
for d in ['home', 'bin', 'cache', 'config', 'data', 'runtime', 'sock', 'tmp']:
    (OUT/d).mkdir(mode=0o700)
home = OUT/'home'
cwd = home/"space 修理 it's literal; $HOME"
cwd.mkdir()
if args.cwd_case != 'ordinary':
    assert args.initial_cwd
    if args.cwd_case == 'home': cwd = home
    elif args.cwd_case == 'outside': cwd = OUT/'outside'
    elif args.cwd_case == 'hidden': cwd = home/'.hidden'
    elif args.cwd_case == 'ignored':
        cwd = home/'ignored'
        (home/'.ignore').write_text('ignored/\n')
    elif args.cwd_case == 'symlink':
        target = OUT/'symlink-target'
        target.mkdir()
        cwd = home/'symlink'
        cwd.symlink_to(target, target_is_directory=True)
    elif args.cwd_case == 'long': cwd = home/('修理 é; $HOME '+ 'x'*140)
    elif args.cwd_case == 'ambiguous': cwd = home/'replacement-�'
    elif args.cwd_case == 'invalid-bytes':
        cwd = Path(os.fsdecode(os.fsencode(home)+b'/invalid-\xff'))
        (home/'invalid-�').mkdir()  # existing lossy twin must NEVER be launched
    else: cwd = home/'deleted'
    cwd.mkdir(exist_ok=True)
expected_cwd = str(cwd.resolve())
expected_input = ('~/' + str(cwd.relative_to(home))) if cwd.is_relative_to(home) else str(cwd)
if cwd == home: expected_input = '~'
if args.cwd_case in ['outside', 'hidden', 'ignored', 'symlink']:
    (cwd/'not-in-home-index').mkdir()
if args.during_index:
    assert args.initial_cwd and not args.refresh
    for i in range(15000): (home/f'index-{i:05}').mkdir()
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
configuration = {}
order = ['Shell', 'Claude', 'Codex', 'Copilot', 'Hermes']
labels = {name: name for name in order}
expected_argv = []
expected_exe = 'default-shell' if args.tool == 'Shell' else args.tool.lower()
if args.commands_case != 'shell-only':
    configuration['commands'] = 'claude,codex,copilot,hermes'
    for name in order[1:]:
        configuration['command_'+name.lower()] = name.lower()
        configuration['label_'+name.lower()] = name
if args.commands_case == 'shell-only':
    assert args.tool == 'Shell'
    order = ['Shell']
if args.commands_case == 'agreed':
    configuration['commands'] = 'claude,hermes,codex'
    configuration['arguments_claude'] = '-w'
    configuration['label_claude'] = labels['Claude'] = 'Claude in Worktree'
    order = ['Shell', 'Claude', 'Hermes', 'Codex']
    if args.tool == 'Claude': expected_argv = ['-w']
if args.commands_case == 'literals':
    assert args.tool == 'Claude'
    configuration = {'commands': 'Variant_2.worktree,second', 'command_Variant_2.worktree': 'claude',
                     'label_Variant_2.worktree': 'Literal 修理 é', 'command_second': 'claude',
                     'arguments_Variant_2.worktree': r"-w --model \"some model\" '' \"\" a\ b '$HOME' ; '$(touch NO_EXPANSION)' ~ *.rs | >".replace('\\"', '"')}
    labels['Claude'] = 'Literal 修理 é'
    order = ['Shell', 'Claude']
    expected_argv = ['-w','--model','some model','','','a b','$HOME',';','$(touch NO_EXPANSION)','~','*.rs','|','>']
if args.commands_case == 'invalid':
    configuration = {'commands': 'bad,missing,claude', 'command_bad': 'claude', 'arguments_bad': "'unterminated", 'command_claude': 'claude', 'label_claude': 'Claude'}
    order = ['Shell','Claude']
if args.commands_case == 'many':
    assert args.tool == 'Claude'
    configuration = {'commands': ','.join(f'variant{n}' for n in range(64))}
    for n in range(64):
        configuration[f'command_variant{n}'] = 'claude'
        configuration[f'label_variant{n}'] = f'Command {n:02} 修理 é '+('long label '*12)
        configuration[f'arguments_variant{n}'] = f'--variant {n}'
    labels['Claude'] = 'Command 63 修理'
    expected_argv = ['--variant', '63']
if args.commands_case == 'review-ui':
    assert args.tool == 'Claude' and args.layout == 'only'
    long_id = 'i'*64
    configuration = {'commands': long_id+',unicode', 'command_'+long_id: 'claude',
                     'label_'+long_id: 'x'*243+' END_OF_LABEL',
                     'command_unicode': 'claude', 'label_unicode': ('界é👩🏽‍💻🇬🇧✈️ '*5)+' UNICODE_END'}
    order = ['Shell', 'Claude']
config_kdl = '; '.join(k+' '+json.dumps(v, ensure_ascii=False) for k,v in configuration.items())
if config_kdl: config_kdl += ';'
plugin = f'pane name="launchpad" focus=true {{ plugin location="file:{WASM}" {{ {config_kdl} }}; }}'
if args.initial_cwd and args.cwd_case != 'invalid-bytes':
    # Plugin cwd is a plugin-block property, not a pane property. Supplying it
    # also exercises a logical symlink path rather than Python's physical cwd.
    plugin = plugin.replace('plugin location=', 'plugin cwd='+json.dumps(str(cwd), ensure_ascii=False)+' location=')
neighbor = f'pane name="neighbor" command="{OUT}/bin/neighbor"'
if args.layout == 'only':
    layout = f'layout {{ {plugin}; }}'
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
                         cwd=cwd if args.initial_cwd else (OUT/'tmp' if args.different_cwd else home), preexec_fn=setup)
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
    if args.startup_redraw:
        pump(1)
        snapshot('startup-before-redraw')
        panes('startup-panes-before-redraw')
        for rows in [35,36]:
            fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack('HHHH',rows,160,0,0))
            screen.resize(lines=rows, columns=160)
            events.append({'resize':[160,rows]})
            pump(.3)
        snapshot('startup-after-redraw')
    if args.cwd_case in ['ambiguous', 'invalid-bytes']:
        expect('unsupported; no fallback', 'ambiguous invoking identity is refused visibly')
        before=panes('unsupported-before')
        send('\r\x1b[15~\r')
        check(panes('unsupported-after') == before, 'unsupported cwd never replaces or renames')
        check(records() == [], 'unsupported cwd and its existing UTF-8 twin execute nothing')
        check(hashlib.sha256(WASM.read_bytes()).hexdigest()==ARTIFACT_HASH, 'loaded artifact unchanged')
        success=True
        raise SystemExit(0)
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
        check('Launch suppressed' not in display(),'denial never simulates success')
    else:
        expect('Indexing HOME' if args.during_index else ('~/space' if args.commands_case == 'invalid' else 'HOME indexed'),'real worker ready')
        if args.commands_case == 'shell-only':
            check('[ Shell ]' in display() and all(n not in display() for n in ['Claude','Codex','Hermes','Copilot']), 'unconfigured production is Shell-only')
        elif args.commands_case not in ['many', 'review-ui']:
            expect(labels[args.tool], 'configured label loaded through plugin config')
        if args.commands_case == 'invalid':
            expect('Config error', 'invalid arguments produce visible config error')
            send('\x1bOP')
            send('\x1b[F')
            expect('command_missing', 'missing executable field diagnostic in help')
            send('\x1b')
        if args.commands_case == 'many':
            def resize(columns, rows):
                fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack('HHHH',rows,columns,0,0))
                screen.resize(lines=rows, columns=columns)
                events.append({'resize':[columns,rows]})
                pump(.3)
            def mouse(x,y):
                send(f'\x1b[<0;{x+1};{y+1}M')
                send(f'\x1b[<0;{x+1};{y+1}m')
            for columns,rows in [(80,24),(40,12),(40,10),(200,36)]:
                resize(columns,rows)
                send('\x1b[15~')
                expect('[ Shell ]',f'initial Shell after reset at {columns}x{rows}')
                send('\x14')
                for n in range(64):
                    os.write(master,b'\x1b[C'); pump(.025)
                    if n in [0,31,63]: expect(f'[ Command {n:02}',f'keyboard reaches configured entry {n} at {columns}x{rows}')
                expect('65/65',f'overflow position at {columns}x{rows}')
                check('03 Recent' in display(),f'many commands retain history heading at {columns}x{rows}')
                row = next(y for y,line in enumerate(screen.display) if '‹ 65/65 ›' in line)
                x = screen.display[row].index('‹ 65/65 ›')
                mouse(x,row)
                expect('[ Command 62',f'mouse previous overflow control at {columns}x{rows}')
                mouse(x+8,row)
                expect('[ Command 63',f'mouse next overflow control at {columns}x{rows}')
                row = next(y for y,line in enumerate(screen.display) if '[ Command 63' in line)
                mouse(screen.display[row].index('[ Command 63')+3,row)
                check(not records(),f'mouse selection does not launch at {columns}x{rows}')
                if columns == 200:
                    check(all(not line[:20].strip() and not line[180:].strip() for line in screen.display),'200-column host frame preserves blank 160-column UI gutters')
                snapshot(f'configured-many-{columns}x{rows}')
            resize(160,36)
            send('\x1b[15~')
            expect('HOME indexed','ready after configured size/reset checks')
        if args.commands_case == 'review-ui':
            def resize(columns, rows):
                fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack('HHHH',rows,columns,0,0))
                screen.resize(lines=rows, columns=columns)
                events.append({'resize':[columns,rows]})
                pump(.4)
            resize(40,10)
            send('\x1bOP')
            send('\x1b[H')
            for _ in range(180):
                if 'END_OF_LABEL' in display(): break
                os.write(master,b'\x1b[B'); pump(.04)
            check('END_OF_LABEL' in display(), '40x10 keyboard scrolling reaches maximum ID/label tail')
            snapshot('review-help-ascii-tail-40x10')
            send('\x1b[F')
            expect('UNICODE_END', '40x10 End reaches Unicode label tail', timeout=5)
            snapshot('review-help-unicode-tail-40x10')
            end_screen = display()
            send('\x1b[A')
            check(display() != end_screen, 'Up after End moves immediately')
            # Pinned SDK wheel has no coordinates; establish last pointer first.
            send('\x1b[<0;6;4M')
            send('\x1b[<0;6;4m')
            send('\x1b[<65;6;4M')
            check(display() == end_screen, 'help wheel down returns one rendered row to End')
            resize(200,36)
            send('\x1b[F')
            expect('UNICODE_END', 'resized help still reaches final tail', timeout=5)
            check(all(not line[:20].strip() and not line[180:].strip() for line in screen.display), 'help retains max160 gutters')
            snapshot('review-help-200x36')
            resize(40,10)
            send('\x1b')
            history_files = list((OUT/'cache').rglob('history.json'))
            check(len(history_files) == 1, 'review uses only owned URL cache')
            journal = history_files[0].parent/'history.d'
            journal.mkdir(exist_ok=True)
            token = '00000000000000000001-review'
            row = {'id': token, 'path':str(cwd), 'tool':'removed-long-id', 'opened_at':int(time.time())}
            (journal/(token+'.json')).write_text(json.dumps({'version':2,'id':token,'entry':row}))
            send('\x1b[15~')
            expect('!removed', '40x10 unavailable marker survives clipped long ID', timeout=8)
            y = next(y for y,line in enumerate(screen.display) if '!removed' in line)
            x = screen.display[y].index('!removed')
            check('unavailable' not in screen.display[y], 'marker is visible with age/status column hidden')
            send(f'\x1b[<0;{x+1};{y+1}M')
            send(f'\x1b[<0;{x+1};{y+1}m')
            check(not records(), 'narrow history mouse selection starts no process')
            snapshot('review-unavailable-40x10')
            send('\r')
            expect('unavailable', 'removed long ID cannot replay', timeout=8)
            send('\t')
            send('\r')
            expect('unavailable', 'copied unavailable ID cannot launch', timeout=8)
            check(not records(), 'removed replay/copy never substitutes an executable')
            snapshot('review-unavailable-rejected-40x10')
            resize(160,36)
            send('\x1b[15~')
            expect('HOME indexed', 'review ready for explicit configured launch')
        if args.refresh:
            send('\x1b[15~')
            expect('~/space' if args.commands_case == 'invalid' else 'HOME indexed','configured F5 refresh ready')
            expect(labels[args.tool], 'configured label survives F5 app recreation')
        if args.initial_cwd:
            if args.cwd_case in ['outside', 'hidden', 'ignored', 'symlink'] and not args.during_index:
                check('HOME indexed · 1 dirs' in display(), 'invoking exception and its descendants do not widen HOME index')
            path_row = next(line for line in screen.display if '│ › ' in line)
            import unicodedata
            expected_tail = unicodedata.normalize('NFC', expected_input[-70:])
            check(expected_tail in path_row, 'untouched input is invoking cwd after remount/reset')
            check('[ Shell ]' in display(), 'initial tool is Shell')
        else:
            send('\x15')
            send("\x1b[200~~/space 修理 it's literal; $HOME\x1b[201~")
            send('\x14')
        for _ in range(64 if args.commands_case == 'many' else order.index(args.tool)):
            send('\x1b[C')
        before=panes('panes-before')
        plugin_before=next(p for p in before if p.get('plugin_url') == 'file:'+str(WASM))
        snapshot('01-before')
        if args.during_index:
            check('Indexing HOME' in display(), 'first Enter is sent while indexing is in progress')
        if args.cwd_case == 'deleted':
            cwd.rmdir()
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
            if args.reset_remount:
                send('\r\x1b[15~')
                pump(2)
                snapshot('reset-after-pending-remount')
                check('HOME filesystem unavailable' not in display(), 'cancelled cwd failure is not a HOME bootstrap failure')
                expect('HOME indexed', 'F5 worker recovers after cancelled remount', timeout=8)
                check(records() == [], 'reset cancels old launch without auto-retry')
                check(panes('panes-after-reset') == before, 'reset and stale acknowledgement preserve plugin and tab name')
                check(expected_input in display(), 'reset retains invoking cwd even when it was deleted')
                check('[ Shell ]' in display(), 'reset retains Shell default')
            send('\r')
        expected=expected_exe
        if args.cwd_case == 'deleted':
            expect('Invoking directory unavailable', 'deleted invoking cwd fails visibly without fallback', timeout=8)
            check(records() == [], 'deleted invoking cwd starts nothing')
            check(panes('panes-deleted') == before, 'deleted invoking cwd retains tab name and plugin')
            snapshot('deleted-error')
            cwd.mkdir()
            send('\r')
        if args.missing:
            expect('did not accept', 'asynchronous spawn failure is visible on original dashboard', timeout=8)
            missing=panes('panes-missing')
            check(next(p for p in missing if p['is_plugin'] and p['id']==plugin_before['id'])['tab_name'] == plugin_before['tab_name'],
                  'known spawn rejection restores original tab name')
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
        label = configuration.get('label_'+args.tool.lower(), args.tool)
        if args.commands_case == 'literals': label = configuration['label_Variant_2.worktree']
        if args.commands_case == 'many': label = configuration['label_variant63']
        if args.commands_case == 'review-ui': label = configuration['label_'+'i'*64]
        check(replacement['tab_name'] == cwd.name+' · '+label,
              'originating stable tab has basename and configured label')
        check(replacement['terminal_command']==(str(OUT/'bin/default-shell') if args.tool=='Shell' else ' '.join([expected, *expected_argv])),
              'host command readback matches selected launch mapping')
        geometry=['pane_x','pane_y','pane_rows','pane_columns','is_floating','tab_id','tab_position']
        check(all(replacement[k]==plugin_before[k] for k in geometry),'replacement preserves pane geometry and tab')
        for old in before:
            if not old['is_plugin']:
                new=next(p for p in after if not p['is_plugin'] and p['id']==old['id'])
                stable=['id','terminal_command','pane_command','pane_cwd',*geometry]
                check(all(new.get(k)==old.get(k) for k in stable),'neighbor identity, command, cwd and geometry unchanged')
        check('Launch suppressed' not in display(),'real request never renders a fake terminal')
        launched=[r for r in records() if r['exe']==expected]
        check(len(launched)==1,'exactly one OS launch')
        check(launched[0]['argv']==expected_argv,'exact configured literal argv including empty arguments')
        check(not (cwd/'NO_EXPANSION').exists(),'no command substitution side effect')
        check(launched[0]['cwd']==expected_cwd,'literal Unicode and metacharacter cwd')
        check(replacement['pane_cwd']==expected_cwd,'host readback confirms literal cwd')
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
