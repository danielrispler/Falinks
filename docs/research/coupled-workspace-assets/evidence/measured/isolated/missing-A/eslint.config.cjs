const parser = require(process.env.TRIAL_TOOLS + "/node_modules/@typescript-eslint/parser/dist/index.js");
module.exports = [{files:["**/*.ts"], languageOptions:{parser}, rules:{"no-unreachable":"error", "no-debugger":"error", "eqeqeq":"error", "no-var":"error", "prefer-const":"error"}}];
