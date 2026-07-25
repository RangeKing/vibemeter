# Security policy

VibeMeter reads sensitive local development metadata. Privacy, retention, Hook-integrity, and export-boundary defects are security issues.

Use the repository's private vulnerability reporting feature. Do not open a public report containing credentials, source code, private prompts, absolute paths, repository identities, databases, or transcripts.

Security-sensitive areas include:

- local session parsing and path sanitization;
- Hook config merge, backup, repair, and uninstall;
- Unix socket permissions and payload bounds;
- 90-day raw live-event retention;
- secret detection and Share Guard;
- deep-review payload construction;
- provider authentication reuse;
- read-only source handling.

Reports should include the affected version, a minimal synthetic reproduction, expected behavior, and observed impact.
