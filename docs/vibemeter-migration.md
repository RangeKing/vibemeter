# VibeMeter migration record

This document records the migration boundary from aftervibe / TokenGraph to VibeMeter. The running application and Rust migrations are authoritative.

## Identity

| Field | VibeMeter |
| --- | --- |
| Product name | `VibeMeter` |
| Bundle identifier | `com.vibemeter.desktop` |
| Database | `vibemeter.sqlite` |
| Rust package / library | `vibemeter` / `vibemeter_lib` |
| npm workspace | `@vibemeter/desktop` |

`Aftervibe` remains the user-facing name for post-session reviews.

## Copy-forward order

At first launch, VibeMeter chooses one source:

1. `~/Library/Application Support/com.aftervibe.desktop/aftervibe.sqlite`
2. `~/Library/Application Support/com.tokengraph.desktop/TokenGraph.sqlite`

The selected source is copied into:

```text
~/Library/Application Support/com.vibemeter.desktop/vibemeter.sqlite
```

The copy uses SQLite online backup, followed by integrity and schema checks. The old database, WAL, and SHM remain untouched. Once a VibeMeter database exists, it is authoritative and the copy-forward step is not repeated.

## Schema additions

- Schema 7: derived `phrase_usage` for user and Agent catchphrases.
- Schema 8: 90-day `live_events` plus long-term `live_session_metrics`.
- Schema 14–17: canonical live and historical evidence, activity cycles, partial-history coverage, and private source-record receipts.
- Schema 18: clears legacy raw `live_events.payload_json`, keeps canonical and derived evidence intact, and adds encrypted seven-day `diagnostic_live_envelopes` for explicit diagnostic consent.
- Schema 19: verifies canonical coverage, preserves duration and stable source fingerprints, then removes the legacy `events` and `live_events` tables. It also establishes durable user-confirmed attention feedback.
- Schemas 20–23: add durable attention episodes, evidence and intervention links, reviewed quality samples, and local quality checks. These records are user-visible derived state; source reindexing does not overwrite user feedback.
- Schema 24: adds privacy-safe operation identities so repeated-failure and repeated-operation rules distinguish different tools without storing their names, prompts, commands, or paths.
- Schema 25: separates notification claims from confirmed system delivery. Failed delivery releases its short-lived claim, and only confirmed delivery contributes to the attention latency quality gate.
- Schema 26: adds bounded attention-history and general session-association indexes. Notch snapshots read only the active queue, while expired-state maintenance is throttled and history is loaded in pages.
- Schema 27: repairs databases that briefly stored the database migration number in the canonical event schema field; canonical event records remain on protocol schema 20.
- Parser 6.1.0: scans complete local user/agent text, filters code, paths, secrets, and punctuation-only noise, then retains only ranked derived phrase counts without storing source text.
- VCTI algorithm 1.3.0: incorporates bounded waiting/error/completion signals from long-term live metrics.

Normal mode discards raw live envelopes after canonical normalization. Diagnostic mode is off by default; when explicitly enabled, encrypted envelopes expire after seven days and can be cleared early. Derived phrase, canonical live, and long-term metrics remain until local data or their project scope is explicitly cleared.

## Canonical contraction and reindexing

Schema 19 uses an expand, verify, contract migration on a staged database copy. The migration first adds any canonical fields required by visible queries, backfills legacy historical and live evidence, and rejects contraction if any legacy row lacks an active canonical counterpart. Only then are the two legacy evidence tables dropped and the staged copy installed.

Historical reindexing publishes one transaction at a time. Existing canonical facts are marked, the new generation is rebuilt by stable source identity, and matching facts are reactivated before commit. A parse, write, or confirmation failure rolls the whole transaction back, so the previously visible generation remains usable. Removed source records stay as tombstones and recover the same canonical identity if they reappear. User-edited reviews, manual work-unit membership, attention feedback, and VCTI snapshots are outside the replaceable source generation and are not overwritten by reindexing.

Schemas 20–27 use the same staged-copy migration and rollback boundary as schema 19. A database is installed only after the latest schema version and integrity checks pass; an interrupted migration keeps the previous database and its validated rollback artifact available for recovery.

## Hook boundary

Hook installation occurs after onboarding, not during database discovery. VibeMeter modifies only these files when their provider directories exist:

- `~/.claude/settings.json`
- `~/.codex/hooks.json`
- `~/.codex/config.toml`

Existing structured content is merged and backed up. The managed Hook script and socket live under `~/.vibemeter`. Uninstall removes the managed Hook entries and script only; it does not attempt to restore a whole stale backup over current user configuration.

## Compatibility

Legacy identifiers remain only in the database locator and debug/test environment-variable fallbacks. They must not reappear in user-facing brand copy, active package identifiers, or the current application data path.

The separate `/Users/rangeking/Code/aftervibe` checkout is not modified during the VibeMeter transformation. The final VibeMeter repository inherits its Git history and removes the old remote.
