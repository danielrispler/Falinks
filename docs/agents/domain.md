# Domain Docs

## Layout

This repo has two contexts. The engine uses `CONTEXT.md` at the repo root and architecture decision records in `docs/adr/`. The studio, a control room for understanding agent work, uses `docs/studio/CONTEXT.md`. Use the context of the area you work in; a term defined in both means what its own context says.

## Before exploring

Read `CONTEXT.md` and ADRs relevant to the area you are exploring.
If they do not exist, proceed silently. The domain-modeling skill creates them when terms or decisions are resolved.

The engine source files are large. Find the file and entry point in the module map in `docs/engine.md`, then read that range.

## Vocabulary

Use the terms defined in `CONTEXT.md` when naming domain concepts in issues, proposals, hypotheses, and tests. If a needed term is missing, reconsider whether it fits the project or note the gap for domain-modeling.

## ADR conflicts

If a proposal contradicts an existing ADR, explicitly identify that ADR and explain why the decision should be reopened.
