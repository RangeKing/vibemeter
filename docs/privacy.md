# Privacy model

aftervibe is local-first. It can still encounter sensitive development data, so every feature is designed around explicit collection and export boundaries.

## Stored locally

- session and task timestamps;
- model and token metadata when available;
- bounded, sanitized goal and result excerpts;
- tool categories, success state, and duration;
- project-relative file paths and line-count summaries;
- optional read-only Git evidence;
- generated reviews, settings, and user edits.

## Not stored

- source-code contents or full diff bodies;
- complete transcripts or model responses;
- terminal output;
- environment-variable values;
- API keys, cookies, or provider credentials.

## Optional features

Git evidence, Prompt structure analysis, API-based deep review, and Cursor Dashboard usage require separate user choices. Cursor account usage is kept in memory and never merged into local session evidence.

## Export boundary

Share Guard checks export content for secrets, usernames, email addresses, absolute paths, and unreviewed free text. Project identity and file paths are hidden unless the user explicitly enables and reviews them.

## Source access

Agent directories and source repositories are read-only. aftervibe does not edit agent configuration, source files, Git state, or session history.
