"""Scratch primitive checks only: no agent harness or Falinks implementation."""

from pathlib import Path
import shutil
import subprocess
import tempfile


def demo():
    with tempfile.TemporaryDirectory(prefix="falinks-capture-check-") as directory:
        root = Path(directory)
        base_text = (
            "export function price() {\n  return 10;\n}\n\n"
            "// Separate owners; textual merge is not an ownership check.\n\n"
            "export function receipt() {\n  return 'plain';\n}\n"
        )
        a_text = base_text.replace("return 10", "return 20")
        b_text = base_text.replace("'plain'", "'detailed'")
        base, a, b, shared = [root / name for name in ("base.ts", "a.ts", "b.ts", "shared.ts")]
        for path, contents in ((base, base_text), (a, a_text), (b, b_text)):
            path.write_text(contents)

        shutil.copyfile(a, shared)
        shutil.copyfile(b, shared)
        assert shared.read_text() == b_text and "return 20" not in shared.read_text()
        print("PASS: naive copy-back loses A's independent edit")

        # Freshness checks must run under publication serialization in a real engine.
        assert shared.read_text() != base.read_text()
        print("PASS: stale whole-file base is detectable (it also rejects harmless sibling edits)")

        merged = subprocess.run(["git", "merge-file", "-p", str(a), str(base), str(b)], capture_output=True, text=True)
        assert merged.returncode == 0, merged.stderr
        assert "return 20" in merged.stdout and "'detailed'" in merged.stdout
        assert a.read_text() == a_text and b.read_text() == b_text
        print("PASS: three-way merge preserves separated edits without changing the drafts")

        b.write_text(base_text.replace("return 10", "return 30"))
        conflict = subprocess.run(["git", "merge-file", "-p", str(a), str(base), str(b)], capture_output=True, text=True)
        assert 0 < conflict.returncode <= 127 and "<<<<<<<" in conflict.stdout
        print("PASS: competing edits to the same line return a merge conflict")

        owner_text = (
            "export function price() {\n  const tax = 1;\n\n"
            "  // Two disjoint edits within the same owning symbol.\n\n"
            "  const discount = 0;\n  return 10 + tax - discount;\n}\n"
        )
        base.write_text(owner_text)
        a.write_text(owner_text.replace("tax = 1", "tax = 2"))
        b.write_text(owner_text.replace("discount = 0", "discount = 3"))
        owner_merge = subprocess.run(["git", "merge-file", "-p", str(a), str(base), str(b)], capture_output=True, text=True)
        assert owner_merge.returncode == 0 and "tax = 2" in owner_merge.stdout and "discount = 3" in owner_merge.stdout
        print("PASS: same-owner disjoint edits can merge cleanly (text merge alone cannot enforce our owner rule)")

        draft = root / "draft"
        draft.mkdir()
        (draft / "code.ts").write_text(merged.stdout)
        (draft / "dependency.ts").write_text("export const tax = 1;\n")
        capture = root / "capture"
        # Quiescent source during capture; copytree is not a concurrent snapshot primitive.
        shutil.copytree(draft, capture)
        (draft / "code.ts").write_text("unfinished edit\n")
        (draft / "dependency.ts").write_text("export const tax = 2;\n")
        assert (capture / "code.ts").read_text() == merged.stdout
        assert (capture / "dependency.ts").read_text() == "export const tax = 1;\n"
        print("PASS: completed directory capture stays fixed while drafts change")
        assert (draft / "dependency.ts").read_text() != (capture / "dependency.ts").read_text()
        print("PASS: copying one edited file does not freeze other dependency files")

        mixed = root / "mixed"
        mixed.mkdir()
        (draft / "code.ts").write_text(merged.stdout)
        (draft / "dependency.ts").write_text("export const tax = 1;\n")
        shutil.copyfile(draft / "code.ts", mixed / "code.ts")
        (draft / "code.ts").write_text("new draft revision\n")
        (draft / "dependency.ts").write_text("export const tax = 2;\n")
        shutil.copyfile(draft / "dependency.ts", mixed / "dependency.ts")
        assert (mixed / "code.ts").read_text() == merged.stdout
        assert (mixed / "dependency.ts").read_text() == "export const tax = 2;\n"
        print("PASS: copying files across an intervening draft change can produce a mixed capture")


if __name__ == "__main__":
    demo()
