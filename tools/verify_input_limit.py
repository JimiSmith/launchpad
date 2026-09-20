#!/usr/bin/env python3
"""Live input-limit/matcher memory regression at the 20,000-directory cap.
Run after both release builds: target/verification-venv/bin/python tools/verify_input_limit.py
HOME is always a disposable fixture.
"""
from pathlib import Path

ASCII_QUERY = 'a' * 100
UNICODE_QUERY = '修' * 30 + 'a' * 70
LONG_PATH = 'b' * 110 + '-longneedle'


def populate(home):
    for name in (ASCII_QUERY, UNICODE_QUERY, LONG_PATH, 'freshneedle'):
        (home / name).mkdir()
    # Root-level witnesses are discovered before these descendants hit the cap.
    for i in range(100):
        for j in range(200):
            (home / f'g{i:03d}' / f'candidate-{j:03d}').mkdir(parents=True, exist_ok=True)


source = Path(__file__).with_name('verify_search.py').read_text().split('success = False')[0]
source = source.replace('master, slave = pty.openpty()', 'populate(home)\nmaster, slave = pty.openpty()')
exec(compile(source, str(Path(__file__).with_name('verify_search.py')), 'exec'))
success = False
report = {'artifact': str(WASM)}


def input_text():
    return next((line for line in screen.display if '│ › ' in line), '')


def expect_input(text, label, timeout=30):
    deadline = time.monotonic() + timeout
    while text not in input_text() and time.monotonic() < deadline:
        pump(.05)
    check(text in input_text(), label)


def paste(text):
    send('\x15')
    send('\x1b[200~' + text + '\x1b[201~')


try:
    expect('permission', 'isolated host permission prompt', 45)
    send('y')
    expect('Launchpad', 'dashboard ready', 45)
    resize(160, 36)
    # Avoid matching every path on each progress tick while constructing the
    # large fixture catalogue; measured queries start after indexing settles.
    paste('zzzznonexistent')
    expect('20000 dirs', 'catalogue reaches 20,000-directory capacity', 900)
    expect('limit reached', 'catalogue cap is visible', 30)
    snapshot('indexed')

    send('\x15')
    # Ordinary key events, not bracketed paste.
    send(ASCII_QUERY)
    expect_input(ASCII_QUERY, 'all 100 typed characters are retained')
    expect(ASCII_QUERY + '/', '100-character typed query returns a real match', 30)
    send('Z')
    check(ASCII_QUERY in input_text() and 'Z' not in input_text(), '101st typed character is ignored')
    snapshot('100-query')

    paste('a' * 4096)
    expect(ASCII_QUERY + '/', '4096-character paste is capped and matched', 30)
    check(input_text().count('a') == 100, 'oversized paste leaves exactly 100 ASCII characters')
    snapshot('oversized-paste')
    paste('freshneedle')
    expect('freshneedle/', 'worker responds to short query after oversized paste', 30)
    snapshot('recovered')

    # 100 Unicode scalars, 160 UTF-8 bytes: exercises the Unicode matcher path.
    paste(UNICODE_QUERY + 'Z' * 4096)
    expect_input(UNICODE_QUERY, 'Unicode paste retains exactly 100 scalars')
    expect(UNICODE_QUERY + '/', '100-scalar Unicode prefix matches after oversized paste', 30)
    check('Z' not in input_text(), 'Unicode paste cannot insert a 101st scalar')
    snapshot('unicode-query')
    paste('freshneedle')
    expect('freshneedle/', 'worker responds after Unicode maximum query', 30)
    for scalar in ('修', '🦀'):
        paste(scalar * 100 + 'Z' * 4096)
        pump(2)
        check('Z' not in input_text(), f'100 {scalar} scalars reject excess input')
        paste('freshneedle')
        expect('freshneedle/', f'worker responds after {len((scalar * 100).encode())}-byte Unicode query', 30)

    paste('longneedle')
    expect(LONG_PATH + '/', 'short query discovers a path longer than the input cap', 30)
    send('\t')
    expect('~/' + LONG_PATH, 'validated completion is never truncated', 10)
    send('Z')
    check('~/' + LONG_PATH in input_text() and 'Z' not in input_text(), 'long completion is preserved when insertion is blocked')
    send('\r')
    expect('Launch suppressed', 'long completed path still validates and launches', 10)
    snapshot('long-completion')
    send('\x1b')
    paste('freshneedle')
    expect('freshneedle/', 'worker remains responsive after long-path validation', 30)
    pump(1)
    check('Worker timed out' not in display(), 'no worker timeout at catalogue capacity')
    send('\x11')
    records = json.loads(cli('action', 'list-panes', '--all', '--json'))
    check(not any(p.get('plugin_url') == 'file:' + str(WASM) for p in records), 'exact pane readback confirms unload')
    success = True
finally:
    snapshot('last')
    (OUT/'session.ansi').write_bytes(raw)
    subprocess.run(['zellij', '--session', NAME, 'kill-session', NAME], env=env, capture_output=True, timeout=10)
    if child.poll() is None:
        child.terminate()
    child.wait(timeout=5)
    os.close(master)
    os.close(slave)
    (probe/'deniedneedle').chmod(0o700)
    shutil.rmtree(home)
    artifact = Path(report['artifact'])
    report.update(success=success, checks=checks, passed=len(checks), evidence=str(OUT), sha256=hashlib.sha256(artifact.read_bytes()).hexdigest())
    (OUT/'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
