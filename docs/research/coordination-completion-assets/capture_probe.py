"""Scratch evidence: pin completed Git revisions while later live edits install."""
import hashlib, json, os, subprocess, tempfile, threading, time
from pathlib import Path
OUT = Path(__file__).resolve().parent
ROOT = Path(tempfile.mkdtemp(prefix='capture-',dir=OUT))
LIVE, STORE, SLOT = ROOT/'live', ROOT/'objects.git', ROOT/'test'
LIVE.mkdir()
subprocess.run(['git','init','--bare',str(STORE)],check=True,capture_output=True)
env=dict(os.environ,GIT_DIR=str(STORE),GIT_INDEX_FILE=str(ROOT/'index'))
def git(*args, input=None):
    return subprocess.check_output(['git',*args],input=input,env=env,stderr=subprocess.PIPE).decode().strip()
def tree_for(previous, updates):
    git('read-tree', previous if previous else '--empty')
    for path, contents in updates.items():
        blob=git('hash-object','-w','--stdin',input=contents.encode())
        git('update-index','--add','--cacheinfo',f'100644,{blob},{path}')
    tree=git('write-tree')
    args=['-c','user.name=Scratch','-c','user.email=scratch@example.invalid','commit-tree',tree,'-m','completed controlled source revision']
    if previous: args+=['-p',previous]
    return git(*args)
initial={'price.txt':'quote(amount)\n','caller.txt':'quote(100)\n','unchanged.txt':'retain this blob\n'}
head=tree_for(None,initial)
for path,content in initial.items(): (LIVE/path).write_text(content)
git('update-ref','refs/probe/completed',head)
write_lock, revision_lock = threading.Lock(), threading.Lock()
records=[]
def capture():
    start=time.perf_counter_ns()
    with revision_lock:
        checkpoint=head
    pointer_ms=(time.perf_counter_ns()-start)/1e6
    # Retaining a ref protects snapshot reachability; this is outside pointer lock.
    git('update-ref',f'refs/probe/checkpoints/c{len(records)}',checkpoint)
    record={'snapshot':checkpoint,'pointer_read_ms':pointer_ms,'total_pin_ms':(time.perf_counter_ns()-start)/1e6}
    records.append(record)
    return checkpoint
half_installed,finish_install=threading.Event(),threading.Event()
updates={'price.txt':'quote(amount,currency)\n','caller.txt':'quote(100,USD)\n'}
new=tree_for(head,updates)
old=head

def apply():
    global head
    with write_lock:
        (LIVE/'price.txt').write_text(updates['price.txt'])
        half_installed.set()
        assert finish_install.wait(10)
        (LIVE/'caller.txt').write_text(updates['caller.txt'])
        # All writes installed before advancing the coherent completed revision.
        with revision_lock:
            git('update-ref','refs/probe/completed',new,head)
            head=new
worker=threading.Thread(target=apply)
worker.start()
assert half_installed.wait(10)
assert (LIVE/'price.txt').read_text()==updates['price.txt']
assert (LIVE/'caller.txt').read_text()==initial['caller.txt']
mid=capture()
assert mid==old
assert git('show',mid+':price.txt')=='quote(amount)'
assert git('show',mid+':caller.txt')=='quote(100)'
finish_install.set();worker.join(timeout=10);assert not worker.is_alive()
after=capture();assert after==new
assert git('show',after+':price.txt')=='quote(amount,currency)'
assert git('show',after+':caller.txt')=='quote(100,USD)'
assert git('rev-parse',old+':unchanged.txt')==git('rev-parse',new+':unchanged.txt')
subprocess.run(['git','--git-dir',str(STORE),'worktree','add','--detach',str(SLOT),after],check=True,capture_output=True)
assert (SLOT/'price.txt').read_text()==updates['price.txt']
assert (SLOT/'caller.txt').read_text()==updates['caller.txt']
(LIVE/'caller.txt').write_text('later unfinished draft\n')
assert (SLOT/'caller.txt').read_text()==updates['caller.txt']
# Incomplete retained edits need not compile; completeness is a separate group offer.
result={'scratch_root':str(ROOT),'captured_mid_install':mid,'captured_after_install':after,'coherent_mid_capture':True,'new_revision_excludes_partial_install':True,'unchanged_blob_reused':True,'test_worktree_isolated_from_later_draft':True,'checkpoint_records':records,'scope':'Known fixed text edits, serialized host mutation, Python metadata lock and retained Git refs. No database revision transaction, crash injection, runtime attribution or general filesystem atomicity. Capture returns last completed engine revision while live reads may observe partial installation.'}
(OUT/'capture-results.json').write_text(json.dumps(result,indent=2))
print(json.dumps(result,indent=2))
