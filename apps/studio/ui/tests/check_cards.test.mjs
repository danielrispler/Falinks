// The verdict on a check's card (#476): the check's result settles the guess. Run: `npm test` in
// apps/studio.
import { test } from "node:test";
import assert from "node:assert/strict";
import { checkVerdict } from "../../dist/cards.js";

const guess = (right) => ({ option: 1, right, at: 1793264010000 });

test("while the check runs, a tap waits for its result and no tap asks for one", () => {
  assert.equal(checkVerdict(null, null), null);
  assert.equal(checkVerdict(null, guess(false)), "The check still runs; its result settles your guess.");
});

test("the result says whether the guess was right, or only the result with no guess", () => {
  assert.equal(checkVerdict("failed", guess(true)), "It failed. You were right.");
  assert.equal(checkVerdict("passed", guess(false)), "It passed. You were wrong.");
  assert.equal(checkVerdict("passed", null), "It passed.");
});

test("an exit code that does not say counts no guess", () => {
  assert.match(checkVerdict("not_counted", guess(false)), /no guess counts/);
});
