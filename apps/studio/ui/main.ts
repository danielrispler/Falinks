// The side pane: the state and "now" line of the session Daniel prompted last, and its cards. The core
// decides what it shows; this file only draws what the shell sends, on load and on each "now" or
// "cards" event.

type Step = { tool: string; detail: string | null };
type SessionState = "working" | "needs_you" | "your_turn";
type Session = { id: string; cwd: string; last_step: Step | null; state: SessionState };
type CardRequest = { session_id: string; tool_use_id: string; step: Step; change: string };
type Card = { question: string; answer: string };
type StoredCard = { request: CardRequest; card: Card; revealed_at: number | null };

const stateLabels: Record<SessionState, string> = {
  working: "Working",
  needs_you: "Needs you",
  your_turn: "Your turn",
};

declare global {
  interface Window {
    __TAURI__: {
      core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> };
      event: { listen<T>(event: string, handler: (event: { payload: T }) => void): Promise<unknown> };
    };
  }
}

const element = (id: string) => document.getElementById(id)!;

function show(session: Session | null) {
  element("empty").hidden = session !== null;
  element("session").hidden = session === null;
  element("state").hidden = session === null;
  element("step").hidden = session === null;
  if (session === null) return;
  const folder = session.cwd.split("/").pop() || session.cwd;
  element("session").textContent = `${folder} · ${session.id.slice(0, 8)}`;
  element("state").textContent = stateLabels[session.state];
  element("state").dataset.state = session.state;
  element("tool").textContent = session.last_step?.tool ?? "No step yet";
  element("detail").textContent = session.last_step?.detail ?? "";
}

// Each card is a <details>: its question shows, and a tap reveals the answer. Cards already drawn are
// kept, keyed by their step's tool_use_id, so one Daniel opened stays open on the next event. The
// shell keeps each reveal in the card store, so after a restart a revealed card is drawn open.
function showCards(cards: StoredCard[]) {
  const list = element("cards");
  const ids = new Set(cards.map(({ request }) => request.tool_use_id));
  const drawn = new Set<string>();
  for (const node of [...list.children] as HTMLElement[]) {
    if (ids.has(node.dataset.id!)) drawn.add(node.dataset.id!);
    else node.remove();
  }
  for (const { request, card, revealed_at } of cards) {
    if (drawn.has(request.tool_use_id)) continue;
    const details = document.createElement("details");
    details.className = "card";
    details.dataset.id = request.tool_use_id;
    details.open = revealed_at !== null;
    details.addEventListener("toggle", () => {
      if (details.open) window.__TAURI__.core.invoke("reveal", { toolUseId: request.tool_use_id });
    });
    const question = document.createElement("summary");
    question.textContent = card.question;
    const answer = document.createElement("p");
    answer.className = "answer";
    answer.textContent = card.answer;
    details.append(question, answer);
    list.prepend(details);
  }
  element("cards-title").hidden = cards.length === 0;
}

window.__TAURI__.event.listen<Session | null>("now", (event) => show(event.payload));
window.__TAURI__.core.invoke<Session | null>("now").then(show);
window.__TAURI__.event.listen<StoredCard[]>("cards", (event) => showCards(event.payload));
window.__TAURI__.core.invoke<StoredCard[]>("cards").then(showCards);

export {};
