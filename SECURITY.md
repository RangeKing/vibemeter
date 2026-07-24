# Security Policy

aftervibe reads sensitive local development metadata, so privacy and data-boundary bugs are treated as security issues.

## Reporting a vulnerability

Please use GitHub's private vulnerability reporting feature for the repository. Do not open a public issue if the report contains:

- credentials or secret-like strings;
- source code or private prompts;
- absolute local paths;
- private repository or organization names;
- a database, session transcript, or export containing personal data.

Include the affected version, a minimal reproduction, expected behavior, and the observed impact. Use synthetic data whenever possible.

## Scope

Security-sensitive areas include local session parsing, path sanitization, secret detection, share exports, deep-review payloads, provider authentication reuse, database retention, and read-only source handling.

There is currently no guaranteed response or patch timeline. Maintainers will acknowledge and triage reports as capacity allows.
