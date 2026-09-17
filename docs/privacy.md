# Privacy model

VibeMeter is local-first. It can encounter sensitive development data, so collection, live monitoring, retention, and export have separate boundaries.

## Stored locally

- session/task timestamps and bounded normalized events;
- model and token metadata when available;
- bounded, sanitized goal and result excerpts;
- tool categories, success state, and duration;
- project-relative file evidence and line-count summaries;
- optional read-only Git evidence;
- generated reviews, settings, and user edits;
- fixed-choice attention feedback linked to canonical evidence;
- derived execution relations and canonical evidence references for Delegation Trace;
- derived memory read/write activity metadata and canonical evidence references for Memory Ledger;
- derived catchphrase counts, session counts, and Agent attribution;
- raw local Hook envelopes only when the user enables diagnostic mode, encrypted with a platform-secured key and deleted after seven days;
- long-term derived live counters used by activity analytics and VCTI.

## Not stored

- source-code contents or full diff bodies;
- complete transcripts or model responses;
- terminal environment-variable values;
- API keys, cookies, or provider credentials;
- historical source text solely because it was scanned for catchphrases;
- raw prompts, commands, code, paths, or tool output in the Notch.
- provider-private event names, tool input, prompt text, response text, or raw source identifiers in Delegation UI labels.
- memory contents, recalled text, memory paths, write payloads, or an inferred claim that a read caused a write.

## Agent configuration

VibeMeter does not change Agent configuration until onboarding is complete. For detected Claude Code and Codex installations, it can then add a managed local Hook:

- existing JSON/TOML content is structurally merged;
- the original config is backed up before its first change;
- unrelated Hook commands and settings are preserved;
- repair is idempotent;
- uninstall removes only VibeMeter-managed entries and its managed script;
- a shared Codex feature flag is not disabled during uninstall.

The managed script sends bounded events only to `~/.vibemeter/vibemeter.sock`, which is created with mode `0600`.

DeepSeek Harness uses no VibeMeter-managed Hook. VibeMeter reads its local structured session records in place, normalizes only bounded lifecycle and analytics evidence, and never edits Harness configuration or source sessions.

## Optional features

Git evidence, prompt-structure analysis, and Cursor Dashboard usage require separate user choices. Account-level Cursor usage is kept separate from local session evidence and does not affect local VCTI.

Diagnostic retention is also off by default. Normal live processing discards the unredacted envelope after a canonical event forms. When the user explicitly enables diagnostics, encrypted envelopes remain in VibeMeter's application database for no more than seven days; macOS Keychain protects the encryption key. If secure storage is unavailable, VibeMeter refuses to enable the mode instead of storing plaintext. The user can clear the envelopes and key early from Settings.

## Export boundary

Share Guard checks export content for secrets, absolute paths, email addresses, repository URLs, and unreviewed free text. Project identity and file paths remain hidden unless the user explicitly enables and reviews them. Preview and export use the same sanitized model.

## Source access and migration

Historical Agent directories and source repositories are read-only. First launch copies an existing aftervibe or legacy TokenGraph database through SQLite online backup into VibeMeter’s own application directory. The source database, WAL, and SHM are not migration targets. Schema contraction runs only on a staged VibeMeter copy and installs it after integrity and canonical-coverage checks. Reindexing tombstones replaceable source evidence instead of deleting user confirmations, manual task membership, attention feedback, or long-term snapshots.

Delegation adapters inspect only source-provided structured identity and lifecycle fields. Shared DTOs expose internal session references, hashed node labels, normalized relation/event types, evidence levels, coverage, confidence, and canonical IDs. They do not expose the source payloads used to extract those fields.

Memory adapters inspect only source-provided structured activity signals. Path data used to recognize a supported local memory activity is discarded before canonicalization. Shared DTOs expose only the normalized operation, safe session/work-unit references, time, evidence level, source coverage, confidence, algorithm version, and canonical evidence ID.

## Edge sidebar

The optional edge panel reads the existing provider quota snapshot: the same rate-window readings the menu bar shows, refreshed through the existing provider commands and only while the credentials permission is on. It adds no source reader, global input hook, telemetry, approval or execution capability. Hover is handled only inside the panel's own window; periodic checks read display geometry, not cursor or application activity. Pinning and dragging change only VibeMeter's presentation: a drag reports the pointer's position inside the panel's own window while the button is held, and stores one number — where along the edge the strip sits.
