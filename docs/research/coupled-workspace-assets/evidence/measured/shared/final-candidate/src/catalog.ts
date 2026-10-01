export function normalizeSku(value: string): { key: string } {
  const key = value.trim();
  if (!key) {
    throw new TypeError("SKU must not be blank");
  }
  return { key };
}

// These functions intentionally share a file, but own separate edits.
// A owns normalization; B owns presentation.
// No symbol reservation or automatic ownership detection is implemented.

export function displaySku(value: { key: string }): string {
  return `SKU:${value.key}`;
}
