<p align="center">
  <img src="docs/assets/vibemeter-banner.png" alt="VibeMeter — a local-first AI coding activity tracker for macOS" width="100%">
</p>

<h1 align="center">VibeMeter</h1>

<p align="center">
  <strong>Know what your agents are doing.</strong><br>
  Track your agents. Discover your coding type.
</p>

<p align="center">
  <a href="https://github.com/RangeKing/vibemeter/releases"><img alt="Version" src="https://img.shields.io/badge/version-v0.1.3-9B87F5"></a>
  <a href="https://github.com/RangeKing/vibemeter/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/RangeKing/vibemeter/actions/workflows/ci.yml/badge.svg"></a>
  <img alt="macOS 14+" src="https://img.shields.io/badge/macOS-14%2B-222222?logo=apple">
  <img alt="Local first" src="https://img.shields.io/badge/data-local--first-79C7A3">
  <a href="LICENSE"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-F29A72"></a>
</p>

<p align="center">
  <a href="README.md">🇨🇳 中文</a> · 🇺🇸 <strong>English</strong>
</p>

VibeMeter brings live status, long-term analytics, and your VCTI profile into one local-first macOS app. A glance at the Notch tells you whether Claude Code or Codex is thinking, reading, using a tool, or waiting for you. Sessions, tokens, cost, models, tools, and Skill usage stay on your Mac and become a history you can inspect and share. Once the evidence is strong enough, 24 VCTI personalities turn that history into a recognizable picture of how you build with agents.

VibeMeter does not control your agents, approve requests, or modify source repositories. If a signal is unavailable, the UI says so instead of dressing missing data up as zero.

## ✨ Three views. One clear picture.

- **Understand at a glance.** The Notch shows live Claude Code and Codex stages, priority, and recent structured actions. The menu-bar popover covers time ranges, tokens, cost, activity trends, and remaining quota.
- **Learn from every session.** Data brings together sessions, input/output/cache tokens, time, cost, agents, models, tools, Skills, activity patterns, and work events.
- **Turn behavior into identity.** VCTI maps verifiable local behavior to 24 AI coding personalities, with dimensions, confidence, and evidence. Thin evidence stays explicitly incomplete.

Share and Settings remain utility surfaces. Catchphrases and insight cards live on VCTI; comparison bars and session replay live on Data. Sources remain a transitional route opened from Settings. The review workspace is intentionally not shipped in VibeMeter; its previous implementation remains archived in TokenGraph. `aftervibe` remains only as a legacy database migration identifier.

## 📸 Key screens

| VCTI profile | Live activity with Notch |
| --- | --- |
| ![VCTI profile](docs/assets/screenshots/vcti.png) | ![Live activity and Notch](docs/assets/screenshots/live.png) |
| Data | Share |
| ![Data page](docs/assets/screenshots/data.png) | ![Share page](docs/assets/screenshots/share.png) |

<p align="center"><img src="docs/assets/screenshots/menubar.png" width="420" alt="Menu-bar analytics popover"></p>

## 🧬 VCTI: 24 AI coding personalities

VCTI groups collaboration behavior into six stages with four personalities in each stage. The names make the patterns memorable; the result still comes from verifiable local behavior. When the evidence is too thin, VibeMeter keeps collecting instead of forcing a type.

![The 24 VCTI personalities](apps/desktop/src/assets/vcti/vcti-types-atlas-v2.webp)

### Starting Style

<p align="center"><img src="apps/desktop/src/assets/vcti/vcti-types-start-v2.png" width="620" alt="VCTI Starting Style: VIBE, SPEC, HACK, and MIX"></p>

| Code | Personality | In one line |
| --- | --- | --- |
| `VIBE` | Vibe Lead | The spec can wait; the feeling has to arrive first. |
| `SPEC` | Spec Owner | A task without acceptance criteria has not received a building permit. |
| `HACK` | Shortcut Hacker | The orthodox answer is still reading docs; your side route already runs. |
| `MIX` | Stack Stitcher | You turn wheels, frames, and engines into a vehicle that runs. |

### Agent Direction

<p align="center"><img src="apps/desktop/src/assets/vcti/vcti-types-agent-v2.png" width="620" alt="VCTI Agent Direction: YOLO, LOOP, BOSS, and SWARM"></p>

| Code | Personality | In one line |
| --- | --- | --- |
| `YOLO` | All-in Operator | Select all, execute, accept, pray—no wasted motion. |
| `LOOP` | One-more-version | There is no failure between you and the agent, only another version. |
| `BOSS` | Agent Foreman | You write less code and get better at arranging how code gets written. |
| `SWARM` | Parallel Maniac | The product is not live, but the agent org chart already is. |

### Quality Control

<p align="center"><img src="apps/desktop/src/assets/vcti/vcti-types-quality-v2.png" width="620" alt="VCTI Quality Control: DIFF, TEST, DOCS, and UNDO"></p>

| Code | Personality | In one line |
| --- | --- | --- |
| `DIFF` | Diff Supervisor | Agents may improvise; every line still answers for itself. |
| `TEST` | Test Gatekeeper | A page opening merely qualifies it to enter testing. |
| `DOCS` | Docs Diehard | Knowledge that only lives in chat is one cleanup away from extinction. |
| `UNDO` | Rollback Master | You let anything happen because you know how to make it unhappen. |

### Debug & Repair

