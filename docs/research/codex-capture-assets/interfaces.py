#!/usr/bin/env python3
"""Installed app-server notification/process boundary check, no inference."""
import json, queue, subprocess, sys, threading, time
from pathlib import Path
root=Path(sys.argv[1]); root.mkdir(parents=True,exist_ok=True)
log=(root/'app-server.jsonl').open('w')
err=(root/'app-server.stderr').open('w')
p=subprocess.Popen(['codex','app-server','--stdio','-c','analytics.enabled=false'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=err,text=True,bufsize=1)
q=queue.Queue()
def reader():
    for line in p.stdout:
        log.write(line); log.flush()
        q.put(json.loads(line))
threading.Thread(target=reader,daemon=True).start()
def send(method,params,ident=None):
    msg={'method':method,'params':params}
    if ident is not None: msg['id']=ident
    p.stdin.write(json.dumps(msg)+'\n'); p.stdin.flush()
def until(test,timeout=30):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        m=q.get(timeout=max(.01,end-time.monotonic()))
        if test(m): return m
    raise TimeoutError()
def req(method,params,ident):
    send(method,params,ident)
    m=until(lambda m:m.get('id')==ident)
    assert 'error' not in m,m
    return m['result']
try:
    initialized=req('initialize',{'clientInfo':{'name':'falinks-capture-research','version':'1'},'capabilities':{'experimentalApi':True}},1)
    send('initialized',{})
    f=root/'watched.txt'; f.write_text('base')
    req('fs/watch',{'watchId':'capture-trial','path':str(f.resolve())},2)
    f.write_text('external-write')
    changed=until(lambda m:m.get('method')=='fs/changed')
    assert 'threadId' not in changed['params'] and 'turnId' not in changed['params']
    req('fs/unwatch',{'watchId':'capture-trial'},3)
    if '--turn' in sys.argv:
        fixture=root/'fixture.ts'
        fixture.write_text('export function ready(): number {\n  return 1;\n}\n\nexport function unfinished(): number {\n  return ; // UNFINISHED_B\n}\n')
        thread=req('thread/start',{'cwd':str(root.resolve()),'sandbox':'workspace-write','approvalPolicy':'never','ephemeral':True},10)
        tid=thread['thread']['id']
        req('turn/start',{'threadId':tid,'input':[{'type':'text','text':'Use apply_patch to change only ready() from return 1 to return 2 in fixture.ts. Preserve unfinished() verbatim. Do not run shell commands or repair the draft. Finish with READY.'}]},11)
        until(lambda m:m.get('method')=='turn/completed',180)
        expected='export function ready(): number {\n  return 2;\n}\n\nexport function unfinished(): number {\n  return ; // UNFINISHED_B\n}\n'
        assert fixture.read_text()==expected
    # Standalone process control is not a guarantee about model-spawned descendants.
    script="from pathlib import Path; import sys,time; p=Path(sys.argv[1]); p.write_text('first'); time.sleep(30); p.write_text('second')"
    send('command/exec',{'command':[sys.executable,'-c',script,str(root/'process.txt')],'cwd':str(root.resolve()),'processId':'trial-writer','streamStdoutStderr':True},4)
    end=time.monotonic()+20
    while not (root/'process.txt').exists() and time.monotonic()<end: time.sleep(.05)
    assert (root/'process.txt').read_text()=='first'
    (root/'process-capture.txt').write_bytes((root/'process.txt').read_bytes())
    req('command/exec/terminate',{'processId':'trial-writer'},5)
    completed=until(lambda m:m.get('id')==4)
    assert (root/'process.txt').read_text()=='first'
    (root/'interface-results.json').write_text(json.dumps({'initialize':initialized,'watch_event':changed,'command_response':completed,'terminated_before_second_write':True},indent=2))
    print('PASS: app-server handshake, unattributed filesystem notification, standalone process termination')
finally:
    p.terminate(); p.wait(timeout=10); log.close(); err.close()
