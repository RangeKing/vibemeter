# Architecture

aftervibe is a Tauri 2 desktop application with a React frontend and a Rust core.

```text
Local agent records
  → provider adapters
  → normalized events and sessions
  → SQLite evidence store
  → range metrics, reviews, insights, and VCTI
  → React UI and deterministic share exports
```

## Frontend

`apps/desktop/src/` contains the React interface, Zustand state, TanStack Query calls, ECharts views, bilingual resources, and share controls. Navigation uses an application page key rather than a URL router.

## Rust core

`apps/desktop/src-tauri/src/` contains:

- `adapters/`: provider-specific parsing;
- `ingestion.rs`: source discovery and incremental indexing;
- `database.rs`: schema, queries, task grouping, and retention;
- `review_engine.rs`: deterministic evidence-backed review rules;
- `vcti.rs`: behavior features and personality matching;
- `providers.rs`: optional provider status and account-usage queries;
- `export.rs`: deterministic SVG and PNG rendering;
- `privacy.rs`: path, secret, and free-text sanitization.

## Data boundaries

Raw source records remain in their original directories and are read-only. The database stores normalized, bounded evidence rather than complete transcripts. Generated reviews and user edits are kept separate from normalized source data.

Provider-specific fields must not leak into shared React components or database queries. Each adapter exposes normalized records and capabilities; downstream features degrade when a capability is unavailable.
