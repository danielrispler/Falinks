import { normalizeSku, displaySku } from "./catalog";

export function checkout(value: string): string {
  return displaySku(normalizeSku(value));
}