<p align="center"><img src="apps/desktop/src/assets/vcti/vcti-types-debug-v2.png" width="620" alt="VCTI Debug and Repair: DEBUG, PATCH, STACK, and AUTO"></p>

| Code | Personality | In one line |
| --- | --- | --- |
| `DEBUG` | Bug Detective | Others see an error; you see an unorganized clue. |
| `PATCH` | Patch Hero | Stop the leak first; restoring service is the present priority. |
| `STACK` | Infra Maximalist | A button problem eventually gets its own service layer. |
| `AUTO` | Automation Maniac | Anything done manually twice is challenging your principles. |

### Delivery Rhythm

<p align="center"><img src="apps/desktop/src/assets/vcti/vcti-types-delivery-v2.png" width="620" alt="VCTI Delivery Rhythm: SHIP, RUSH, MVP, and DETAIL"></p>

| Code | Personality | In one line |
| --- | --- | --- |
| `SHIP` | Release Warrior | While others debate field names, your preview link is already in chat. |
| `RUSH` | Sprint Burner | You cruise steadily, then compress the final push into one intense closing window. |
| `MVP` | Barebones Builder | The flow works and the data stays put—time to invite the first users. |
| `DETAIL` | Detail Controller | The feature shipped long ago; the final two pixels have not. |

### Tool Relationship

<p align="center"><img src="apps/desktop/src/assets/vcti/vcti-types-tools-v2.png" width="620" alt="VCTI Tool Relationship: FORK, TOKEN, CACHE, and BUDDY"></p>

| Code | Personality | In one line |
| --- | --- | --- |
| `FORK` | Tool Hopper | Every new tool is a long-term relationship until the next one appears. |
| `TOKEN` | Token Accountant | Every model call opens a cost report in your head. |
| `CACHE` | Context Hoarder | Give the agent every background fact and it will find the answer somewhere. |
| `BUDDY` | Cyber Partner | A genuinely compatible agent is worth building a long relationship with. |

## ⚡ Live monitoring

After onboarding, VibeMeter can install managed local hooks for detected Claude Code and Codex installations:

- existing JSON and TOML configuration is merged rather than replaced;
- an existing configuration is backed up before the first change;
- the managed script sends events to a `0600` Unix socket under `~/.vibemeter`;
- the Notch shows structured status only, never raw prompts, commands, code, paths, or tool output;
- Codex phase refinement reads only event type, collaboration mode, tool name, and lifecycle timestamps—not prompts, responses, reasoning, code, paths, or tool arguments;
- background waiting and error transitions send silent notifications; background CLI completion may notify, while Codex Desktop completion never receives a duplicate VibeMeter notification;
- repair and uninstall touch only VibeMeter-managed entries.

The Notch disappears into the physical cutout while idle. During activity, a compact left wing shows Codex and Claude Code instance counts and the right wing shows one highest-priority state. Click or deliberately hover over the cutout/wings for about 300 ms to expand. A hover-opened panel collapses about 500 ms after exit; clicking elsewhere also collapses unless that expansion is temporarily pinned. Manual close and app restart reset the pin. Notch and menu-bar visibility can be controlled independently. Macs without a physical Notch keep the menu-bar and main-window paths.

## 💬 Catchphrases

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

## 🔐 Privacy boundaries

- Source histories and source repositories remain read-only.
- The VibeMeter index lives at `~/Library/Application Support/com.vibemeter.desktop/vibemeter.sqlite`.
- First launch copies the aftervibe database when available, otherwise the legacy TokenGraph database, using SQLite online backup.
- Git evidence and account-level Cursor usage remain opt-in and separate from local VCTI/session analytics.
- Share Guard blocks secret-like strings and absolute paths before export.

See [docs/privacy.md](docs/privacy.md) and [docs/vibemeter-migration.md](docs/vibemeter-migration.md).

## 🎨 Share Studio

Share Studio includes six public templates across usage, developer retrospective, agent comparison, session recap, VCTI identity, and catchphrases. It is preview-first, supports five common aspect-ratio presets in the UI, and exports deterministic PNG or SVG output in Simplified Chinese or English, light or dark. Every export passes through Share Guard before it leaves the app.

## 🧰 Development

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

Local builds are ad-hoc signed and are not notarized.

For a release, keep the versions in both `package.json` files, `Cargo.toml`, and `tauri.conf.json` in sync, then push the matching `vX.Y.Z` tag. GitHub Actions runs the full validation suite, builds Apple Silicon and Intel DMG and ZIP packages, and creates the GitHub Release. These artifacts are ad-hoc signed; Apple notarization remains a separate distribution step.

## 🗂️ Repository layout

```text
apps/desktop/src/                 React UI, state, charts, and localization
apps/desktop/src-tauri/src/       Rust adapters, live monitor, storage, and exports
apps/desktop/src-tauri/tests/     Rust integration tests
docs/                             Architecture, privacy, and migration records
```

## 🤝 Contributing

Issues and pull requests are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md) first. Use synthetic fixtures and screenshots; never commit real conversations, credentials, databases, or local build artifacts.

VibeMeter is available under the [MIT License](LICENSE). The bundled Space Grotesk font is distributed under the [SIL Open Font License 1.1](apps/desktop/src/assets/fonts/OFL.txt). VibeMeter is not affiliated with or endorsed by the coding-agent providers named above.
