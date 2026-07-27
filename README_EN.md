# VibeMeter

> Track your agents. Discover your coding type.

VibeMeter is a local-first macOS activity tracker for AI coding. It shows live Claude Code and Codex status in the MacBook Notch and menu bar, turns local agent sessions into traceable analytics and replay, and builds an evolving VCTI profile from observed behavior.

![VibeMeter icon](vibemeter.png)

## Core surfaces

- **VCTI:** Build an evidence-bound AI coding profile with explicit dimensions, confidence, and supporting behavior. Default landing page; insufficient data stays insufficient.
- **Live:** See whether Claude Code or Codex is running, waiting, in error, or complete. The Live page adds today's timeline, concurrency lanes, and waiting/error history. Jump back to the source app or terminal without approving requests or sending prompts.
- **Data:** Review sessions, tokens, time, cost, models, tools, activity, and work events.

Share and Settings are utility surfaces. Share Studio is preview-first with five common aspect presets and collapsed copy/display/metric controls. Catchphrases and insight cards live on VCTI; comparison bars and session replay live on Data. Sources remain a transitional route opened from Settings. The review workspace is intentionally not shipped in VibeMeter; its previous implementation remains archived in TokenGraph. `aftervibe` remains only as a legacy database migration identifier.

## Live monitoring

After onboarding, VibeMeter can install managed local hooks for detected Claude Code and Codex installations:

- existing JSON and TOML configuration is merged rather than replaced;
- an existing configuration is backed up before the first change;
- the managed script sends events to a `0600` Unix socket under `~/.vibemeter`;
- the Notch shows structured status only, never raw prompts, commands, code, paths, or tool output;
- Codex phase refinement reads only event type, collaboration mode, tool name, and lifecycle timestamps—not prompts, responses, reasoning, code, paths, or tool arguments;
- background waiting and error transitions send silent notifications; background CLI completion may notify, while Codex Desktop completion never receives a duplicate VibeMeter notification;
- repair and uninstall touch only VibeMeter-managed entries.

The Notch disappears into the physical cutout while idle. During activity, a compact left wing shows Codex and Claude Code instance counts and the right wing shows one highest-priority state. Click or deliberately hover over the cutout/wings for about 300 ms to expand. A hover-opened panel collapses about 500 ms after exit; clicking elsewhere also collapses unless that expansion is temporarily pinned. Manual close and app restart reset the pin. Notch and menu-bar visibility can be controlled independently. Macs without a physical Notch keep the menu-bar and main-window paths.

## Catchphrases

Historical source text is scanned transiently in local memory. VibeMeter stores only derived phrase counts, session counts, and source attribution:

- Chinese candidates contain 3–12 characters; English candidates contain 2–5 words;
- variable yes/no questions may be collapsed to a safe frame such as `你接受……吗` or `do you accept…?`; the variable body is not retained;
- a phrase must repeat across multiple sessions;
- code, paths, secret-like values, tool output, markup, punctuation-only tokens, and stopwords are filtered;
- client-generated transport scaffolding, including Codex attachment manifests and `My request for Codex` headings, is filtered;
- nested phrases with substantially overlapping session evidence are collapsed to the most complete expression;
- each role exposes at most eight phrases;
- font size represents frequency;
- Agent phrase backgrounds identify the dominant source; attribution prefers the recorded model and falls back to the Agent.

Raw live-hook envelopes are retained for at most 90 days. Long-term VCTI and activity features use derived metrics.

## Privacy boundaries

- Source histories and source repositories remain read-only.
- The VibeMeter index lives at `~/Library/Application Support/com.vibemeter.desktop/vibemeter.sqlite`.
- First launch copies the aftervibe database when available, otherwise the legacy TokenGraph database, using SQLite online backup.
- Git evidence and account-level Cursor usage remain opt-in and separate from local VCTI/session analytics.
- Share Guard blocks secret-like strings and absolute paths before export.

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
