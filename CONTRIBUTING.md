# Contributing to VibeMeter

Development requires macOS 14+, Node.js 22+, Rust stable, and Xcode Command Line Tools.

```sh
npm install
npm run dev
```

Run the full local gate before opening a pull request:

```sh
npm run ci
```

## Privacy

Coding-agent histories can contain source code, prompts, file paths, credentials, and personal data.

- Never commit a real transcript, local database, credential, private repository name, or absolute home path.
- Reduce parser fixtures to the smallest synthetic record that reproduces the behavior.
- Do not log raw payloads when parsing fails.
- Keep source histories and repositories read-only.
- Treat missing data as unavailable, not zero.
- Preserve unrelated Claude Code and Codex Hook configuration.

## Product changes

Adapter and live-provider changes must be carried through analytics, VCTI, replay/review, sharing, source capability, attribution, tests, and documentation. Visible UI work requires a screenshot from the running Tauri app; export work requires a real SVG/PNG check.

Every user-facing string must exist in `en-US` and `zh-CN`.

Contributions are licensed under the MIT License.
