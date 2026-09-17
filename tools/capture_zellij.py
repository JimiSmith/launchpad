#!/usr/bin/env python3
"""Replay actual Zellij PTY records in xterm.js; never redraw text snapshots.
Dev only: playwright Python package + Chromium. Downloads pinned xterm.js 6.0.0
into target on first run. Usage: python tools/capture_zellij.py target/zj-<id>
"""
import functools
import http.server
import json
from pathlib import Path
import shutil
import sys
import tarfile
import threading
import urllib.request

from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[1]
OUT = Path(sys.argv[1]).resolve()
assert OUT.is_relative_to(ROOT/'target') and (OUT/'report.json').is_file()
archive = ROOT/'target/xterm-xterm-6.0.0.tgz'
if not archive.exists():
    urllib.request.urlretrieve('https://registry.npmjs.org/@xterm/xterm/-/xterm-6.0.0.tgz', archive)
with tarfile.open(archive) as tar:
    js = tar.extractfile('package/lib/xterm.js').read().decode()
    css = tar.extractfile('package/css/xterm.css').read().decode()
html = '''<!doctype html><meta charset="utf-8"><style>
body{margin:0;padding:16px;background:#101310}#terminal{width:max-content}
''' + css + '</style><div id="terminal"></div><script>' + js + '''</script><script>
window.ready=false;
(async()=>{
 const name=new URLSearchParams(location.search).get('snapshot');
 window.term=new Terminal({cols:80,rows:24,fontSize:16,
  fontFamily:'"DejaVu Sans Mono",monospace',lineHeight:1.2,
  theme:{background:'#171b17',foreground:'#e4e8dd'},allowProposedApi:true});
 term.open(document.getElementById('terminal'));
 const events=await (await fetch(name+'.json')).json();
 let pending='';
 const flush=async()=>{if(pending){await new Promise(r=>term.write(pending,r));pending='';}};
 for(const event of events){
  if(event.resize){await flush();term.resize(...event.resize);}
  else pending+=event.write;
 }
 await flush();window.ready=true;
})();
</script>'''
(OUT/'replay.html').write_text(html)
class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass
server = http.server.ThreadingHTTPServer(('127.0.0.1',0),functools.partial(QuietHandler,directory=OUT))
thread = threading.Thread(target=server.serve_forever,daemon=True)
thread.start()
shots = sys.argv[2:] or ['01-initial-80x24','06-unicode-80x24','07-wide-120x36',
         '09-narrow-40x10','10-guard-30x8','13-split-before-key-quit']
try:
    with sync_playwright() as pw:
        browser = pw.chromium.launch(executable_path=shutil.which('chromium'),headless=True,
                                     args=['--no-sandbox'])
        page = browser.new_page(viewport={'width':1400,'height':900},device_scale_factor=1)
        for name in shots:
            page.goto(f'http://127.0.0.1:{server.server_port}/replay.html?snapshot={name}')
            page.wait_for_function('window.ready === true',timeout=30000)
            page.locator('#terminal').screenshot(path=str(OUT/(name+'.png')))
        browser.close()
finally:
    server.shutdown()
print(json.dumps({'screenshots':[str(OUT/(name+'.png')) for name in shots]},indent=2))
