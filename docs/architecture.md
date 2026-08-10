# Architecture

VibeMeter is a Tauri 2 macOS application with a React frontend, a Rust core, and a local SQLite evidence store.

```text
Historical agent records                  Claude Code / Codex Hook events
  → read-only provider adapters             → managed Python bridge
  → normalized sessions and evidence        → 0600 local Unix socket
                 ↘                         ↙
                    SQLite evidence store
                    ├─ canonical event ledger and activity cycles
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

## Evidence contract

`canonical_events` is the only event-level fact source used by Live and historical replay. Provider adapters may produce richer source records transiently, but shared queries consume normalized event types, lifecycle state, process phase, evidence level, coverage, privacy level, stable identity, and tombstone state. Session aggregates, activity cycles, tasks, and VCTI are derived models; they do not reintroduce provider-specific event tables.

Exact Claude Code and Codex hooks publish `live-hook` lifecycle evidence. Experimental Kimi Code and ZCode observers publish `live-observer` recent-activity evidence only and cannot create exact waiting, error, completion, or activity-cycle claims. Historical evidence uses `history-index` and remains isolated from live queries.

Every active live session exposes one explainable work pulse with four independent dimensions: lifecycle, work phase, attention signal, and freshness. Exact sources may populate all four from canonical lifecycle evidence. Experimental observers populate recent activity and freshness only; lifecycle and attention stay explicitly unknown. Freshness is derived from the last structured update using fixed 30-second fresh and 120-second lost-update boundaries. Live renders the full pulse, while Notch renders a compact value from the same model.

Exact waiting and error facts open persistent attention episodes. Repeated canonical facts attach as evidence to the same unresolved episode; acknowledged and snoozed episodes remain unresolved until canonical progress resumes. Unresolved episodes expire after 24 hours. Episode identity, evidence links, rule version, evidence level, and source coverage live in SQLite so restart and replay do not duplicate alerts.

Reindexing uses one transaction to mark the prior source generation, upsert the rebuilt generation, and commit only after all writes succeed. Stable source fingerprints let reordered records retain identity, absent records remain recoverable tombstones, and a failed generation leaves the previous visible result unchanged. User-owned reviews, manual task membership, attention feedback, and long-term VCTI snapshots have separate lifecycles.

## Data boundaries

Source records and repositories remain read-only. Historical text used for catchphrases is processed transiently and is not stored as phrase source text. Raw Hook envelopes are discarded after normalization by default. An explicitly enabled diagnostic mode encrypts them with a macOS Keychain-protected key for at most seven days; long-term features use canonical events, derived counters, and evidence references.

Provider-specific fields must not leak into shared UI or query contracts. A provider is considered supported only when its data is represented end-to-end in analytics, VCTI, replay, sharing, source status, and attribution. Exact live monitoring is currently limited to Claude Code and Codex.

Preview and export share one deterministic render model. Privacy review and Share Guard run before every exposed export path.
