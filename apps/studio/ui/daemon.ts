// The terminal daemon, through the shell (#466): the requests and events of the protocol in
// `terminal-daemon/src/lib.rs`, with terminal bytes base64-encoded in `data`.

import { invoke, listen } from "./dom.js";

// What the shell answers on connect: the configured repository, where new terminals start.
export interface Config {
  repo: string;
}

// One daemon event: `spawned`, `error`, `replay`, `output`, `exit`, `list`, or `lost` from the shell.
export interface DaemonEvent {
  ev: string;
  id?: string;
  data?: string;
  cols?: number;
  rows?: number;
  exited?: string | null;
  status?: string;
  message?: string;
  [field: string]: unknown;
}

// Connects to the daemon, starting it if none runs; again after `lost`, it reconnects.
export const connect = () => invoke<Config>("terminal_connect");

// Sends one request, such as `{ op: "attach", id }`.
export const request = (msg: Record<string, unknown>) => invoke<void>("terminal_send", { msg });

// Every daemon event, for every terminal; each pane keeps those of its own id.
export const onEvent = (handler: (event: DaemonEvent) => void) => listen<DaemonEvent>("terminal", handler);

export const encode = (bytes: Uint8Array) => btoa(Array.from(bytes, (b) => String.fromCharCode(b)).join(""));
export const decode = (data: string) => Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
