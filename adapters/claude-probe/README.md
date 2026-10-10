# Claude worker probe

Throwaway research probe for the Claude worker runtime. It is a standalone crate with its own `[workspace]` and `Cargo.lock`, outside the engine workspace. Method, launch flags and findings are in `docs/research/claude-worker-runtime.md`.

## Running it

- Set `CLAUDE_CONFIG_DIR` to a fresh temporary directory for every run, for example `CLAUDE_CONFIG_DIR="$(mktemp -d)"`. Probe runs must never write into your real `~/.claude`. Without it, `claude -p` writes into `~/.claude/projects/` even with `--no-session-persistence`.
- A separate config directory may not see the keychain login. This is untested; if `claude` reports that it is not logged in, stop and raise it with the user.
- Nested `claude -p` runs need a session outside auto mode. Raise this with the user before you start.
- Keep raw logs outside the repo. They can hold runtime messaging tokens. Commit only curated evidence under `evidence/`.
