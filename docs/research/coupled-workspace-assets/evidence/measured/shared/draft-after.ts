export function normalizeSku(value: string): string {
  const key = value.trim();
  if (!key) {
    throw new TypeError("SKU must not be blank");
  }
  return key;
}

// These functions intentionally share a file, but own separate edits.
// A owns normalization; B owns presentation.
// No symbol reservation or automatic ownership detection is implemented.

export function displaySku(value: string): string {
  return undefined; // UNFINISHED_B
}
