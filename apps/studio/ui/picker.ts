// The picker (#467): where a new session starts. It lists the configured repository's main checkout
// and its tasks, read live from git each time it opens, with `agent-*` worktrees hidden and counted
// on the main checkout. Choosing one starts `claude` there in a new terminal.
// #468 adds a "new task" field and a remove button per task here.

import { element, invoke, pressable, text } from "./dom.js";
import { startClaude } from "./panes.js";
import type { Place, Places } from "./types.js";

// A place's row: its name, then its folder.
function placeRow(name: string, place: Place): HTMLElement {
  const button = pressable(document.createElement("button"), false, () => {
    element("picker").hidden = true;
    startClaude(place.path).catch((e) => console.warn(e));
  });
  button.className = "row";
  const first = text("span", "", "l1");
  first.append(text("span", name, "name"));
  const second = text("span", place.path, "l2");
  button.append(first, second);
  return button;
}

function picked({ main, tasks, hidden }: Places): HTMLElement[] {
  const folder = (place: Place) => place.path.split("/").pop() ?? place.path;
  const count = hidden ? [text("p", `${hidden} agent worktree${hidden === 1 ? "" : "s"} hidden`, "hidden-count")] : [];
  return [
    text("h1", "Start a session in"),
    placeRow(`Main checkout${main.branch ? ` · ${main.branch}` : ""}`, main),
    ...count,
    ...tasks.map((task) => placeRow(task.branch ?? `${folder(task)} (detached)`, task)),
  ];
}

// Opens the picker, or closes it when open.
export async function showPicker() {
  const picker = element("picker");
  picker.hidden = !picker.hidden;
  if (picker.hidden) return;
  picker.replaceChildren(text("p", "Reading worktrees…", "empty"));
  try {
    picker.replaceChildren(...picked(await invoke<Places>("places")));
  } catch (e) {
    picker.replaceChildren(text("p", `No worktrees: ${e}`, "empty"));
  }
}
