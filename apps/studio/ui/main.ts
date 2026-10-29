// The side pane: the state and "now" line of the session Daniel prompted last. The core decides what
// it shows; this file only draws what the shell sends, on load and on each "now" event.

type Step = { tool: string; detail: string | null };
type SessionState = "working" | "needs_you" | "your_turn";
type Session = { id: string; cwd: string; last_step: Step | null; state: SessionState };

const stateLabels: Record<SessionState, string> = {
  working: "Working",
  needs_you: "Needs you",
  your_turn: "Your turn",
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

window.__TAURI__.event.listen<Session | null>("now", (event) => show(event.payload));
window.__TAURI__.core.invoke<Session | null>("now").then(show);

export {};
