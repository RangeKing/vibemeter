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
- Schema 18: clears legacy raw `live_events.payload_json`, keeps canonical and derived evidence intact, and adds encrypted seven-day `diagnostic_live_envelopes` for explicit diagnostic consent.
- Parser 6.1.0: scans complete local user/agent text, filters code, paths, secrets, and punctuation-only noise, then retains only ranked derived phrase counts without storing source text.
- VCTI algorithm 1.3.0: incorporates bounded waiting/error/completion signals from long-term live metrics.

Normal mode discards raw live envelopes after canonical normalization. Diagnostic mode is off by default; when explicitly enabled, encrypted envelopes expire after seven days and can be cleared early. Derived phrase, canonical live, and long-term metrics remain until local data or their project scope is explicitly cleared.

## Hook boundary

Hook installation occurs after onboarding, not during database discovery. VibeMeter modifies only these files when their provider directories exist:

- `~/.claude/settings.json`
- `~/.codex/hooks.json`
- `~/.codex/config.toml`

Existing structured content is merged and backed up. The managed Hook script and socket live under `~/.vibemeter`. Uninstall removes the managed Hook entries and script only; it does not attempt to restore a whole stale backup over current user configuration.

## Compatibility

Legacy identifiers remain only in the database locator and debug/test environment-variable fallbacks. They must not reappear in user-facing brand copy, active package identifiers, or the current application data path.

The separate `/Users/rangeking/Code/aftervibe` checkout is not modified during the VibeMeter transformation. The final VibeMeter repository inherits its Git history and removes the old remote.
