"""Reconnect to the saved trial thread without starting another model turn."""
import json, queue, subprocess, threading, time, sqlite3
from pathlib import Path
OUT=Path(__file__).resolve().parent
r=json.loads((OUT/'runtime-results.json').read_text())
command=r['command']
q=queue.Queue()
with (OUT/'resume.stderr.log').open('w') as errors:
    p=subprocess.Popen(command,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=errors,text=True)
    def reader():
        for line in p.stdout: q.put(json.loads(line))
        q.put(None)
    threading.Thread(target=reader,daemon=True).start()
    def send(item):p.stdin.write(json.dumps(item)+'\n');p.stdin.flush()
    def request(method,params,n):
        send({'method':method,'params':params,'id':n})
        deadline=time.monotonic()+20
        while True:
            item=q.get(timeout=max(.01,deadline-time.monotonic()))
            if item is None:raise RuntimeError('server closed')
            if item.get('id')==n and not item.get('method'):
                assert 'result' in item,item
                return item['result']
    try:
        request('initialize',{'clientInfo':{'name':'falinks_resume_probe','version':'0.1'},'capabilities':{'experimentalApi':True}},1)
        send({'method':'initialized','params':{}})
        resumed=request('thread/resume',{'threadId':r['persisted_thread_id'],'permissions':'falinks-bridge','approvalPolicy':'never','excludeTurns':True},2)
        thread=resumed['thread']
        assert thread['id']==r['persisted_thread_id']
        assert resumed['activePermissionProfile']['id']=='falinks-bridge'
        path=Path(thread['path'])
        native=[];tool_names=[]
        for line in path.read_text().splitlines():
            obj=json.loads(line)
            if obj.get('type')=='session_meta':
                tool_names=[t['name'] for t in obj['payload'].get('dynamic_tools',[])]
            if obj.get('type')=='response_item':
                payload=obj.get('payload',{})
                if payload.get('name')=='apply_patch' and payload.get('type') in ('custom_tool_call','function_call'):
                    native.append({k:payload[k] for k in ['type','name','input','arguments','call_id'] if k in payload})
        # Save only our fixed-fixture native calls and tool names, never reasoning/history.
        assert set(tool_names)=={'engine_edit_probe','engine_review_probe','engine_offer_probe'},tool_names
        # Router-rejected calls may not be retained as response items. Preserve this limitation.
        native_path_verified = len(native)==1 and 'source.txt' in json.dumps(native)
        db=sqlite3.connect(Path(r['root'])/'notification-ledger.sqlite')
        processed=db.execute('select status from obligations where event=?',('E1',)).fetchone()[0]
        assert processed=='processed'
        # Independent storage fixture: preserve deferred state over close/reopen.
        db.execute('insert into obligations values(?,?,?)',('synthetic-E2','revision-R2','deferred'));db.commit();db.close()
        db=sqlite3.connect(Path(r['root'])/'notification-ledger.sqlite')
        deferred=db.execute('select revision,status from obligations where event=?',('synthetic-E2',)).fetchone()
        assert deferred==('revision-R2','deferred');db.close()
        result={'resumed_thread':thread['id'],'permission_profile':resumed['activePermissionProfile'],'restored_dynamic_tool_names':tool_names,'native_call_fixture_evidence':native,'native_path_verified':native_path_verified,'processed_obligation_restored':True,'synthetic_deferred_obligation_restored':True,'model_turns_started':0,'scope':'Fresh app-server resumed saved thread and metadata. No model resumed pending work. E2 is an independent SQLite persistence fixture, not a real deferred-agent reconnect.'}
        (OUT/'resume-results.json').write_text(json.dumps(result,indent=2))
        print(json.dumps(result,indent=2))
    finally:
        p.stdin.close()
        try:p.wait(timeout=10)
        except subprocess.TimeoutExpired:p.terminate();p.wait(timeout=10)
