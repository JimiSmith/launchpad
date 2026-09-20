#!/usr/bin/env python3
"""Real PTY worker stress. --fault uses a worker-faults feature build only."""
import argparse
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--fault', action='store_true')
worker_options = parser.parse_args()
SEARCH_ARGS = []
source=Path(__file__).with_name('verify_search.py').read_text().split('success = False')[0]
source=source.replace('master, slave = pty.openpty()', "layout.write_text(layout.read_text().replace('simulate_launch \"true\";', 'simulate_launch \"true\"; test_silence_worker \"true\";') if worker_options.fault else layout.read_text())\nfor i in range(2500): (home/f'candidate-{i:04d}-notes').mkdir()\n(home/'freshneedle').mkdir()\nmaster, slave = pty.openpty()")
exec(compile(source, str(Path(__file__).with_name('verify_search.py')), 'exec'))
fault = '--fault' in __import__('sys').argv
success=False
try:
    expect('permission','host permission',45);send('y')
    expect('Indexing HOME','worker mapping ready',45)
    if fault:
        expect('Worker timed out','missing response produces visible failure',20)
        send('\x15\x1b[200~still-editable\x1b[201~')
        expect('still-editable','failed worker does not freeze editing',5)
        send('\r')
        check('Launch suppressed' not in display(),'failed worker never validates a launch')
        snapshot('fault')
    else:
        check('Indexing HOME' in display(),'cold index active before burst')
        os.write(master,b'\x15nts')
        for _ in range(30):
            os.write(master,b'x\x7f');pump(.003)
        os.write(master,b'\x15freshneedle')
        expect('freshneedle/', 'latest query wins after rapid edits and backspaces',15)
        send('\x1b');pump(1)
        check('freshneedle/' not in display(),'dismissed suggestions stay dismissed during progress')
        # Start expensive matching, immediately refresh while it is in flight.
        os.write(master,b'\x15~');pump(.005);os.write(master,b'\x1b[15~')
        expect('Indexing HOME','F5 starts a new worker epoch',10)
        send('\x15freshneedle')
        expect('freshneedle/','new epoch yields fresh results',20)
        expect('HOME indexed','large controlled index completes',45)
        (home/'mousefresh').mkdir()
        click('F5 reset')
        send('\x15mousefresh')
        expect('mousefresh/','mouse reset refreshes the worker catalogue',45)
        send('\x15~\r')
        expect('Launch suppressed','async literal validation completes',10)
        send('\x1b');send('\x12');send('\t');pump(1)
        check('Copied to form' in display(),'history copy not overwritten by late replies')
        check('freshneedle/' not in display(),'history copy keeps suggestions closed')
        snapshot('stress')
        # Close while a worker request is outstanding.
        os.write(master,b'\x15~');pump(.005)
    send('\x11')
    records=json.loads(cli('action','list-panes','--all','--json'))
    check(not any(p.get('plugin_url') == 'file:'+str(WASM) for p in records),'exact pane readback confirms unload during work')
    success=True
finally:
    snapshot('last');(OUT/'session.ansi').write_bytes(raw)
    try:
        subprocess.run(['zellij','--session',NAME,'kill-session',NAME],env=env,capture_output=True,timeout=10)
    finally:
        if child.poll() is None:child.terminate()
        try:child.wait(timeout=5)
        finally:
            os.close(master);os.close(slave)
            (probe/'deniedneedle').chmod(0o700)
            shutil.rmtree(home)
    report={'success':success,'checks':checks,'passed':len(checks),'fault':fault,'evidence':str(OUT),'sha256':hashlib.sha256(WASM.read_bytes()).hexdigest()}
    (OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))
