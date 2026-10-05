"""Path-specific native patch-helper and sandbox mutation checks, without inference."""
import hashlib,json,subprocess,tempfile
from pathlib import Path
OUT=Path(__file__).resolve().parent
ROOT=Path(tempfile.mkdtemp(prefix='native-',dir=OUT));SOURCE=ROOT/'source';SOURCE.mkdir()
TARGET=SOURCE/'source.txt';TARGET.write_text('original\n')
PATCH='*** Begin Patch\n*** Update File: '+str(TARGET)+'\n@@\n-original\n+NATIVE_BYPASS\n*** End Patch'
BINARY='/opt/homebrew/Caskroom/codex/0.160.0/bin/codex'
PROFILE='permissions.falinks-native={extends=":workspace",filesystem={"'+str(SOURCE)+'"="read"},network={enabled=false}}'
command=[BINARY,'sandbox','-P','falinks-native','-C',str(ROOT),'-c',PROFILE,'--',BINARY,'--codex-run-as-apply-patch',PATCH]
p=subprocess.run(command,capture_output=True,text=True,timeout=30)
assert p.returncode != 0 and TARGET.read_text()=='original\n'
assert 'Failed to write file '+str(TARGET) in p.stderr,p.stderr
CONTROL=ROOT/'control.txt';CONTROL.write_text('original\n')
control_command=command[:-1]+[PATCH.replace(str(TARGET),str(CONTROL))]
control=subprocess.run(control_command,capture_output=True,text=True,timeout=30)
assert control.returncode==0 and CONTROL.read_text()=='NATIVE_BYPASS\n',control.stderr
result={'binary':BINARY,'binary_sha256':hashlib.sha256(Path(BINARY).read_bytes()).hexdigest(),'fixture_path':str(TARGET),'patch':PATCH,'exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr,'source_unchanged':True,'writable_control_patch_succeeded':True,'scope':'Native helper executes an exact path-bearing patch under the selected read-only subtree OS sandbox. Separately, the model router rejects native patch calls; its path-bearing event is not retained. This helper probe is not a newly observed model-native event.'}
(OUT/'native-results.json').write_text(json.dumps(result,indent=2))
print(json.dumps(result,indent=2))
