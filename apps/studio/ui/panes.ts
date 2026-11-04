// The terminal panes (#467): one per daemon terminal, in `#terminal-body`, one shown at a time. The
// login shell `shell` in the configured repository shows until a session's pane does. A session
// started from the studio runs `claude` in its own terminal, whose fresh id reaches its hooks as
// `STUDIO_TERMINAL_ID`, so the core links its row to the pane (`Session.terminal`).
// #469 closes a pane here (`kill`, `forget`, then drop it from `panes`); #470 revives a terminal the
// daemon lost, which `showSession` only reports today.

import type * as Xterm from "@xterm/xterm";
import { connect, list } from "./daemon.js";
import { element, invoke } from "./dom.js";
import { openPane } from "./terminal.js";
import type { Session } from "./types.js";

type Pane = { box: HTMLElement; term: Xterm.Terminal | null; title: string };
const panes = new Map<string, Pane>();
let repo = "";
let shown = "";
let ready: Promise<void> | null = null;

// Connects and shows the login shell in the configured repository.
export function startPanes(): Promise<void> {
  ready = connect().then((config) => {
    repo = config.repo;
    show("shell", () => openPane(box("shell"), "shell", repo));
  });
  return ready;
}

// A pane's box in `#terminal-body`, made the first time.
function box(id: string): HTMLElement {
  let pane = panes.get(id);
  if (!pane) {
    const node = document.createElement("div");
    node.className = "pane";
    element("terminal-body").append(node);
    pane = { box: node, term: null, title: "terminal" };
    panes.set(id, pane);
  }
  return pane.box;
}

// Shows terminal `id`'s pane, drawing it with `open` the first time, and the header's title and size.
function show(id: string, open: () => Xterm.Terminal | null) {
  shown = id;
  const place = box(id);
  for (const pane of panes.values()) pane.box.hidden = pane.box !== place;
  const pane = panes.get(id)!;
  if (pane.term === null && place.childElementCount === 0) {
    pane.term = open();
    pane.term?.onResize(() => shown === id && header(pane));
    pane.term?.onTitleChange((title) => {
      pane.title = title || "terminal";
      if (shown === id) header(pane);
    });
  }
  header(pane);
  pane.term?.focus();
}

function header({ term, title }: Pane) {
  element("terminal-title").textContent = title;
  element("size").textContent = term ? `${term.cols} × ${term.rows}` : "";
}

// Starts `claude` in `cwd` in a new terminal, or `claude --resume <resume>` for an ended session,
// and shows its pane.
export async function startClaude(cwd: string, resume: string | null = null) {
  const argv = await invoke<string[]>("claude_argv", { resume });
  const id = `claude-${crypto.randomUUID()}`;
  show(id, () => openPane(box(id), id, cwd, argv));
}

// The selected session's pane: its terminal's, or the shell for a session the studio did not start.
// Daniel's click always shows it; the shell's updates of the selection switch the pane only when the
// session or its terminal changed, so they never take away a pane Daniel just started.
let selected: string | null = null;
export async function showSession(session: Session | null, clicked = false) {
  const key = session ? `${session.id} ${session.terminal ?? ""}` : null;
  if (key === selected && !clicked) return;
  selected = key;
  const connected = await ready?.then(() => true, () => false);
  const current = () => selected === key;
  const id = session?.terminal;
  if (!connected || !current()) return;
  if (!id) return show("shell", () => openPane(box("shell"), "shell", repo));
  if (panes.has(id)) return show(id, () => null);
  // A terminal of an earlier app run: attach when the daemon still runs it. Opening an exited or
  // unknown id would start a new program under it.
  const terminal = (await list()).find((t) => t.id === id);
  if (!current()) return;
  const gone = terminal ? `its claude exited (${terminal.exited}); Open starts it again` : "the daemon has no such terminal";
  show(id, () => {
    if (terminal && terminal.exited === null) return openPane(box(id), id, terminal.cwd, terminal.argv);
    box(id).append(Object.assign(document.createElement("p"), { className: "empty", textContent: `No pane: ${gone}.` }));
    return null;
  });
}
