#!/usr/bin/env python3
"""Compare release WASM indexing on identical disposable HOME trees in Zellij."""
import argparse
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--wasm', type=Path, required=True)
args = parser.parse_args()
artifact = args.wasm.resolve()
SEARCH_ARGS = []


def populate(home):
    # Identical contents for each artifact: 18,110 visible directories, no rules.
    for i in range(90):
        for j in range(200):
            (home / f'g{i:03d}' / f'candidate-{j:03d}').mkdir(parents=True, exist_ok=True)
    for i in range(20):
        (home / f'witness-{i:02d}').mkdir()


source = Path(__file__).with_name('verify_search.py').read_text().split('success = False')[0]
source = source.replace("WASM = ROOT / 'target/wasm32-wasip1/release/zellij-launchpad.wasm'", 'WASM = artifact')
source = source.replace('master, slave = pty.openpty()', "(probe/'deniedneedle').chmod(0o700)\nshutil.rmtree(probe)\npopulate(home)\nlayout.write_text(layout.read_text().replace(str(ROOT/'target/wasm32-wasip1/release/zellij-launchpad.wasm'), str(WASM)))\nmaster, slave = pty.openpty()")
exec(compile(source, str(Path(__file__).with_name('verify_search.py')), 'exec'))
success = False
report = {'artifact': str(WASM), 'sha256': hashlib.sha256(WASM.read_bytes()).hexdigest(),
          'directories': 18110, 'host': subprocess.check_output(['zellij', '--version'], text=True).strip()}


def visible_edit(text):
    start = time.monotonic()
    os.write(master, ('\x15' + text).encode())
    while text not in display() and time.monotonic() - start < 10:
        pump(.01)
    check(text in display(), 'edit visible: ' + text)
    return round((time.monotonic() - start) * 1000, 2)


try:
    expect('permission', 'host requests permission', 45)
    send('y')
    expect('Indexing HOME', 'worker mapping ready', 45)
    start = time.monotonic()
    report['cold_edit_ms'] = visible_edit('zznonexistent')
    expect('HOME indexed', 'index completes', 900)
    report['index_seconds_from_first_visible_indexing'] = round(time.monotonic() - start, 3)
    resize(140, 24)
    check('18110 dirs' in display(), 'exact catalogue count')
    snapshot('indexed')
    report['warm_edit_ms'] = [visible_edit('zznonexistent' + str(i)) for i in range(5)]
    report['single_key_ms'] = []
    for i in range(5):
        os.write(master, b'\x15zznonexistent')
        pump(.5)
        marker = str(i)
        start = time.monotonic()
        os.write(master, marker.encode())
        expected = 'zznonexistent' + marker
        while expected not in display() and time.monotonic() - start < 10:
            pump(.01)
        check(expected in display(), 'single-key edit visible: ' + marker)
        report['single_key_ms'].append(round((time.monotonic() - start) * 1000, 2))
    start = time.monotonic()
    os.write(master, b'\x15witness-19')
    expect('witness-19/', 'warm matching completes', 30)
    report['warm_result_ms'] = round((time.monotonic() - start) * 1000, 2)
    snapshot('matched')
    records = json.loads(cli('action', 'list-panes', '--all', '--json'))
    check(any(p.get('plugin_url') == 'file:' + str(WASM) for p in records), 'exact loaded artifact readback')
    report['loaded_panes'] = records
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
    shutil.rmtree(home)
    report.update(success=success, passed=len(checks), checks=checks, evidence=str(OUT))
    (OUT/'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
