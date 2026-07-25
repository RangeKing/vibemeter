# VibeMeter

> Track your agents. Discover your coding type.

VibeMeter is a local-first macOS activity tracker for AI coding. It shows live Claude Code and Codex status in the MacBook Notch and menu bar, turns local agent sessions into traceable analytics and reviews, and builds an evolving VCTI profile from observed behavior.

![VibeMeter icon](vibemeter.png)

## Core surfaces

- **Live:** See whether Claude Code or Codex is running, waiting, in error, or complete. Jump back to the source app or terminal without approving requests or sending prompts.
- **Data:** Review sessions, tokens, time, cost, models, tools, activity, and work events. “My Catchphrases” and “Agent Catchphrases” are derived locally without an LLM.
- **VCTI:** Build an evidence-bound AI coding profile with explicit dimensions, confidence, and supporting behavior. Insufficient data stays insufficient.

Sessions, Aftervibe post-session reviews, Insights, Share, Sources, and Settings remain available as secondary capabilities.

## Live monitoring

After onboarding, VibeMeter can install managed local hooks for detected Claude Code and Codex installations:

- existing JSON and TOML configuration is merged rather than replaced;
- an existing configuration is backed up before the first change;
- the managed script sends events to a `0600` Unix socket under `~/.vibemeter`;
- the Notch shows structured status only, never raw prompts, commands, code, paths, or tool output;
- notifications are limited to waiting and error transitions while the source is in the background;
- repair and uninstall touch only VibeMeter-managed entries.

Notch and menu-bar visibility can be controlled independently. Macs without a physical Notch keep the menu-bar and main-window paths.

## Catchphrases

Historical source text is scanned transiently in local memory. VibeMeter stores only derived phrase counts, session counts, and Agent attribution:

- Chinese candidates contain 2–8 characters; English candidates contain 1–3 words;
- a phrase must repeat across multiple sessions;
- code, paths, secret-like values, tool output, markup, punctuation-only tokens, and stopwords are filtered;
- font size represents frequency;
- Agent phrase backgrounds identify the dominant source, with a legend and hover attribution.

Raw live-hook envelopes are retained for at most 90 days. Long-term VCTI and activity features use derived metrics.

## Privacy boundaries

- Source histories and source repositories remain read-only.
- The VibeMeter index lives at `~/Library/Application Support/com.vibemeter.desktop/vibemeter.sqlite`.
- First launch copies the aftervibe database when available, otherwise the legacy TokenGraph database, using SQLite online backup.
- Git evidence and account-level Cursor usage remain opt-in and separate from local VCTI/session analytics.
- Share Guard blocks secret-like strings and absolute paths before export.
- Deep review sends only the bounded payload shown on its confirmation screen.

See [docs/privacy.md](docs/privacy.md) and [docs/vibemeter-migration.md](docs/vibemeter-migration.md).

## Development

Requirements: macOS 14+, Node.js 22+, Rust stable, and Xcode Command Line Tools.

```sh
npm install
npm run ci
npm run build
```

The Tauri bundle is written to:

```text
apps/desktop/src-tauri/target/release/bundle/macos/VibeMeter.app
```

The checked local delivery copy is `release/VibeMeter.app`. It is currently Apple Silicon and ad-hoc signed, not notarized.

## Repository layout

```text
apps/desktop/src/                 React UI, state, charts, and localization
apps/desktop/src-tauri/src/       Rust adapters, live monitor, storage, and exports
apps/desktop/src-tauri/tests/     Rust integration tests
docs/                             Architecture, privacy, and migration records
```

VibeMeter is available under the [MIT License](LICENSE). It is not affiliated with or endorsed by the coding-agent providers named above.
