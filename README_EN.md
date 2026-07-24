<div align="center">
  <img src="docs/assets/aftervibe-banner.png" width="100%" alt="aftervibe, a local-first coding-agent review app">

  <h1>aftervibe</h1>

  <p><strong>Review the work behind the vibe.</strong></p>

  <p>
    <img alt="macOS 14+" src="https://img.shields.io/badge/macOS-14%2B-000000?style=for-the-badge&logo=apple&logoColor=white">
    <img alt="Tauri 2" src="https://img.shields.io/badge/Tauri-2-24C8DB?style=for-the-badge&logo=tauri&logoColor=white">
    <img alt="Rust stable" src="https://img.shields.io/badge/Rust-stable-000000?style=for-the-badge&logo=rust&logoColor=white">
    <img alt="React 19" src="https://img.shields.io/badge/React-19-61DAFB?style=for-the-badge&logo=react&logoColor=0B1F33">
    <img alt="TypeScript 6" src="https://img.shields.io/badge/TypeScript-6-3178C6?style=for-the-badge&logo=typescript&logoColor=white">
  </p>

  <p>
    <img alt="Local first" src="https://img.shields.io/badge/privacy-local--first-6B5BFF?style=flat-square&logo=shield&logoColor=white">
    <img alt="SQLite" src="https://img.shields.io/badge/storage-SQLite-003B57?style=flat-square&logo=sqlite&logoColor=white">
    <img alt="Simplified Chinese and English" src="https://img.shields.io/badge/i18n-中文%20%7C%20English-EF4444?style=flat-square">
    <img alt="MIT License" src="https://img.shields.io/badge/license-MIT-2EA44F?style=flat-square">
    <img alt="Open source" src="https://img.shields.io/badge/open%20source-yes-F59E0B?style=flat-square&logo=github&logoColor=white">
  </p>

  <p>
    <a href="README.md">🇨🇳 中文</a>
    ·
    🇺🇸 English
  </p>
</div>

---

aftervibe is a local-first macOS app for understanding how you work with coding agents. It turns scattered sessions, tool calls, file changes, test activity, and Git evidence into traceable usage analytics, process replays, work reviews, long-term insights, and shareable cards.

aftervibe does not launch or control your agents, and it never modifies your source repositories. It reads existing local records in read-only mode and shows an explicit unavailable or unrecorded state whenever the evidence is incomplete.

### ✨ Highlights

- 📊 **Usage overview:** Explore sessions, tokens, active time, models, and tools across today, 7, 30, 90, 180, or 365 days.
- 🎬 **Session replay:** Reconstruct inspection, editing, testing, fixing, and verification from observed events.
- 🔎 **Evidence-backed reviews:** Separate facts, inferences, and suggestions, with links back to supporting evidence whenever possible.
- 🧬 **VCTI personality:** Generate a Vibe Coding personality from real collaboration patterns without completing a questionnaire.
- 🎨 **Share cards:** Export deterministic PNG and SVG cards, or copy a rendered image directly.
- 💳 **Cursor account usage:** Opt in to range-aware Dashboard token and cost reporting. Account-wide data stays separate from local session evidence.
- 🌏 **Bilingual UI:** Use the app and its share templates in Simplified Chinese or English.

### 🤖 Supported data sources

aftervibe looks for readable local records from:

- Claude Code
- Codex
- Cursor
- Kimi Code
- OpenClaw
- Hermes

Each tool exposes a different set of fields, so some metrics may be unavailable or unrecorded. Every adapter keeps source directories read-only, skips unknown records safely, and avoids exposing raw private payloads when parsing fails.

### 🔐 Privacy boundaries

- 🏠 The local index lives at `~/Library/Application Support/com.aftervibe.desktop/aftervibe.sqlite`.
- 🚫 aftervibe does not store source code, full diffs, terminal output, environment-variable values, API keys, or complete model responses.
- 🎛️ Git evidence, prompt-structure analysis, and Cursor Dashboard usage each have a separate switch.
- 🛡️ Share Guard checks exported content; project names, file paths, and free text remain hidden by default.
- 👀 Deep review sends only the bounded, sanitized payload shown on the confirmation screen.

See the full [privacy model](docs/privacy.md).

### 🧰 Requirements

- macOS 14 or later
- Node.js 22 or later
- Rust stable with `rustfmt` and `clippy`
- Xcode Command Line Tools

### 🚀 Start developing

```sh
npm install
npm run dev
```

### 🧪 Run checks

```sh
npm run check
npm test
npm run check:rust
npm run test:rust
```

Run the complete local CI sequence with:

```sh
npm run ci
```

### 🏗️ Build the macOS app

```sh
npm run build
```

The app bundle is written to:

```text
apps/desktop/src-tauri/target/release/bundle/macos/aftervibe.app
```

### 🔑 Optional environment variables

aftervibe never writes API keys to its database. It reads these variables only when you manually select an API-based deep review:

```text
OPENAI_API_KEY
ANTHROPIC_API_KEY
```

Development and export tests can also use:

```text
AFTERVIBE_TEST_DB
AFTERVIBE_PREVIEW_MENUBAR
AFTERVIBE_EXPORT_MATRIX_DIR
AFTERVIBE_EXPORT_TEMPLATE
```

### 🗂️ Repository layout

```text
apps/desktop/src/                 React UI, state, charts, and localization
apps/desktop/src-tauri/src/       Rust adapters, indexing, reviews, and exports
apps/desktop/src-tauri/tests/     Rust integration tests
docs/                             Architecture and privacy documentation
```

Stack: Tauri 2, Rust, React, TypeScript, SQLite, and ECharts.

### 🤝 Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before sending a change. If you need to demonstrate a real session format, commit only the smallest sanitized fixture. Never upload your complete records, database, or exports.

Report security and privacy issues privately by following [SECURITY.md](SECURITY.md). Do not attach sensitive records to a public issue.

### 📄 License

aftervibe is available under the [MIT License](LICENSE).

Space Grotesk is distributed under the [SIL Open Font License](apps/desktop/src/assets/fonts/OFL.txt) included with the font files.

aftervibe is not affiliated with or endorsed by the coding-agent providers named above. Their names and trademarks belong to their respective owners.
