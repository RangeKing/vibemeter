# Architecture

VibeMeter is a Tauri 2 macOS application with a React frontend, a Rust core, and a local SQLite evidence store.

```text
Historical agent records                  Claude Code / Codex Hook events
  → read-only provider adapters             → managed Python bridge
  → normalized sessions and evidence        → 0600 local Unix socket
                 ↘                         ↙
                    SQLite evidence store
                    ├─ range analytics and work events
                    ├─ session ledger and replay
                    ├─ catchphrase counts and attribution
                    ├─ VCTI derived behavior
                    └─ deterministic share rendering
                                 ↓
                     React UI / Notch / menu bar
```

## Frontend

`apps/desktop/src/` contains the React interface, Zustand state, TanStack Query calls, ECharts views, bilingual resources, the Notch surface, and share controls. `Live`, `Data`, and `VCTI` are the three primary surfaces.

## Rust core

`apps/desktop/src-tauri/src/` contains:

- `adapters/`: provider-specific historical parsing;
- `ingestion.rs`: source discovery and incremental indexing;
- `live.rs`: Hook installation, socket ingestion, status mapping, notification, and jump-back behavior;
- `migration.rs`: copy-forward database discovery and SQLite online backup;
- `database.rs`: schema, queries, task grouping, phrase/live storage, and retention;
- `phrases.rs`: deterministic local phrase extraction and compaction;
- `skill_usage.rs`: explicit Skill-use extraction and aggregation;
- `pricing.rs`: API-equivalent model pricing with dated aliases;
- `vcti.rs`: behavior features, sample gates, and type matching;
- `providers.rs`: optional provider status and account-usage queries;
- `export.rs`: deterministic SVG and PNG rendering;
- `privacy.rs`: path, secret, title, and export sanitization.

## Data boundaries

Source records and repositories remain read-only. Historical text used for catchphrases is processed transiently and is not stored as phrase source text. Raw Hook envelopes are discarded after normalization by default. An explicitly enabled diagnostic mode encrypts them with a macOS Keychain-protected key for at most seven days; long-term features use canonical events, derived counters, and evidence references.

Provider-specific fields must not leak into shared UI or query contracts. A provider is considered supported only when its data is represented end-to-end in analytics, VCTI, replay, sharing, source status, and attribution. Exact live monitoring is currently limited to Claude Code and Codex.

Preview and export share one deterministic render model. Privacy review and Share Guard run before every exposed export path.
