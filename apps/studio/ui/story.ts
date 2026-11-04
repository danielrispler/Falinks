// The story rail, on the window's left: the selected session's state and step, its brief, cards and
// checks, with its open question docked at the bottom. A subagent's view shows its own line,
// questions, brief and cards; the checks are its session's.

import { code, element, text } from "./dom.js";
import { agentState } from "./list.js";
import { startClaude } from "./panes.js";
import { questionElement } from "./questions.js";
import { stateLabels, type CheckLine, type Question, type Selection } from "./types.js";

export function showView(selected: Selection | null) {
  element("empty").hidden = selected !== null;
  element("view").hidden = selected === null;
  element("identity").textContent = "";
  if (selected === null) {
    showQuestions([]);
    return;
  }
  const session = selected.session;
  const agent = session.agents.find((a) => a.id === selected.agent) ?? null;
  const [label, state] = agent === null ? [stateLabels[session.state], session.state] : agentState(session, agent);
  const step = (agent ?? session).last_step;
  element("view").toggleAttribute("data-agent", agent !== null);
  element("identity").textContent =
    agent === null ? `${session.folder_name} · ${session.id.slice(0, 8)}` : `${session.folder_name} · ${agent.name}`;
  element("state").textContent = label;
  element("state").dataset.state = state;
  // "Open" resumes an ended session in a new terminal, in its folder, where Claude Code keeps it.
  element("reopen").hidden = agent !== null || session.state !== "ended";
  element("reopen").onclick = () => startClaude(session.cwd, session.id).catch((e) => console.warn(e));
  element("tool").textContent = step?.tool ?? (agent === null ? session.returned : null) ?? "No step yet";
  element("detail").textContent = step?.detail ?? "";
  showQuestions(session.questions.filter((question) => question.agent === (agent?.id ?? null)));
  showChecks(session.checks);
}

// The questions the viewed agent asked: an open one docked under the story, the rest in it, newest
// first.
function showQuestions(questions: Question[]) {
  const newest = [...questions].reverse();
  const open = newest.filter((question) => question.answer === "open");
  const past = newest.filter((question) => question.answer !== "open");
  element("dock").replaceChildren(...open.map(questionElement));
  element("dock").hidden = open.length === 0;
  element("questions").replaceChildren(...past.map(questionElement));
  element("questions-title").hidden = past.length === 0;
}

// Whether a check passed, from its exit code; with no colour, as colour is only for "needs you".
function outcome(passed: boolean | null): string {
  if (passed === null) return "Ran";
  return passed ? "Passed" : "Failed";
}

// The checks that ran, newest first, one line each.
function showChecks(checks: CheckLine[]) {
  element("checks").replaceChildren(
    ...[...checks].reverse().map((line) => {
      const item = document.createElement("li");
      item.append(text("span", outcome(line.passed), "outcome"), code(line.command));
      return item;
    }),
  );
  element("checks-title").hidden = checks.length === 0;
}
