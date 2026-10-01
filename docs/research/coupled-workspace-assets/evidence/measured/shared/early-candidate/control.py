import hashlib,json,os,pathlib,subprocess,sys,time
root=pathlib.Path.cwd()
coord=pathlib.Path(os.environ["TRIAL_COORD"])
role,stage=sys.argv[1:3]
def mark(name,data=None):
 (coord/name).write_text(json.dumps({"time":time.monotonic(),"pid":os.getpid(),"data":data}))
def wait(name):
 while not (coord/name).exists(): time.sleep(.05)
if stage=="generate":
 # Synchronous shell child: the checkpoint comes only after its exit.
 subprocess.run([sys.executable,"generator.py"],check=True)
 mark(role+"-generator-exited")
 sys.exit(0)
mark(role+"-"+stage)
if role=="B" and stage=="draft" and os.environ["TRIAL_CONDITION"]=="shared":
 wait("withdraw")
 p=root/"src/catalog.ts"
 text=p.read_text(); head,draft=text.split("export function displaySku",1)
 (coord/"B-draft-backup.txt").write_text("export function displaySku"+draft)
 p.write_text(head+os.environ["TRIAL_BASE_B"])
 mark("B-withdrawn",hashlib.sha256(draft.encode()).hexdigest())
 wait("restore")
 current=p.read_text(); head,_=current.split("export function displaySku",1)
 p.write_text(head+(coord/"B-draft-backup.txt").read_text())
 mark("B-restored")
wait("release-"+role+"-"+stage)
mark(role+"-"+stage+"-released")
