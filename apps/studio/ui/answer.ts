// Answering from the card (#461, decision 16). While a session's view shows and the window is
// visible, the window tells the shell every second, so the answer hook waits for the card; the shell
// returns the session's open request, whose nonce the answer echoes.

import { invoke } from "./dom.js";
import { studio } from "./state.js";
import type { Question } from "./types.js";

// The answer hook's request (`hook/src/answer.rs`): a question or a plan at its `PreToolUse`, or a
// permission request, which names no call (#478).
export type Ask = {
  nonce: string;
  hook_event_name: string;
  tool_name: string;
  tool_use_id: string | null;
  tool_input: Record<string, unknown>;
};
// A question's answer: each question's text to its label, or a multi-select's to its labels. A plan's
// or a permission's: allow or deny. Null answers in the terminal.
export type Reply = { answers: Record<string, string | string[]> } | { decision: "allow" | "deny" } | null;

let current: { session: string; ask: Ask } | null = null;
let redraw = () => {};
// The request last answered, which the shell may still return until the hook takes the answer.
let sent = "";

// The open request for `question`, if the card can answer it.
export const askFor = (question: Question) =>
  current?.ask.tool_use_id === question.tool_use_id ? current.ask : null;

// The open plan or permission request, if any (#478): the core keeps no question for either.
export const openDecision = () =>
  current !== null && current.ask.tool_name !== "AskUserQuestion" ? current.ask : null;

export function startWatching(draw: () => void) {
  redraw = draw;
  setInterval(async () => {
    const selected = document.visibilityState === "visible" ? studio.selection?.session : undefined;
    // The hook's payload names the latest conversation, which a `/clear` changes, not the row (#474).
    const session = selected ? selected.resume_id || selected.id : null;
    const ask = await invoke<Ask | null>("watch", { session }).catch((e) => (console.warn(e), null));
    const next = session !== null && ask !== null && ask.nonce !== sent ? { session, ask } : null;
    if (next?.ask.nonce === current?.ask.nonce) return;
    current = next;
    redraw();
  }, 1000);
}

export async function sendAnswer(ask: Ask, reply: Reply) {
  if (current?.ask.nonce !== ask.nonce) return;
  const { session } = current;
  sent = ask.nonce;
  current = null;
  redraw();
  const args = { session, nonce: ask.nonce, answers: null, decision: null, ...reply };
  await invoke("answer", args).catch((e) => console.warn(e));
}
