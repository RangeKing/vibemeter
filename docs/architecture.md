# Architecture

VibeMeter is a Tauri 2 macOS application with a React frontend, a Rust core, and a local SQLite evidence store.

```text
Historical agent records                  Exact live event sources
  → read-only provider adapters             → Claude Code / Codex managed hooks
  → normalized sessions and evidence        → DeepSeek Harness read-only observer
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

Exact Claude Code and Codex hooks publish `live-hook` lifecycle evidence. The DeepSeek Harness, Kimi Code, and ZCode adapters convert their durable structured local session state into the same exact canonical lifecycle contract without installing a Hook or changing provider state. Historical evidence uses `history-index` and remains isolated from live queries.

Every active live session exposes one explainable work pulse with four independent dimensions: lifecycle, work phase, attention signal, and freshness. Exact sources may populate all four from canonical lifecycle evidence. Experimental observers populate recent activity and freshness only; lifecycle and attention stay explicitly unknown. Freshness is derived from the last structured update using fixed 30-second fresh and 120-second lost-update boundaries. Live renders the full pulse, while Notch renders a compact value from the same model.

Exact waiting and error facts open persistent attention episodes. Repeated canonical facts attach as evidence to the same unresolved episode; acknowledged and snoozed episodes remain unresolved until canonical progress resumes. Unresolved episodes expire after 24 hours. Episode identity, evidence links, rule version, evidence level, and source coverage live in SQLite so restart and replay do not duplicate alerts.

Attention actions use fixed feedback values only: handled, not relevant, not stuck, or snoozed. A successful jump acknowledges an episode but does not resolve waiting or error; a failed jump records an intervention and preserves the episode. Notifications are atomically claimed once per episode after the foreground-source check. Intervention records distinguish user-confirmed feedback and jumps, observed recovery, and inferred expiry.

High-confidence stuck detection is derived only from exact canonical evidence. Versioned rules require three matching failures within ten minutes, three matching operation starts without a valid progress fact, or three subagent-start failures. A valid finish, resume, agent stop, or explicit progress fact resolves the episode. Reading/editing repetition, isolated failures, silence, token-only activity, and successful verification are excluded. A user-confirmed “not stuck” response becomes a durable review sample for the quality gate.

The attention queue has one fixed priority: waiting, blocking error, high-confidence stuck, then completion review. Notch collapses to the first item and expands to the full queue. Live shows the current queue plus resolved, ignored, and expired history. Data links episodes and intervention counts back to both tasks and session replay. Completion review is resolved only by acknowledgement or a successful verified jump and never outranks an unresolved wait or error.

The local attention quality gate is deliberately hard. It requires at least 100 user-reviewed stuck samples, at least 90% precision, no more than 10% irrelevant or false-positive feedback, notification latency below two seconds at the 95th percentile, at least 95% verified jump success, and three real-app checks covering duplicate suppression, foreground silence, and privacy surfaces. Missing observations remain unavailable and the gate stays incomplete; development fixtures never count as real acceptance evidence.

Reindexing uses one transaction to mark the prior source generation, upsert the rebuilt generation, and commit only after all writes succeed. Stable source fingerprints let reordered records retain identity, absent records remain recoverable tombstones, and a failed generation leaves the previous visible result unchanged. User-owned reviews, manual task membership, attention feedback, and long-term VCTI snapshots have separate lifecycles.

## Data boundaries

Source records and repositories remain read-only. Historical text used for catchphrases is processed transiently and is not stored as phrase source text. Raw Hook envelopes are discarded after normalization by default. An explicitly enabled diagnostic mode encrypts them with a macOS Keychain-protected key for at most seven days; long-term features use canonical events, derived counters, and evidence references.

Provider-specific fields must not leak into shared UI or query contracts. A provider is considered supported only when its data is represented end-to-end in analytics, VCTI, replay, sharing, source status, and attribution. Exact live monitoring currently covers Claude Code, Codex, and DeepSeek Harness.

Preview and export share one deterministic render model. Privacy review and Share Guard run before every exposed export path.
