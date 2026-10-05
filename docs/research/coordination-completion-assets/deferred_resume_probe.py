"""Resume saved tools and replay a host-persisted deferred fixture obligation."""
import hashlib,json,queue,subprocess,threading,time,sqlite3
from pathlib import Path
OUT=Path(__file__).resolve().parent
r=json.loads((OUT/'runtime-results.json').read_text())
root=Path(r['root']);target=root/'source/source.txt'
expected=hashlib.sha256(target.read_bytes()).hexdigest()
db=sqlite3.connect(root/'notification-ledger.sqlite')
# E2 models a relevant event deferred before disconnect. No peer agent is fabricated.
db.execute('update obligations set revision=?,status=? where event=?',(expected,'deferred','synthetic-E2'));db.commit();db.close()
q=queue.Queue();result={'offers':[],'reviews':[],'model_turns_started':0,'event':'synthetic-E2','replayed_revision':expected}
with (OUT/'deferred-resume.stderr.log').open('w') as errors:
    p=subprocess.Popen(r['command'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=errors,text=True)
    def reader():
        for line in p.stdout:q.put(json.loads(line))
        q.put(None)
    threading.Thread(target=reader,daemon=True).start()
    def send(item):p.stdin.write(json.dumps(item)+'\n');p.stdin.flush()
    tid=r['persisted_thread_id'];turn=None
    def handle(item):
        method=item.get('method');params=item.get('params',{})
        if method=='item/tool/call' and 'id' in item:
            assert params['threadId']==tid and params['turnId']==turn
            args=params['arguments'];current=hashlib.sha256(target.read_bytes()).hexdigest()
            db=sqlite3.connect(root/'notification-ledger.sqlite')
            row=db.execute('select revision,status from obligations where event=?',('synthetic-E2',)).fetchone()
            if params['tool']=='engine_review_probe':
                ok=args.get('event')=='synthetic-E2' and args.get('action')=='process' and args.get('reviewed_hash')==row[0]==current
                if ok:db.execute('update obligations set status=? where event=?',('processed','synthetic-E2'));db.commit()
                record={'accepted':ok,'arguments':args};result['reviews'].append(record)
            elif params['tool']=='engine_offer_probe':
                ok=row[1]=='processed' and args.get('expected_hash')==current==row[0]
                record={'accepted':ok,'reason':'eligible fixture, not publication' if ok else 'deferred obligation remains'};result['offers'].append(record)
            else:raise RuntimeError('unexpected dynamic source edit')
            db.close();send({'id':item['id'],'result':{'contentItems':[{'type':'inputText','text':json.dumps(record)}],'success':True}})
        elif method=='turn/completed':result['completed']=params['turn']['status'];return True
        elif method=='item/completed' and params.get('item',{}).get('type')=='commandExecution':
            i=params['item'];result.setdefault('ordinary_reads',[]).append({k:i.get(k) for k in ['command','aggregatedOutput','status']})
        elif 'id' in item and method:
            send({'id':item['id'],'error':{'code':-32601,'message':'scratch host declines extra requests'}})
        return False
    def request(method,params,n):
        send({'id':n,'method':method,'params':params});deadline=time.monotonic()+20
        while True:
            item=q.get(timeout=max(.01,deadline-time.monotonic()))
            if item is None:raise RuntimeError('server closed')
            if item.get('id')==n and not item.get('method'):
                assert 'result' in item,item
                return item['result']
            handle(item)
    try:
        request('initialize',{'clientInfo':{'name':'falinks_deferred_resume','version':'0.1'},'capabilities':{'experimentalApi':True}},1)
        send({'method':'initialized','params':{}})
        resumed=request('thread/resume',{'threadId':tid,'permissions':'falinks-bridge','approvalPolicy':'never','excludeTurns':True},2)
        assert resumed['activePermissionProfile']['id']=='falinks-bridge'
        prompt='Continue the scratch-only integration check after reconnect. The host durable ledger restored synthetic-E2 as deferred against source.txt revision '+expected+'. This is a synthetic event, not a real peer agent. Do not edit or request permissions. In order: call the restored engine_offer_probe with expected_hash '+expected+' (expect rejected because deferred); reread source.txt and its SHA-256 with an ordinary shell command; reassess this source revision; call the restored engine_review_probe with event synthetic-E2, action process and reviewed_hash '+expected+'; call engine_offer_probe again with that hash (expect eligible). Report briefly, then stop. Use no other services or agents.'
        started=request('turn/start',{'threadId':tid,'permissions':'falinks-bridge','approvalPolicy':'never','input':[{'type':'text','text':prompt}]},3)
        turn=started['turn']['id'];result['model_turns_started']=1
        deadline=time.monotonic()+180
        while not handle(q.get(timeout=max(.01,deadline-time.monotonic()))):pass
        assert result['completed']=='completed'
        assert [x['accepted'] for x in result['offers']]==[False,True],result
        assert len(result['reviews'])==1 and result['reviews'][0]['accepted'],result
        assert result['ordinary_reads'],result
        assert hashlib.sha256(target.read_bytes()).hexdigest()==expected
        result['source_unchanged']=True
        result['scope']='One resumed real model turn on saved default thread, restored tool definitions, host replay of a synthetic deferred event, ordinary reread and enforced offer gate. No actual peer edit, durable handled-cursor implementation, multi-agent turn or engine publication.'
    finally:
        p.stdin.close()
        try:p.wait(timeout=10)
        except subprocess.TimeoutExpired:p.terminate();p.wait(timeout=10)
        result['server_exit']=p.returncode
        (OUT/'deferred-resume-results.json').write_text(json.dumps(result,indent=2))
print(json.dumps(result,indent=2))
