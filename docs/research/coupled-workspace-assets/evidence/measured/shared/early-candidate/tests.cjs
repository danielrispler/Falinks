const assert = require("node:assert/strict");
const { normalizeSku, displaySku } = require("./dist/catalog");
const { checkout } = require("./dist/caller");
const phase = process.argv[2];
assert.equal(checkout("  abc  "), "SKU:abc");
assert.equal(checkout("é"), "SKU:é");
if (phase !== "base") {
  assert.throws(() => normalizeSku("   "), TypeError);
}
if (phase === "final") {
  assert.deepEqual(normalizeSku(" abc "), { key: "abc" });
  assert.equal(displaySku({ key: "abc" }), "SKU:abc");
  assert.equal(require("./dist/generated/tag").tag, "v2");
  assert.equal(require("./dist/generated/schema").schema, "key");
} else {
  assert.equal(normalizeSku(" abc "), "abc");
}
console.log(`fixed ${phase} assertions passed`);
