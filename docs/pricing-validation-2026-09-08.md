# Official pricing verification — 2026-09-08

Fetched provider pages using `npm run pricing:update`; the generated catalog records retrieval time, URLs, SHA-256 hashes and model counts. This run produced 71 models from seven official sources at `2026-09-08T01:13:32Z`.

| Model | Input | Cache read | Cache write | 1 h cache write | Output |
| --- | ---: | ---: | ---: | ---: | ---: |
| Claude Fable 5.1 | $10 | $0.25 | $12.50 (5 min) | $20 | $50 |
| GPT 6 Astra | $10 | $1 | $12.50 | Not published in this table | $50 |

All amounts are USD per million tokens at the standard API rate.

Sources opened and inspected:

- Anthropic: https://platform.claude.com/docs/en/about-claude/pricing — the six-column base-pricing row for Claude Fable 5.1. Its cache-read multiplier is 0.025 × base input, not the older 0.1 × multiplier.
- OpenAI: https://developers.openai.com/api/docs/pricing — the **Standard**, short-context row for `gpt-6-astra`. The updater reads the Standard table's complete server-rendered metadata, including rows collapsed behind “All models”. It never uses the first arbitrary visible table or the Batch/Flex/Fast table.

OpenAI also lists Astra long-context Standard rates of $20 input, $2 cached input, $25 cache write and $75 output. Fast short-context rates are $20 / $2 / $25 / $100. These are not substituted for the standard estimates: the current history cost contract does not consistently carry per-request context size, service tier or regional processing. Nor does estimated token cost represent a Codex/Claude subscription bill. This release adds standard model pricing, not full invoice reconciliation.

The updater now accepts four-column legacy model rows and missing cache-write rates. Removing the Standard table or either requested model fails the refresh instead of silently publishing stale or discounted prices. Runtime matching permits exact names, provider prefixes and dated snapshots; arbitrary future models or service-tier suffixes remain unknown. Existing non-USD prices remain recorded without inventing a currency conversion.

Validation: three offline parser tests cover table selection, source-shape failure and Fable's cache-write durations/read discount. Rust tests cover exact rates, dated/provider-qualified names, token arithmetic and rejecting unsupported Astra/Fable variants. The pricing tests are part of `npm run ci`.
