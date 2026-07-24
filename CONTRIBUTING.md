# Contributing to aftervibe

Thanks for helping improve aftervibe.

## Before you start

Please open an issue before a large architectural change. Small bug fixes, tests, documentation improvements, and adapter compatibility updates can go directly to a pull request.

Development requires macOS 14+, Node.js 22+, Rust stable, and Xcode Command Line Tools.

```sh
npm install
npm run dev
```

## Required checks

Run the full local check before opening a pull request:

```sh
npm run ci
```

For a release build:

```sh
npm run build
```

## Privacy rules

Coding-agent logs can contain source code, prompts, file paths, credentials, and personal data. Contributions must follow these rules:

- Never commit a real session transcript, local database, API key, access token, private repository name, or absolute home-directory path.
- Reduce parser fixtures to the smallest record that reproduces the behavior.
- Replace project names, prompts, paths, commit messages, and identifiers with synthetic values.
- Do not log raw payloads when parsing fails.
- Keep source directories read-only.
- Treat missing fields as unavailable, not as zero.

## Adapter changes

Keep provider-specific logic inside `apps/desktop/src-tauri/src/adapters/`. A parser change should include:

- a sanitized fixture or focused unit test;
- graceful handling of unknown record shapes;
- correct capability and provenance labels;
- no raw private payloads in errors or logs.

## UI and localization

Every user-facing string must exist in both `en-US` and `zh-CN`. Avoid hardcoded copy inside React components. Keep keyboard navigation, visible focus, reduced motion, and non-color status labels working.

## Pull requests

Describe:

1. what changed;
2. which data source or product surface is affected;
3. how privacy boundaries were preserved;
4. which checks you ran;
5. screenshots for visible UI changes.

By contributing, you agree that your contribution is licensed under the MIT License.
