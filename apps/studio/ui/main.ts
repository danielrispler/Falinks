// The side pane: the "now" line of the session Daniel prompted last, and its cards. The core decides
// what it shows; this file only draws what the shell sends, on load and on each "now" or "cards" event.

type Step = { tool: string; detail: string | null };
type Session = { id: string; cwd: string; last_step: Step | null };
type CardRequest = { session_id: string; tool_use_id: string; step: Step; change: string };
type Card = {
  question: string;
  answer: string;
  paths: string[];
  code: string;
  why_it_matters: string;
  not_checked: string;
};

declare global {
  interface Window {
    __TAURI__: {
      core: { invoke<T>(command: string): Promise<T> };
      event: { listen<T>(event: string, handler: (event: { payload: T }) => void): Promise<unknown> };
    };
  }
}

const element = (id: string) => document.getElementById(id)!;

function show(session: Session | null) {
  element("empty").hidden = session !== null;
  element("session").hidden = session === null;
  element("step").hidden = session === null;
  if (session === null) return;
  const folder = session.cwd.split("/").pop() || session.cwd;
  element("session").textContent = `${folder} · ${session.id.slice(0, 8)}`;
  element("tool").textContent = session.last_step?.tool ?? "No step yet";
  element("detail").textContent = session.last_step?.detail ?? "";
}

// Code, left to right, in its own isolate.
function code(text: string): HTMLElement {
  const node = document.createElement("code");
  node.dir = "ltr";
  node.textContent = text;
  return node;
}

// Hebrew text whose code is in backticks. Each code span becomes a left-to-right isolate, so `a <= b`
// is not mirrored inside the right-to-left text.
function hebrew(tag: string, text: string): HTMLElement {
  const node = document.createElement(tag);
  node.append(...text.split("`").map((part, i) => (i % 2 ? code(part) : part)));
  return node;
}

// A card in Hebrew, right to left: the question, then, on a tap, the answer, the file paths, the code
// left to right, why the step matters and what was not checked.
function cardElement(request: CardRequest, card: Card): HTMLElement {
  const details = document.createElement("details");
  details.className = "card";
  details.dir = "rtl";
  details.lang = "he";
  details.dataset.id = request.tool_use_id;
  const answer = hebrew("p", card.answer);
  answer.className = "answer";
  const paths = document.createElement("p");
  paths.className = "paths";
  paths.append(...card.paths.map(code));
  const source = document.createElement("pre");
  source.dir = "ltr";
  source.textContent = card.code;
  details.append(
    hebrew("summary", card.question),
    answer,
    paths,
    source,
    hebrew("h2", "למה זה חשוב"),
    hebrew("p", card.why_it_matters),
    hebrew("h2", "מה לא נבדק"),
    hebrew("p", card.not_checked),
  );
  return details;
}

// Each card is a <details>: its question shows, and a tap reveals the rest. Cards already drawn are
// kept, keyed by their step's tool_use_id, so one Daniel opened stays open on the next event.
function showCards(cards: [CardRequest, Card][]) {
  const list = element("cards");
  const ids = new Set(cards.map(([request]) => request.tool_use_id));
  const drawn = new Set<string>();
  for (const node of [...list.children] as HTMLElement[]) {
    if (ids.has(node.dataset.id!)) drawn.add(node.dataset.id!);
    else node.remove();
  }
  for (const [request, card] of cards) {
    if (drawn.has(request.tool_use_id)) continue;
    list.prepend(cardElement(request, card));
  }
  element("cards-title").hidden = cards.length === 0;
}

window.__TAURI__.event.listen<Session | null>("now", (event) => show(event.payload));
window.__TAURI__.core.invoke<Session | null>("now").then(show);
window.__TAURI__.event.listen<[CardRequest, Card][]>("cards", (event) => showCards(event.payload));
window.__TAURI__.core.invoke<[CardRequest, Card][]>("cards").then(showCards);

export {};
