# Security policy

Falinks runs agent-proposed edits and host-enrolled checks inside OS containment (Seatbelt on macOS, Landlock and seccomp on Linux). A flaw there matters, so please report it privately.

## Reporting a vulnerability

Use [GitHub private vulnerability reporting](https://github.com/danielrispler/Falinks/security/advisories/new). Do not open a public issue.

Include the platform, the Falinks commit, and the smallest reproduction you have. You will get an acknowledgement within 7 days.

## In scope

- A contained job or required check that writes outside its output directory, reaches the network, or reads controller state.
- An agent session that edits source without going through the engine, or acts as another client.
- Containment that silently falls back to weaker isolation instead of failing visibly.
- Recovery that overwrites drafts or loses retained evidence.

## Out of scope

- Behavior on platforms that [docs/platforms.md](docs/platforms.md) marks as not verified or unsupported. These fail closed by design.
- Known differences listed in [docs/platforms.md](docs/platforms.md), such as escaped descendants on macOS (#58).
- Hostile programs enrolled as trusted jobs. Jobs are trusted formatters and generators, not a sandbox for arbitrary code.

## Supported versions

Falinks is pre-1.0. Only the latest commit on `main` receives fixes.
