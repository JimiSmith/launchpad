#!/usr/bin/env python3
"""Echo and suggestion latency for one release artifact.

Usage: target/verification-venv/bin/python tools/benchmark_latency.py LABEL WASM

`benchmark_workers.py` times a keystroke to its echo in the path field. Since
the host gained a 120ms search debounce, matching no longer runs inside that
window, so that number is now blind to the matcher: profiles that differ by 25%
in search cost measure identically there. This tool times both halves.

Each trial pastes a unique witness directory name and records:

  echo     paste -> the text visible inside the bordered path field
  suggest  paste -> the matching row visible in the suggestion region,
           which includes the debounce plus a whole-catalogue Frizbee pass

Controlled disposable HOME of 18,100 directories plus one witness per trial.
Latencies include Zellij, rendering, PTY transport and pyte parsing; they are
not a filesystem benchmark and carry no latency SLA.
"""
import argparse
import statistics
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('label')
parser.add_argument('artifact')
latency_options = parser.parse_args()
label, artifact = latency_options.label, latency_options.artifact
SEARCH_ARGS = []
WITNESSES = 24

source = Path(__file__).with_name('verify_search.py').read_text().split('success = False')[0]
source = source.replace(
    "WASM = ROOT / 'target/wasm32-wasip1/release/zellij-launchpad.wasm'",
    "WASM = Path(artifact).resolve()")
source = source.replace(
    "shutil.copyfile(ROOT/'examples/launchpad.kdl', layout)",
    "layout.write_text((ROOT/'examples/launchpad.kdl').read_text()"
    ".replace(str(ROOT/'target/wasm32-wasip1/release/zellij-launchpad.wasm'), str(WASM)))\n"
    "for i in range(100):\n"
    "    for j in range(180):\n"
    "        (home/f'group-{i:03d}'/f'candidate-{j:04d}-notes-project').mkdir(parents=True, exist_ok=True)\n"
    f"for w in range({WITNESSES}):\n"
    "    (home/f'wit{w:02d}marker').mkdir(exist_ok=True)")
exec(compile(source, str(Path(__file__).with_name('verify_search.py')), 'exec'))

report = {'label': label, 'artifact': str(WASM),
          'sha256': hashlib.sha256(WASM.read_bytes()).hexdigest(), 'evidence': str(OUT)}

INPUT_ROWS = slice(3, 5)      # inside the bordered path field
SUGGEST_ROWS = slice(5, 10)   # capped suggestion region beneath it


def rows(where):
    return '\n'.join(screen.display[where])


def wait_until(predicate, timeout=30):
    end = time.monotonic() + timeout
    while not predicate() and time.monotonic() < end:
        pump(.002)
    return predicate()


def trial(witness):
    token = f'wit{witness:02d}marker'
    os.write(master, b'\x15')
    check(wait_until(lambda: token not in rows(INPUT_ROWS) and token not in rows(SUGGEST_ROWS)),
          'previous trial cleared')
    pump(.05)
    start = time.monotonic()
    os.write(master, ('\x1b[200~' + token + '\x1b[201~').encode())
    check(wait_until(lambda: token in rows(INPUT_ROWS)), 'pasted text echoes')
    echo = (time.monotonic() - start) * 1000
    check(wait_until(lambda: token in rows(SUGGEST_ROWS)), 'witness reaches suggestions')
    return round(echo, 3), round((time.monotonic() - start) * 1000, 3)


def summary(values):
    return {'samples_ms': values, 'median_ms': round(statistics.median(values), 3),
            'mean_ms': round(statistics.mean(values), 3), 'max_ms': max(values)}


success = False
try:
    expect('permission', 'permissions', 45)
    send('y')
    # The guarded worker reload can show a second prompt; only this probe's
    # isolated session is ever approved.
    deadline = time.monotonic() + 120
    while 'Indexing HOME' not in display() and time.monotonic() < deadline:
        pump(.01)
        if 'permission' in display():
            os.write(master, b'y')
    started = time.monotonic()
    # The pane repaints only when an event makes update() return true, so an
    # idle wait can watch a stale progress line forever. Nudge once a second.
    deadline = time.monotonic() + 300
    while time.monotonic() < deadline:
        if 'HOME indexed' in display() or 'limit reached' in display():
            break
        os.write(master, b'\x15')
        end = time.monotonic() + 1.0
        while time.monotonic() < end:
            pump(.01)
    check('HOME indexed' in display() or 'limit reached' in display(), 'index completes')
    # Quantised by the one-second nudge above; not a traversal measurement.
    report['index_observed_seconds'] = round(time.monotonic() - started, 3)
    snapshot('indexed')
    os.write(master, b'\x15')
    pump(.3)
    echoes, suggests = [], []
    for witness in range(WITNESSES):
        one_echo, one_suggest = trial(witness)
        echoes.append(one_echo)
        suggests.append(one_suggest)
    report['echo'] = summary(echoes)
    report['suggest'] = summary(suggests)
    success = True
finally:
    report['success'] = success
    report['trials'] = WITNESSES
    snapshot('last')
    (OUT / 'session.ansi').write_bytes(raw)
    (OUT / 'latency.json').write_text(json.dumps(report, indent=2) + '\n')
    subprocess.run(['zellij', '--session', NAME, 'kill-session', NAME], env=env,
                   capture_output=True, timeout=10)
    if child.poll() is None:
        child.terminate()
    child.wait(timeout=5)
    os.close(master)
    os.close(slave)
    (probe / 'deniedneedle').chmod(0o700)
    shutil.rmtree(home, ignore_errors=True)
    print(json.dumps(report, indent=2))
