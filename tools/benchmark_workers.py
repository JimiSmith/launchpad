#!/usr/bin/env python3
"""Real PTY key-to-visible-edit timings; controlled 18,100-directory HOME.
Usage: target/verification-venv/bin/python tools/benchmark_workers.py LABEL WASM
Each latency includes Zellij, rendering, PTY transport and pyte parsing. Not a
filesystem cold-cache benchmark. No page-cache flushing or private HOME reads.
"""
import argparse
import statistics

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('label')
parser.add_argument('artifact')
benchmark_options = parser.parse_args()
label, artifact = benchmark_options.label, benchmark_options.artifact
SEARCH_ARGS = []
source = (__import__('pathlib').Path(__file__).with_name('verify_search.py')).read_text().split('success = False')[0]
source = source.replace("WASM = ROOT / 'target/wasm32-wasip1/release/zellij-launchpad.wasm'", "WASM = Path(artifact).resolve()")
source = source.replace("shutil.copyfile(ROOT/'examples/launchpad.kdl', layout)", "layout.write_text((ROOT/'examples/launchpad.kdl').read_text().replace(str(ROOT/'target/wasm32-wasip1/release/zellij-launchpad.wasm'), str(WASM)))\nfor i in range(100):\n    for j in range(180):\n        (home/f'group-{i:03d}'/f'candidate-{j:04d}-notes-project').mkdir(parents=True, exist_ok=True)")
exec(compile(source, str(__import__('pathlib').Path(__file__).with_name('verify_search.py')), 'exec'))
report = {'label':label, 'artifact':str(WASM), 'sha256':hashlib.sha256(WASM.read_bytes()).hexdigest(), 'evidence':str(OUT)}
def wait_until(predicate, timeout=240):
    end=time.monotonic()+timeout
    while not predicate() and time.monotonic()<end: pump(.002)
    assert predicate(), display()
def edit_trial(text):
    # Clear separately; edit latency starts only after clear is visible.
    os.write(master,b'\x15')
    wait_until(lambda: not any('candidate' in line for line in screen.display[3:5]),10)
    pump(.05)
    start=time.monotonic()
    os.write(master, ('\x1b[200~'+text+'\x1b[201~').encode())
    wait_until(lambda: text in '\n'.join(screen.display[3:5]),10)
    return round((time.monotonic()-start)*1000,3)
def summary(values):
    return {'samples_ms':values, 'median_ms':statistics.median(values), 'max_ms':max(values)}
success=False
try:
    expect('permission','permissions',45)
    send('y')
    # Reload may show a second permission screen; permit only this isolated probe.
    deadline=time.monotonic()+90
    while 'Indexing HOME' not in display() and time.monotonic()<deadline:
        pump(.01)
        if 'permission' in display(): os.write(master,b'y')
    check('Indexing HOME' in display(),'index starts')
    started=time.monotonic()
    cold=[]
    for i in range(12):
        check('Indexing HOME' in display(),'typing while indexing')
        cold.append(edit_trial(f'candidate{i:02d}'))
    report['during_index']=summary(cold)
    os.write(master,b'\x15')
    wait_until(lambda:'HOME indexed' in display() or 'limit reached' in display())
    report['index_observed_seconds']=round(time.monotonic()-started,3)
    snapshot('indexed')
    warm=[edit_trial(f'candidate{i:02d}') for i in range(20)]
    report['cached']=summary(warm)
    success=True
finally:
    report['success']=success
    snapshot('last')
    (OUT/'session.ansi').write_bytes(raw)
    (OUT/'benchmark.json').write_text(json.dumps(report,indent=2)+'\n')
    subprocess.run(['zellij','--session',NAME,'kill-session',NAME],env=env,capture_output=True,timeout=10)
    if child.poll() is None: child.terminate()
    child.wait(timeout=5)
    os.close(master); os.close(slave)
    (probe/'deniedneedle').chmod(0o700)
    # Keep captures/reports but remove controlled large tree.
    shutil.rmtree(home)
    print(json.dumps(report,indent=2))
