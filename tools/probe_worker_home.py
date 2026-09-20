#!/usr/bin/env python3
"""Isolated real Zellij gate for automatic worker HOME remapping."""
from pathlib import Path
source=Path(__file__).with_name('verify_search.py').read_text().split('success = False')[0]
source=source.replace("WASM = ROOT / 'target/wasm32-wasip1/release/zellij-launchpad.wasm'", "WASM = ROOT / 'target/wasm32-wasip1/release/worker-probe.wasm'")
source=source.replace("shutil.copyfile(ROOT/'examples/launchpad.kdl', layout)", "layout.write_text((ROOT/'examples/launchpad.kdl').read_text().replace('zellij-launchpad.wasm', 'worker-probe.wasm'))\n(home/'worker-home-witness').mkdir()")
exec(compile(source, str(Path(__file__).with_name('verify_search.py')), 'exec'))
success=False
try:
    expect('permission','host permissions',45)
    send('n' if DENY else 'y')
    if DENY:
        expect('GATE denied safely','denied without scanning',10)
    else:
        deadline=time.monotonic()+90
        while 'GATE PASS' not in display() and 'BLOCKED' not in display() and time.monotonic()<deadline:
            pump(.1)
            if 'permission' in display(): send('y')
        expect('GATE PASS worker HOME mapping','workers see actual HOME despite invocation CWD',1)
        pump(2)
        check('GATE PASS' in display(),'no reload loop')
    success=True
finally:
    snapshot('last')
    (OUT/'session.ansi').write_bytes(raw)
    subprocess.run(['zellij','--session',NAME,'kill-session',NAME],env=env,capture_output=True,timeout=10)
    if child.poll() is None: child.terminate()
    child.wait(timeout=5)
    os.close(master);os.close(slave)
    (probe/'deniedneedle').chmod(0o700)
    report={'success':success,'checks':checks,'evidence':str(OUT)}
    (OUT/'gate.json').write_text(json.dumps(report,indent=2))
    print(json.dumps(report,indent=2))
