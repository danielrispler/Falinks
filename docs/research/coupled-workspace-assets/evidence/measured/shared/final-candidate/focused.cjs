const fs = require("node:fs");
const assert = require("node:assert/strict");
const ts = require(process.env.TRIAL_TOOLS + "/node_modules/typescript");
const code = ts.transpileModule(fs.readFileSync("src/catalog.ts","utf8"), {compilerOptions:{module:ts.ModuleKind.CommonJS}}).outputText;
const m = {exports:{}}; new Function("exports",code)(m.exports);
const mode = process.argv[2];
if(mode === "B-final") assert.equal(m.exports.displaySku({key:"abc"}), "SKU:abc");
else {assert.deepEqual(m.exports.normalizeSku(" abc "), mode === "A-final" ? {key:"abc"} : "abc"); assert.throws(()=>m.exports.normalizeSku(" "),TypeError);}
console.log(`focused ${mode} passed; not publication authorization`);
