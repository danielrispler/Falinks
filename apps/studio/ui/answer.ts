// Answering from the card (#461, decision 16). While a session's view shows and the window is
// visible, the window tells the shell every second, so the answer hook waits for the card; the shell
// returns the session's open request, whose nonce the answer echoes.

import { invoke } from "./dom.js";
import { studio } from "./state.js";
import type { Question } from "./types.js";

// The answer hook's request (`hook/src/answer.rs`).
export type Ask = { nonce: string; tool_name: string; tool_use_id: string; tool_input: unknown };
// Each question's text to its label, or a multi-select's to its labels; null answers in the terminal.
export type Answers = Record<string, string | string[]> | null;

let current: { session: string; ask: Ask } | null = null;
let redraw = () => {};
// The request last answered, which the shell may still return until the hook takes the answer.
let sent = "";

// The open request for `question`, if the card can answer it.
export const askFor = (question: Question) =>
  current?.ask.tool_use_id === question.tool_use_id ? current.ask : null;

export function startWatching(draw: () => void) {
  redraw = draw;
  setInterval(async () => {
    const session = document.visibilityState === "visible" ? (studio.selection?.session.id ?? null) : null;
    const ask = await invoke<Ask | null>("watch", { session }).catch((e) => (console.warn(e), null));
    const next = session !== null && ask !== null && ask.nonce !== sent ? { session, ask } : null;
    if (next?.ask.nonce === current?.ask.nonce) return;
    current = next;
    redraw();
  }, 1000);
}

export async function sendAnswer(ask: Ask, answers: Answers) {
  if (current?.ask.nonce !== ask.nonce) return;
  const { session } = current;
  sent = ask.nonce;
  current = null;
  redraw();
  await invoke("answer", { session, nonce: ask.nonce, answers }).catch((e) => console.warn(e));
}
