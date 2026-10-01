"""Offline assertions over saved evidence; no model calls or installations."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).parent / "evidence"


def hashes(root):
    return {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(root.rglob("*")) if p.is_file()}


for name in ("shared", "isolated"):
    root = ROOT / "measured" / name
    data = json.loads((root / "result.json").read_text())
    early = root / "early-candidate"
    final = root / "final-candidate"
    assert hashes(early) == data["early_hashes"]
    assert hashes(final) == data["final_hashes"]
    assert (root / "draft-before.ts").read_bytes() == (root / "draft-after.ts").read_bytes()
    assert "UNFINISHED_B" in (root / "draft-before.ts").read_text()
    assert "UNFINISHED_B" not in (early / "src/catalog.ts").read_text()
    assert "return `SKU:${value}`" in (early / "src/catalog.ts").read_text()
    assert 'throw new TypeError' in (early / "src/catalog.ts").read_text()
    assert "return { key }" in (final / "src/catalog.ts").read_text()
    assert "value.key" in (final / "src/catalog.ts").read_text()
    assert (early / "src/caller.ts").read_bytes() == (final / "src/caller.ts").read_bytes()
    assert data["baseline"]["passed"] and data["early_checks"]["passed"] and data["final_checks"]["passed"]
    assert not data["missing_required_B"]["passed"]
    assert "caller.ts" in data["missing_required_B"]["build"]["stdout"]
    assert data["missing_required_B"]["tests"]["skipped"] == "build failed"
    missing_a = json.loads((root / "missing-A-check.json").read_text())
    assert not missing_a["passed"] and "caller.ts" in missing_a["build"]["stdout"]
    assert data["engine_early_capture_wait_seconds"] >= 0
    assert min(data["agent_ended"].values()) > max(data["agent_started"].values())
    assert data["draft_checkpoint"]["parent_observed"] <= data["early_request"]["parent_observed"]
    if name == "shared":
        assert data["withdrawn"] and data["restored"]
        assert not data["dirty_shared_development_check"]["passed"]
        assert data["interruption_seconds"] > 0
    else:
        assert data["withdrawn"] is data["restored"] is None
        assert len(data["merges"]) == 2 and all(m["code"] == 0 for m in data["merges"])
    for role in ("A", "B"):
        events = [json.loads(line) for line in (root / (role + ".jsonl")).read_text().splitlines()]
        assert any(e.get("type") == "turn.completed" for e in events)
        patches = [e for e in events if e.get("type") == "item.completed" and e.get("item", {}).get("type") == "file_change"]
        assert len(patches) == 2
        commands = [e["item"] for e in events if e.get("type") == "item.completed" and e.get("item", {}).get("type") == "command_execution"]
        assert any("focused " + ("A-final" if role == "A" else "B-final") + " passed" in c.get("aggregated_output", "") and c.get("exit_code") == 0 for c in commands)
        if role == "B":
            assert any("control.py B generate" in c.get("command", "") and c.get("exit_code") == 0 for c in commands)
    for path in ("tests.cjs", "tsconfig.json", "eslint.config.cjs", "control.py", "generator.py", "focused.cjs"):
        assert (early / path).read_bytes() == (final / path).read_bytes()
probe = json.loads((ROOT / "child-probe/result.json").read_text())
assert probe["parent_exit"] == 0 and probe["child_alive_at_diagnostic_copy"]
assert probe["diagnostic_partial_group"] and probe["diagnostic_stayed_fixed"]
assert probe["registered_child_done_before_complete_copy"]
main = json.loads((ROOT / "main-preservation.json").read_text())
assert main["same_head"] and main["same_branch"] and main["same_status"] and main["same_tracked_bytes"]
print("Saved evidence passed: exact candidates, excluded/restored drafts, group failure, full checks, overlap, child boundary, main preservation.")
