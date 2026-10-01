"""A completed shell parent is not proof that its descendants stopped writing."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time


def wait(path):
    while not path.exists():
        time.sleep(.02)


if len(sys.argv) == 3 and sys.argv[1] == "writer":
    root = Path(sys.argv[2])
    (root / "one.ts").write_text('export const version = 1;\n')
    (root / "started.json").write_text(json.dumps({"pid": os.getpid()}))
    wait(root / "release")
    (root / "one.ts").write_text('export const version = 2;\n')
    (root / "two.ts").write_text('export const version = 2;\n')
    (root / "done").write_text("done\n")
elif len(sys.argv) == 3 and sys.argv[1] == "parent":
    subprocess.Popen([sys.executable, __file__, "writer", sys.argv[2]],
                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                     stderr=subprocess.DEVNULL, start_new_session=True)
else:
    root = Path(sys.argv[1]); root.mkdir()
    live = root / "live"; live.mkdir()
    started = time.monotonic()
    parent = subprocess.run([sys.executable, __file__, "parent", str(live)], check=True)
    wait(live / "started.json")
    child = json.loads((live / "started.json").read_text())["pid"]
    os.kill(child, 0)  # Registered child's continued existence is observed.
    diagnostic = root / "diagnostic"; shutil.copytree(live, diagnostic)
    assert (diagnostic / "one.ts").read_text() == 'export const version = 1;\n'
    assert not (diagnostic / "two.ts").exists()
    (live / "release").write_text("finish\n")
    wait(live / "done")
    complete = root / "complete"; shutil.copytree(live, complete)
    assert (complete / "one.ts").read_text() == (complete / "two.ts").read_text()
    assert (diagnostic / "one.ts").read_text() == 'export const version = 1;\n'
    result = {"parent_exit": parent.returncode, "child_alive_at_diagnostic_copy": True,
              "diagnostic_partial_group": True, "diagnostic_stayed_fixed": True,
              "registered_child_done_before_complete_copy": True,
              "seconds": time.monotonic() - started}
    (root / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))
