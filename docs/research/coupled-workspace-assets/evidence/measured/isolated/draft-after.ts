export function normalizeSku(value: string): string {
  return value.trim();
}

// These functions intentionally share a file, but own separate edits.
// A owns normalization; B owns presentation.
// No symbol reservation or automatic ownership detection is implemented.

export function displaySku(value: string): string {
  return undefined; // UNFINISHED_B
}
