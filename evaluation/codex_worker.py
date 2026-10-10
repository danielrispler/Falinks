#!/usr/bin/env python3
"""Baseline-only Codex exec bridge. Host copies this into each fresh sandbox."""
import json
from pathlib import Path
import queue
import subprocess
import sys
import threading


def emit(value):
    print(json.dumps(value), flush=True)


def main():
    start = json.loads(sys.stdin.readline())
    events = queue.Queue()
    def receive():
        for line in sys.stdin:
            event = json.loads(line)
            if event.get('type')=='ok' and event.get('action')=='usage':
                continue  # Telemetry receipts must not wake a waiting model.
            events.put(event)
    threading.Thread(target=receive, daemon=True).start()
    scratch = Path('scratch')
    schema = scratch / 'response-schema.json'
    schema.write_text(json.dumps({
        'type':'object', 'additionalProperties':False,
        'properties':{
            'action':{'type':'string','enum':['message','share','ack','check','ready','wait']},
            'text':{'type':'string'}, 'milestone':{'type':'string'},
            'commit':{'type':'string'}, 'resolved':{'type':'string'}},
        'required':['action','text','milestone','commit','resolved']}))
    thread = None
    pending = [start]
    ready = False
    turn = 0
    while True:
        if not pending:
            pending.append(events.get())
        while not events.empty():
            pending.append(events.get())
        if ready:
            threading.Event().wait()  # Host terminates the process after final validation.
        turn += 1
        answer = scratch / 'answer.json'
        prompt = ('You are one agent in the Git baseline. Implement work with your peer. '
                  'Use tools to read/edit your worktree and run focused checks. '
                  'Do not write your peer worktree. You may resolve merges in integration_workspace. '
                  'Return exactly one host action as JSON; empty strings for unused fields. '
                  'Host events are queued at turn boundaries, not mid-reasoning. '
                  'Do not perform Git commits yourself; share asks the host to capture your draft. '
                  'Do not edit while share capture is pending. Use wait if awaiting peer input. '
                  'No additional subagents, apps, browser, network tools or permission escalation.\n'
                  + json.dumps(pending))
        pending = []
        binary = start['runtime_binary']
        common = ['--ignore-user-config','--ignore-rules','--json','--output-schema',str(schema),
                  '-o',str(answer),'-m',start['model']['model'],
                  '-c','model_reasoning_effort='+json.dumps(start['model']['reasoning']),
                  '-c','approval_policy="never"']
        if thread:
            argv = [binary,'exec','resume',*common,thread,'-']
        else:
            argv = [binary,'exec','--sandbox','workspace-write',*common,'-']
        process = subprocess.run(argv,input=prompt,text=True,stdout=subprocess.PIPE,stderr=sys.stderr)
        # Preserve runtime events in agent scratch; host retains stdout requests and stderr separately.
        (scratch / f'turn-{turn}.jsonl').write_text(process.stdout)
        for line in process.stdout.splitlines():
            try:
                event = json.loads(line)
            except ValueError:
                continue
            if event.get('type')=='thread.started':
                thread = event['thread_id']
            if event.get('type')=='turn.completed' and 'usage' in event:
                emit(dict(action='usage',usage=dict(turn=turn,**event['usage'])))
        if process.returncode:
            # Generic runtime failures are infrastructure failures; no invented usage-limit diagnosis.
            raise RuntimeError('Codex exited '+str(process.returncode))
        response = json.loads(answer.read_text())
        action = response['action']
        if action=='wait':
            continue
        request = {'action':action}
        for field in ('text','milestone','commit','resolved'):
            if response.get(field):
                request[field]=response[field]
        emit(request)
        ready = action=='ready'
        expected = {'share':'shared','check':'check_result'}.get(action,'ok')
        while True:
            reply=events.get()
            pending.append(reply)
            if reply['type']==expected and (expected!='ok' or reply.get('action')==action):
                break


if __name__=='__main__':
    main()
