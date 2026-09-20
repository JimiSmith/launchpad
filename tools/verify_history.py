#!/usr/bin/env python3
"""Shared history across real Zellij 0.45.1 panes/sessions; only owned executable fixtures.
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
args = argparse.Namespace(tool='Hermes', layout='tiled', deny=False, missing=False,
                          race=False, different_cwd=True, exit_code=0)
OUT = ROOT / 'target/history-persistence' / ('live-' + uuid.uuid4().hex[:8])
OUT.mkdir(parents=True)
NAME = OUT.name[-8:]
for d in ['home', 'bin', 'cache', 'config', 'data', 'runtime', 'sock', 'tmp']:
    (OUT/d).mkdir(mode=0o700)
home = OUT/'home'
cwd = home/"space 修理 it's literal; $HOME"
cwd.mkdir()
for n in range(12): (home/f'project-{n:02}').mkdir()
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
           ZELLIJ_SOCKET_DIR=str(ROOT/'target/hp-sock'/NAME), TMPDIR=str(OUT/'tmp'))
fixture_config = {'commands': 'claude,codex,copilot,hermes'}
for name in ['Claude','Codex','Copilot','Hermes']:
    fixture_config['command_'+name.lower()] = name.lower()
    fixture_config['label_'+name.lower()] = name

def kdl_config(configuration):
    return '; '.join(k+' '+json.dumps(v, ensure_ascii=False) for k,v in configuration.items())+(';' if configuration else '')

aliases = {
    'fixture-commands': fixture_config,
    'renamed-commands': {'commands': 'hermes', 'command_hermes': 'claude', 'arguments_hermes': "--changed ''", 'label_hermes': 'Renamed Worktree'},
    'removed-commands': {},
    'invalid-commands': {'commands': 'hermes', 'command_hermes': 'hermes', 'arguments_hermes': "'bad"},
}
config = OUT/'config.kdl'
config.write_text((ROOT/'examples/locked.kdl').read_text() +
                  f'\nsession_name "{NAME}"\ndefault_shell "{OUT}/bin/default-shell"\n')
with config.open('a') as f:
    f.write('\nplugins {\n')
    for alias, values in aliases.items():
        f.write(f'  {alias} location="file:{WASM}" {{ {kdl_config(values)} }}\n')
    f.write('}\n')
plugin = f'pane name="launchpad" focus=true {{ plugin location="file:{WASM}" {{ {kdl_config(fixture_config)} }}; }}'
neighbor = f'pane name="neighbor" command="{OUT}/bin/neighbor"'
if args.layout == 'only':
    layout = f'layout {{ pane {{ plugin location="file:{WASM}" {{ {kdl_config(fixture_config)} }}; }}; }}'
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
    if command[:2] == ('action','launch-plugin') and command[-1] in aliases:
        command = ('action','launch-or-focus-plugin', *command[2:])
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
def reopen(configuration=None, wasm=WASM):
    screen.reset()
    command = ['action','launch-plugin','--in-place','--close-replaced-pane','--skip-plugin-cache']
    if configuration: command += ['--configuration', configuration]
    cli(*command, 'file:'+str(wasm) if configuration or wasm != WASM else 'fixture-commands')
    if not configuration and wasm == WASM:
        # An alias can focus an existing rejected dashboard; reset before waiting
        # for status text hidden by its transient rejection message.
        send('\x1b[15~')
    target = 'HOME indexed'
    deadline = time.monotonic()+45
    approvals = 0
    while target not in display() and time.monotonic() < deadline:
        pump()
        if 'Allow? (y/n)' in display() and approvals < 2:
            send('y'); approvals += 1
    check(target in display(), 'dashboard ready '+str(configuration))
    if not configuration and wasm == WASM:
        check('[ Shell ]' in display(), 'alias dashboard reset preserves initial Shell')

def launch(path, tool='Shell'):
    old_count = len(records())
    send('\x10\x15')
    send('\x1b[200~'+path+'\x1b[201~')
    send('\x14')
    for _ in range(['Shell','Claude','Codex','Copilot','Hermes'].index(tool)): send('\x1b[C')
    send('\r')
    expected = 'default-shell' if tool == 'Shell' else tool.lower()
    expect('FIXTURE_READY '+expected, 'fixture launch '+path)
    check(len(records()) == old_count+1, 'exactly one launch '+path)
    return records()[-1]

second = None
second_fds = None
second_raw = bytearray()
second_name = NAME+'b'
def cli_second(*command):
    if command[:2] == ('action','launch-plugin') and command[-1] in aliases:
        command = ('action','launch-or-focus-plugin', *command[2:])
    result = subprocess.run([ZELLIJ,'--session',second_name,*command],env=env,cwd=home,
                            capture_output=True,text=True,timeout=10)
    assert result.returncode == 0, result.stderr
    return result.stdout

def second_ready():
    deadline = time.monotonic()+45
    text = ''
    while time.monotonic() < deadline:
        try: text = cli_second('action','dump-screen')
        except AssertionError: pump(); continue
        if 'HOME indexed' in text: break
        if 'Allow? (y/n)' in text: cli_second('action','write-chars','y')
        pump()
    (OUT/'second-dashboard.txt').write_text(text)
    check('HOME indexed' in text, 'fresh second session worker ready')
    return text

success=False
try:
    expect('permission', 'permission prompt')
    send('y')
    expect('HOME indexed', 'worker HOME ready after remount/reload')
    send('\x15')
    send("\x1b[200~~/space 修理 it's literal; $HOME\x1b[201~")
    send('\x14')
    for _ in range(4): send('\x1b[C')
    snapshot('01-before-launch')
    send('\r')
    expect('FIXTURE_READY hermes', 'harmless Hermes fixture actually launches')
    history_files = list((OUT/'cache').rglob('history.json'))
    check(len(history_files) == 1, 'one URL-shared history.json exists after replacement')
    history_file = history_files[0]
    data = json.loads(history_file.read_text())
    check(data['version'] == 2 and len(data['entries']) == 1, 'versioned history contains launch')
    check(data['entries'][0]['path'] == str(cwd) and data['entries'][0]['tool'] == 'hermes', 'literal path and last tool persisted')
    # Same URL, different actual plugin configuration: ID, not label/exe, binds replay.
    cli('action','launch-plugin','--in-place','--close-replaced-pane','--skip-plugin-cache','renamed-commands')
    expect('HOME indexed','renamed config ready at same URL')
    expect('Renamed Worktree','current configured label displayed')
    replay_origin = next(p for p in panes('replay-origin') if p.get('plugin_url') == 'renamed-commands')
    send('\x12\r')
    expect('FIXTURE_READY claude','history ID resolves to current executable')
    replay_panes = panes('replay-named')
    check(any(p['tab_id'] == replay_origin['tab_id'] and p['tab_name'] == cwd.name+' · Renamed Worktree' for p in replay_panes),
          'history replay names originating stable tab using current configured label')
    check(records()[-1]['argv'] == ['--changed',''] and records()[-1]['cwd'] == str(cwd), 'current args and exact historical cwd used on replay')
    check(json.loads(history_file.read_text())['entries'][0]['tool'] == 'hermes','history stores ID, not new label or executable')
    cli('action','launch-plugin','--in-place','--close-replaced-pane','--skip-plugin-cache','removed-commands')
    expect('HOME indexed','removed config ready at same URL')
    count = len(records())
    send('\x12\r')
    expect('unavailable','removed historical command cannot replay')
    send('\t')
    expect('Choose a tool','removed history directory is copied without substituting Shell')
    send('\r')
    expect('unavailable','copied unavailable ID still cannot launch')
    check(len(records()) == count,'unavailable replay and copy start no process')
    snapshot('configured-removed')
    cli('action','launch-plugin','--in-place','--close-replaced-pane','--skip-plugin-cache','invalid-commands')
    expect('~/space','invalid config still has working directory search')
    expect('Config error','invalid current command definition is visible')
    send('\x12\r')
    expect('unavailable','invalid current definition cannot replay saved ID')
    check(len(records()) == count,'invalid config never falls back')
    # Unsupported old cache is disposable, never imported or executed.
    data = json.loads(history_file.read_text())
    old_row = dict(data['entries'][0], tool='Hermes')
    journal = history_file.parent/'history.d'
    authoritative = journal/(old_row['id']+'.json')
    authoritative.write_text(json.dumps({'version':1,'id':old_row['id'],'entry':old_row}))
    reopen()
    check(json.loads(history_file.read_text())['entries'] == [], 'WASI ignores old journal records')
    check(len(records()) == count, 'old records start no process')
    shutil.rmtree(journal)  # Owned harness cache only.
    history_file.write_text(json.dumps({'version':1,'entries':[old_row]}))
    reopen()
    check(json.loads(history_file.read_text())['entries'] == [], 'WASI ignores old projection without a journal')
    check(len(records()) == count, 'old projection starts no process')
    launch("~/space 修理 it's literal; $HOME", 'Hermes')
    data = json.loads(history_file.read_text())
    check(data['version'] == 2 and data['entries'][0]['tool'] == 'hermes', 'real launch writes v2 after unsupported cache')
    check(json.loads((journal/(data['entries'][0]['id']+'.json')).read_text())['version'] == 2, 'new journal is v2')
    snapshot('configured-v2-only')
    cli('action','launch-plugin','--in-place','--close-replaced-pane','--skip-plugin-cache', 'fixture-commands')
    expect('HOME indexed', 'reopened plugin worker ready')
    expect('1 events', 'history is visible before inducing journal read failure', timeout=5)
    # An unreadable authoritative record is an I/O failure, not empty history.
    authoritative = history_file.parent/'history.d'/(data['entries'][0]['id']+'.json')
    saved_projection = history_file.read_bytes()
    saved_record = authoritative.read_bytes()
    saved_mode = authoritative.stat().st_mode
    authoritative.chmod(0o000)
    try:
        send('\x1b[15~')
        expect('History unavailable', 'unreadable journal refresh reports failure', timeout=8)
        check(history_file.read_bytes() == saved_projection, 'failed journal read preserves good projection byte-for-byte')
        check('1 events' in display() and 'No recent launches' not in display(), 'failed refresh retains previously visible history')
        snapshot('02-unreadable-history')
        launched = launch('~/project-00')
        check(launched['cwd'] == str(home/'project-00'), 'unreadable history still permits the actual host launch')
        check(history_file.read_bytes() == saved_projection, 'launch with unreadable authority does not replace projection')
    finally:
        authoritative.chmod(saved_mode)
    check(authoritative.read_bytes() == saved_record, 'unreadable authoritative data survives unchanged')
    reopen()
    check(history_file.read_bytes() == saved_projection and '1 events' in display(), 'restoring permissions recovers persisted history')
    send('\x12\t')
    expect('Copied to form', 'reopened history is selectable and copied using existing UI', timeout=5)
    snapshot('02-reopened-history')
    replay_count = len([r for r in records() if r['exe']=='hermes' and r['cwd']==str(cwd)])
    send('\r')
    expect('FIXTURE_READY hermes', 'saved tool replays through real host launch')
    check(len([r for r in records() if r['exe']=='hermes' and r['cwd']==str(cwd)]) == replay_count + 1, 'replay validates and launches exact saved cwd')
    check(len(json.loads(history_file.read_text())['entries']) == 1, 'replaying same directory does not duplicate history')
    cli('action','launch-plugin','--in-place','--close-replaced-pane','--skip-plugin-cache', 'fixture-commands')
    expect('HOME indexed', 'reopened for clear')
    send('\x12\x0c')
    expect('Clear history?', 'clear requires confirmation')
    send('\x1b')
    check(len(json.loads(history_file.read_text())['entries']) == 1, 'cancel clear keeps disk history')
    send('\x0c\x0c')
    expect('No recent launches', 'confirmed clear changes UI only after persistence', timeout=5)
    check(json.loads(history_file.read_text())['entries'] == [], 'confirmed clear is persisted')
    send('\x1b[15~')
    expect('HOME indexed', 'refresh after clear')
    check('No recent launches' in display(), 'clear survives F5 refresh')
    snapshot('03-cleared')
    (OUT/'bin/copilot').unlink()
    send('\x14')
    for _ in range(3): send('\x1b[C')
    send('\r')
    expect('did not accept', 'known missing-command rejection stays on dashboard', timeout=8)
    check(json.loads(history_file.read_text())['entries'] == [], 'known host rejection rolls back only its persisted attempt')
    snapshot('04-rejected')
    for n in range(12):
        reopen()
        launched = launch(f'~/project-{n:02}')
        check(launched['cwd'] == str(home/f'project-{n:02}'), 'literal project cwd')
    reopen()
    launch('~/project-06', 'Codex')
    rows = json.loads(history_file.read_text())['entries']
    expected_paths = [str(home/f'project-{n:02}') for n in [6,11,10,9,8,7,5,4,3,2]]
    check([r['path'] for r in rows] == expected_paths, 'ten unique directories in newest-first order')
    check(rows[0]['tool'] == 'codex', 'duplicate directory remembers latest tool')
    journal = history_file.parent/'history.d'
    check(len(list(journal.iterdir())) <= 11, 'journal compacts to ten rows plus clear watermark')
    first_event = journal/(rows[0]['id']+'.json')
    event = json.loads(first_event.read_text())
    event['entry']['opened_at'] -= 7200
    first_event.write_text(json.dumps(event))
    reopen()
    expect('2h ago', 'stored timestamp age survives reload')
    (home/'project-06').rmdir()
    before_records = len(records())
    send('\x12\r')
    expect('Invoking directory unavailable; no fallback.', 'deleted invoking/history directory revalidated before replay')
    check(len(records()) == before_records, 'deleted directory launches nothing')
    send('\x1b[3~')
    check(len(json.loads(history_file.read_text())['entries']) == 9, 'delete-selected persists')
    check(all(r['path'] != str(home/'project-06') for r in json.loads(history_file.read_text())['entries']), 'delete targets selected stable entry')
    snapshot('05-aged-and-deleted')
    saved = history_file.read_bytes()
    sentinel = OUT/'cache-sentinel'
    sentinel.write_text('KEEP')
    history_file.unlink()
    history_file.symlink_to(sentinel)
    send('\x0c\x0c')
    expect('History change failed', 'clear failure is visible, not reported as success')
    check('No recent launches' not in display(), 'failed clear retains displayed rows')
    snapshot('06-clear-failed')
    send('\x10')
    # The existing selection is Shell after reopen, regardless of history tool.
    launched = launch('~/project-00')
    check(launched['cwd'] == str(home/'project-00'), 'cache write failure does not block actual launch')
    check(sentinel.read_text() == 'KEEP' and history_file.is_symlink(), 'unsafe history target is never overwritten')
    history_file.unlink()
    history_file.write_bytes(saved)
    for payload in [b'{corrupt', b'x'*200000, None]:
        if payload is None: history_file.unlink()
        else: history_file.write_bytes(payload)
        reopen()
        check(len(json.loads(history_file.read_text())['entries']) == 9, 'missing/corrupt projection recovers from bounded journal')
    before_cache = {p.name:p.read_bytes() for p in journal.iterdir()}
    before_json = history_file.read_bytes()
    reopen('simulate_launch=true')
    check('No recent launches' in display(), 'simulation does not load persistent history')
    send('\x14\r')
    expect('Launch suppressed', 'the harness mode never spawns a process')
    check(history_file.read_bytes() == before_json and {p.name:p.read_bytes() for p in journal.iterdir()} == before_cache, 'simulation never changes persistent history')
    reopen()
    snapshot('07-restored-real-history')
    # A second actual PTY/client and Zellij server, sharing ONLY owned cache/config/data.
    import threading
    second_master, second_slave = pty.openpty()
    second_fds = (second_master, second_slave)
    fcntl.ioctl(second_slave, termios.TIOCSWINSZ, struct.pack('HHHH',36,120,0,0))
    config2 = OUT/'config-second.kdl'
    config2.write_text(config.read_text().replace('session_name "'+NAME+'"', 'session_name "'+second_name+'"'))
    layout2 = f'layout {{ pane {{ plugin location="file:{WASM}" {{ {kdl_config(fixture_config)} }}; }}; }}'
    second = subprocess.Popen([ZELLIJ,'--config',str(config2),'--layout-string',layout2],
                              stdin=second_slave,stdout=second_slave,stderr=second_slave,
                              env=env,cwd=home,preexec_fn=setup)
    def drain_second():
        while second.poll() is None:
            if select.select([second_master],[],[],.1)[0]:
                try: data = os.read(second_master,65536)
                except OSError: break
                if not data: break
                second_raw.extend(data)
    reader = threading.Thread(target=drain_second, daemon=True)
    reader.start()
    second_text = second_ready()
    check('9 events' in second_text and '~/project-11' in second_text, 'new session same URL/cache loads persisted history')
    send('\x12\x0c\x0c')
    expect('No recent launches', 'clear first session before concurrency')
    send('\x10\x15')
    send('\x1b[200~~/project-00\x1b[201~')
    send('\x14')
    second_panes = json.loads(cli_second('action','list-panes','--all','--json'))
    second_id = 'plugin_'+str(next(p for p in second_panes if p.get('plugin_url') in {'file:'+str(WASM), 'fixture-commands'} and not p['is_suppressed'])['id'])
    cli_second('action','send-keys','--pane-id',second_id,'Ctrl p','Ctrl u')
    cli_second('action','write-chars','--pane-id',second_id,'~/project-01')
    cli_second('action','send-keys','--pane-id',second_id,'Ctrl t')
    check('~/project-01' in cli_second('action','dump-screen'), 'second pane literal launch prepared')
    first_id = 'plugin_'+str(next(p for p in panes('concurrent-before') if p.get('plugin_url') in {'file:'+str(WASM), 'fixture-commands'} and not p['is_suppressed'])['id'])
    count_before = len(records())
    from concurrent.futures import ThreadPoolExecutor
    gate = threading.Barrier(2)
    def submit(command, pane):
        gate.wait()
        command('action','send-keys','--pane-id',pane,'Enter')
    with ThreadPoolExecutor(max_workers=2) as pool:
        a = pool.submit(submit,cli,first_id)
        b = pool.submit(submit,cli_second,second_id)
        a.result(); b.result()
    deadline = time.monotonic()+10
    while len(records()) < count_before+2 and time.monotonic()<deadline: pump()
    check(len(records()) == count_before+2, 'two concurrent sessions actually launched fixtures')
    check({r['cwd'] for r in records()[count_before:]} == {str(home/'project-00'),str(home/'project-01')}, 'concurrent launches preserve both literal cwd values')
    reopen()
    rows = json.loads(history_file.read_text())['entries']
    check({r['path'] for r in rows} == {str(home/'project-00'),str(home/'project-01')}, 'immutable records merge concurrent sessions without lost updates')
    check(len(list((OUT/'cache').rglob('history.json'))) == 1, 'both sessions use same URL cache file')
    snapshot('08-concurrent-merged')
    send('\x12\x0c\x0c')
    expect('No recent launches', 'concurrent history cleared')
    cli_second('action','launch-plugin','--in-place','--close-replaced-pane','--skip-plugin-cache','fixture-commands')
    check('No recent launches' in second_ready(), 'clear persists across sessions')
    launch('~/project-00')
    # Remove only this disposable URL cache while no first-session plugin holds it.
    shutil.rmtree(history_file.parent)
    reopen()
    check('No recent launches' in display(), 'deleted cache reopens as empty without blocking HOME')
    check(json.loads(history_file.read_text())['entries'] == [], 'cache deletion recreates empty versioned projection')
    snapshot('09-cache-recreated')
    journal = history_file.parent/'history.d'
    journal.mkdir(exist_ok=True)
    (journal/'bad.json').write_text('{')
    (journal/'oversized.json').write_bytes(b'x'*33000)
    (journal/'future.json').write_text('{"version":99,"id":"future","entry":null}')
    stale_temp = history_file.parent/'history-abandoned.tmp'
    stale_temp.write_text('incomplete')
    os.utime(stale_temp,(time.time()-120,time.time()-120))
    live_temp = history_file.parent/'history-active.tmp'
    live_temp.write_text('active')
    reopen()
    check(not stale_temp.exists() and live_temp.read_text() == 'active', 'WASI reclaims abandoned temp without deleting active temp')
    check('No recent launches' in display(), 'corrupt oversized unknown-version journal records ignored safely')
    live_temp.unlink()
    launch('~/project-00')
    reopen()
    check(len(json.loads(history_file.read_text())['entries']) == 1, 'ordinary launch survives corrupt journal records')
    alternate = OUT/'alternate.wasm'
    shutil.copyfile(WASM, alternate)
    original = history_file.read_bytes()
    reopen(wasm=alternate)
    check('No recent launches' in display(), 'different plugin URL has separate empty history')
    check(len(list((OUT/'cache').rglob('history.json'))) == 2, 'different URL creates distinct cache history')
    check(history_file.read_bytes() == original, 'different URL does not modify original history')
    reopen()
    check('1 events' in display(), 'original URL retains its own recent directory')
    snapshot('10-url-isolation')
    check(hashlib.sha256(WASM.read_bytes()).hexdigest() == ARTIFACT_HASH, 'actual tested artifact remained unchanged')
    success=True
finally:
    if second is not None:
        subprocess.run([ZELLIJ,'--session',second_name,'kill-session',second_name],env=env,capture_output=True,timeout=10)
        if second.poll() is None: second.terminate()
        try: second.wait(timeout=5)
        except subprocess.TimeoutExpired:
            second.kill(); second.wait(timeout=5)
        reader.join(timeout=2)
        (OUT/'second-session.ansi').write_bytes(second_raw)
    if second_fds:
        for fd in second_fds: os.close(fd)
    snapshot('last')
    (OUT/'session.ansi').write_bytes(raw)
    subprocess.run([ZELLIJ,'--session',NAME,'kill-session',NAME],env=env,capture_output=True,timeout=10)
    pump(.2)
    if child.poll() is None: child.terminate()
    try: child.wait(timeout=5)
    except subprocess.TimeoutExpired:
        child.kill(); child.wait(timeout=5)
    os.close(master);os.close(slave)
    report={'success':success,'checks':checks,'passed':len(checks),'case':vars(args),
            'evidence':str(OUT),'sha256':ARTIFACT_HASH}
    (OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))
