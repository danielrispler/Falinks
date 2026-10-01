"""Bounded cooperative Codex comparison; fresh output directory required."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

BASE = '''export function normalizeSku(value: string): string {
  return value.trim();
}

// These functions intentionally share a file, but own separate edits.
// A owns normalization; B owns presentation.
// No symbol reservation or automatic ownership detection is implemented.

export function displaySku(value: string): string {
  return `SKU:${value}`;
}
'''
CALLER = '''import { normalizeSku, displaySku } from "./catalog";

export function checkout(value: string): string {
  return displaySku(normalizeSku(value));
}
'''
TESTS = '''const assert = require("node:assert/strict");
const { normalizeSku, displaySku } = require("./dist/catalog");
const { checkout } = require("./dist/caller");
const phase = process.argv[2];
assert.equal(checkout("  abc  "), "SKU:abc");
assert.equal(checkout("é"), "SKU:é");
if (phase !== "base") {
  assert.throws(() => normalizeSku("   "), TypeError);
}
if (phase === "final") {
  assert.deepEqual(normalizeSku(" abc "), { key: "abc" });
  assert.equal(displaySku({ key: "abc" }), "SKU:abc");
  assert.equal(require("./dist/generated/tag").tag, "v2");
  assert.equal(require("./dist/generated/schema").schema, "key");
} else {
  assert.equal(normalizeSku(" abc "), "abc");
}
console.log(`fixed ${phase} assertions passed`);
'''
CONTROL = '''import hashlib,json,os,pathlib,subprocess,sys,time
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
'''


def run(argv, cwd, check=True):
    result = subprocess.run(argv, cwd=cwd, text=True, capture_output=True)
    if check and result.returncode:
        raise RuntimeError(f"{argv}: {result.stderr}\n{result.stdout}")
    return {"argv": argv, "code": result.returncode, "stdout": result.stdout,
            "stderr": result.stderr}


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value)


def hashes(root):
    return {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(root.rglob("*")) if p.is_file()
            and p.parts[len(root.parts)] not in (".git", "dist")}


def copy_candidate(source, target):
    shutil.copytree(source, target, ignore=shutil.ignore_patterns(".git", "dist"))


def validate(root, phase, tools):
    started = time.monotonic()
    shutil.rmtree(root / "dist", ignore_errors=True)
    build = run(["node", str(tools / "node_modules/typescript/bin/tsc"),
                 "-p", "tsconfig.json"], root, False)
    env = dict(os.environ, TRIAL_TOOLS=str(tools))
    lint = subprocess.run(["node", str(tools / "node_modules/eslint/bin/eslint.js"),
                           "src", "--config", "eslint.config.cjs"], cwd=root,
                          env=env, capture_output=True, text=True)
    lint = {"code": lint.returncode, "stdout": lint.stdout, "stderr": lint.stderr}
    tests = run(["node", "tests.cjs", phase], root, False) if build["code"] == 0 else {"skipped": "build failed"}
    return {"build": build, "lint": lint, "tests": tests,
            "seconds": time.monotonic() - started,
            "passed": build["code"] == lint["code"] == tests.get("code", -1) == 0}


def wait_mark(coord, name, processes):
    # ponytail: cooperative file barriers only; use a mediated harness for arbitrary writers.
    while not (coord / name).exists():
        for role, process in processes.items():
            if process.poll() is not None and not (coord / (role + "-final")).exists():
                raise RuntimeError(f"Agent {role} exited before checkpoint: {process.returncode}")
        time.sleep(.05)
    return dict(json.loads((coord / name).read_text()), parent_observed=time.monotonic())


def wait_marks(coord, names, processes):
    observations = {}
    while len(observations) != len(names):
        for name in names:
            if name not in observations and (coord / name).exists():
                observations[name] = wait_mark(coord, name, processes)
        for role, process in processes.items():
            if process.poll() is not None and not (coord / (role + "-final")).exists():
                raise RuntimeError(f"Agent {role} exited before checkpoint: {process.returncode}")
        time.sleep(.02)
    return observations


def signal(coord, name):
    write(coord / name, "release\n")


def prompt(role):
    common = '''This is a bounded scratch experiment, not Falinks production code. Work only here; do not modify checks/control/configuration, use Git commits, spawn agents, or access the network. A owns normalizeSku; B owns displaySku and generated modules. caller.ts is deliberately UNTOUCHED and its complete compilation matters. Use native apply_patch for catalog.ts changes; never replace the whole file or overwrite the other function. The fixed behavioral contract: trim whitespace, reject a blank SKU with TypeError, preserve casing/Unicode, presentation remains SKU:<key>. The final coupled contract changes normalizeSku's result to {key: string} and displaySku's argument to that object; both revisions are required together, with caller.ts unchanged. Readiness/participation is advisory, focused tests are development feedback only. The controller validates copies of exact candidates. There is no completion deadline; checkpoints force the unfinished-draft scenario for observation, not speed scoring. After each checkpoint command returns, reread catalog.ts before your next patch. Do not leave asynchronous/detached writers.\n'''
    if role == "A":
        return common + '''First change ONLY normalizeSku to still return a string, trim it and throw TypeError for empty. Run `node focused.cjs A`. Then request full validation by running `python3 control.py A early` (this deliberately parks while B's draft exists; wait for the tool to complete). Next change ONLY normalizeSku to return {key: string}, with the same empty rejection. Run `node focused.cjs A-final` as feedback, then offer the required group revision using `python3 control.py A final`. After release, finish with a concise statement of changes and checks. Do not change displaySku.\n'''
    return common + '''First change ONLY displaySku to `return undefined; // UNFINISHED_B`, deliberately unfinished and unoffered. Then run `python3 control.py B draft`. This means you decline the OPTIONAL early joint check, with no estimate; the controller may proceed without you. In the shared condition this helper cooperatively backs up, withdraws and restores YOUR unfinished function while both writers are parked; it preserves A's function and returns only after restoration. In the isolated condition no withdrawal occurs. After the helper returns, reread the file and complete ONLY displaySku: accept {key: string} and return `SKU:${value.key}`. Run `python3 control.py B generate` to synchronously generate two valid TypeScript modules in a shell child, then `node focused.cjs B-final`, then offer the required group revision with `python3 control.py B final`. After release finish concisely. Do not change normalizeSku.\n'''


def fixture(root):
    root.mkdir()
    write(root / "src/catalog.ts", BASE)
    write(root / "src/caller.ts", CALLER)
    write(root / "tests.cjs", TESTS)
    write(root / "control.py", CONTROL)
    write(root / "generator.py", '''import pathlib
p = pathlib.Path("src/generated")
p.mkdir(exist_ok=True)
(p / "tag.ts").write_text('export const tag = "v2";\\n')
(p / "schema.ts").write_text('export const schema = "key";\\n')
''')
    write(root / ".gitignore", "dist/\n")
    write(root / "tsconfig.json", json.dumps({"compilerOptions": {
        "strict": True, "target": "ES2022", "module": "CommonJS", "outDir": "dist",
        "noEmitOnError": True}, "include": ["src/**/*.ts"]}, indent=2) + "\n")
    write(root / "eslint.config.cjs", '''const parser = require(process.env.TRIAL_TOOLS + "/node_modules/@typescript-eslint/parser/dist/index.js");
module.exports = [{files:["**/*.ts"], languageOptions:{parser}, rules:{"no-unreachable":"error", "no-debugger":"error", "eqeqeq":"error", "no-var":"error", "prefer-const":"error"}}];
''')
    write(root / "focused.cjs", '''const fs = require("node:fs");
const assert = require("node:assert/strict");
const ts = require(process.env.TRIAL_TOOLS + "/node_modules/typescript");
const code = ts.transpileModule(fs.readFileSync("src/catalog.ts","utf8"), {compilerOptions:{module:ts.ModuleKind.CommonJS}}).outputText;
const m = {exports:{}}; new Function("exports",code)(m.exports);
const mode = process.argv[2];
if(mode === "B-final") assert.equal(m.exports.displaySku({key:"abc"}), "SKU:abc");
else {assert.deepEqual(m.exports.normalizeSku(" abc "), mode === "A-final" ? {key:"abc"} : "abc"); assert.throws(()=>m.exports.normalizeSku(" "),TypeError);}
console.log(`focused ${mode} passed; not publication authorization`);
''')
    run(["git", "init", "-b", "base"], root)
    run(["git", "config", "user.name", "Falinks scratch experiment"], root)
    run(["git", "config", "user.email", "scratch@invalid.local"], root)
    run(["git", "add", "."], root)
    run(["git", "commit", "-m", "Fixed coupled-agent fixture and behavioral contracts"], root)


def condition(dest, name, tools):
    started = time.monotonic()
    dest.mkdir()
    coord = dest / "coord"; coord.mkdir()
    root = dest / "repo"; fixture(root)
    baseline = validate(root, "base", tools)
    assert baseline["passed"], baseline
    base = run(["git", "rev-parse", "HEAD"], root)["stdout"].strip()
    workspaces = {"A": root, "B": root}
    if name == "isolated":
        for role in ("A", "B"):
            workspaces[role] = dest / role
            run(["git", "worktree", "add", "-b", role, str(workspaces[role]), base], root)
    setup_seconds = time.monotonic() - started
    processes = {}; streams = []; argv_saved = {}; agent_start = {}
    for role in ("A", "B"):
        write(dest / (role + "-prompt.txt"), prompt(role))
        out = open(dest / (role + ".jsonl"), "w")
        err = open(dest / (role + ".stderr"), "w")
        streams += [out, err]
        argv = ["codex", "exec", "--ignore-user-config", "--ephemeral", "-C", str(workspaces[role]),
                "-s", "workspace-write", "--add-dir", str(coord), "--json", "-"]
        env = dict(os.environ, TRIAL_COORD=str(coord), TRIAL_CONDITION=name,
                   TRIAL_TOOLS=str(tools), TRIAL_BASE_B="export function displaySku" + BASE.split("export function displaySku", 1)[1])
        agent_start[role] = time.monotonic()
        processes[role] = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=out, stderr=err,
                                           cwd=workspaces[role], env=env, text=True)
        processes[role].stdin.write(prompt(role)); processes[role].stdin.close()
        argv_saved[role] = argv
    try:
        initial = wait_marks(coord, ["A-early", "B-draft"], processes)
        b_draft = initial["B-draft"]
        a_early = initial["A-early"]
        request_at = a_early["parent_observed"]
        write(dest / "draft-before.ts", (workspaces["B"] / "src/catalog.ts").read_text())
        dirty = validate(root, "early", tools) if name == "shared" else None
        if name == "shared":
            assert "UNFINISHED_B" in (root / "src/catalog.ts").read_text()
            signal(coord, "withdraw")
            withdrawn = wait_mark(coord, "B-withdrawn", processes)
        else:
            withdrawn = None
        capture_at = time.monotonic()
        early = dest / "early-candidate"; copy_candidate(workspaces["A"], early)
        early_hash = hashes(early)
        assert "UNFINISHED_B" not in (early / "src/catalog.ts").read_text()
        if name == "shared":
            signal(coord, "restore")
            restored = wait_mark(coord, "B-restored", processes)
            assert (root / "src/catalog.ts").read_text() == (dest / "draft-before.ts").read_text()
        else:
            restored = None
            assert (workspaces["B"] / "src/catalog.ts").read_text() == (dest / "draft-before.ts").read_text()
        write(dest / "draft-after.ts", (workspaces["B"] / "src/catalog.ts").read_text())
        signal(coord, "release-A-early"); signal(coord, "release-B-draft")
        early_checks = validate(early, "early", tools)
        assert early_checks["passed"], early_checks
        final_marks = wait_marks(coord, ["A-final", "B-final"], processes)
        marks = {role: final_marks[role + "-final"] for role in ("A", "B")}
        assert hashes(early) == early_hash
        integration_started = time.monotonic()
        merges = []
        if name == "isolated":
            for role in ("A", "B"):
                run(["git", "add", "src"], workspaces[role])
                run(["git", "commit", "-m", role + " offered final revision"], workspaces[role])
            final = dest / "final-candidate"
            run(["git", "worktree", "add", "-b", "combined", str(final), base], root)
            merges.append(run(["git", "merge", "--no-edit", "A"], final))
            a_only = validate(final, "final", tools)
            assert not a_only["passed"] and "caller.ts" in a_only["build"]["stdout"]
            merges.append(run(["git", "merge", "--no-edit", "B"], final, False))
            if merges[-1]["code"]:
                raise RuntimeError("Normal Git merge conflicted; retain evidence and report recovery required")
        else:
            final = dest / "final-candidate"; copy_candidate(root, final)
            # Diagnostic omitted-member candidate, not attributed shared capture.
            diagnostic = dest / "missing-B"; copy_candidate(final, diagnostic)
            text = (diagnostic / "src/catalog.ts").read_text()
            write(diagnostic / "src/catalog.ts", text.split("export function displaySku", 1)[0]
                  + "export function displaySku" + BASE.split("export function displaySku", 1)[1])
            a_only = validate(diagnostic, "final", tools)
            assert not a_only["passed"] and "caller.ts" in a_only["build"]["stdout"]
        integration_seconds = time.monotonic() - integration_started - a_only["seconds"]
        final_hash = hashes(final)
        final_checks = validate(final, "final", tools)
        assert final_checks["passed"], final_checks
        assert (final / "src/caller.ts").read_text() == CALLER
        assert hashes(final) == final_hash
        for role in ("A", "B"): signal(coord, "release-" + role + "-final")
        exits = {}; agent_end = {}
        for role, process in processes.items():
            exits[role] = process.wait(); agent_end[role] = time.monotonic()
        assert all(code == 0 for code in exits.values()), exits
        result = {"condition": name, "base": base, "setup_seconds": setup_seconds,
            "argv": argv_saved, "agent_started": agent_start, "agent_ended": agent_end,
            "baseline": baseline, "early_request": a_early, "draft_checkpoint": b_draft,
            "engine_early_capture_wait_seconds": capture_at - request_at,
            "withdrawn": withdrawn, "restored": restored,
            "interruption_seconds": restored["parent_observed"] - withdrawn["parent_observed"] if restored else 0,
            "dirty_shared_development_check": dirty, "early_hashes": early_hash,
            "early_checks": early_checks, "final_checkpoints": marks, "missing_required_B": a_only,
            "merges": merges, "integration_seconds": integration_seconds,
            "final_hashes": final_hash, "final_checks": final_checks,
            "total_seconds": time.monotonic() - started, "exits": exits}
        write(dest / "result.json", json.dumps(result, indent=2) + "\n")
        print(json.dumps({"condition": name, "total_seconds": result["total_seconds"],
                          "capture_wait": result["engine_early_capture_wait_seconds"],
                          "integration_seconds": integration_seconds}), flush=True)
    finally:
        for process in processes.values():
            if process.poll() is None: process.terminate()
        for stream in streams: stream.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    parser.add_argument("tools", type=Path)
    args = parser.parse_args()
    args.output.mkdir()  # Never reuse or reset an existing experiment.
    for name in ("shared", "isolated"):
        condition(args.output / name, name, args.tools.resolve())
