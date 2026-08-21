# VibeMeter 官方 API 价格复核

复核时间：2026-08-21（Asia/Shanghai）

价格真相面是发版脚本生成的 [`pricing.generated.json`](../apps/desktop/src-tauri/pricing.generated.json)，而不是 Rust 源码中的手写价格表。脚本通过网络读取各供应商官网；任何来源页面解析不到最低数量的模型时，脚本失败并阻止发版，避免静默保留旧价格。

运行方式：

```sh
npm run pricing:update
```

本次在线运行结果：7 个官方来源、55 个模型。生成文件记录了抓取时间、来源 URL、页面 SHA-256 和各来源模型数，便于审计价格漂移。

## 官方来源与计价口径

| 来源 | 官方页面 | 本次模型数 | 口径 |
| --- | --- | ---: | --- |
| OpenAI / Codex | [API Pricing](https://developers.openai.com/api/docs/pricing) | 6 | USD / 1M tokens，取短上下文表 |
| Anthropic / Claude | [Claude API Pricing](https://platform.claude.com/docs/en/about-claude/pricing) | 15 | USD / MTok；保留 5m、1h cache write |
| DeepSeek | [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/) | 2 | 采用明确标注的 off-peak 价格 |
| Kimi / Moonshot | [Chat Pricing](https://platform.kimi.com/docs/pricing/chat) | 4 | CNY / 1M tokens，保留在目录但不伪装成 USD |
| Z.AI / GLM | [Models and Tools Pricing](https://docs.z.ai/guides/overview/pricing) | 19 | USD / 1M tokens |
| xAI / Grok | [Grok Models](https://docs.x.ai/developers/models) | 7 | USD / 1M tokens，取标准上下文价格 |
| Cursor / Composer | [Models & Pricing](https://cursor.com/docs/models-and-pricing) | 2 | USD / 1M tokens |

代码的输入、缓存命中、缓存写入和输出都以每百万 token 价格保存；估算时统一除以 1,000,000。页面未提供独立 cache-write 价格的模型保持 `null`，一旦观测到对应 token，成本显示为不可估算，而不是猜测。

## 关键模型核对

| 模型 | 输入 | 缓存命中 | 缓存写入 | 输出 | 结果 |
| --- | ---: | ---: | ---: | ---: | --- |
| `gpt-5.6-sol`（含 `gpt-5.6` 别名） | 5 | 0.50 | 6.25 | 30 | 与 OpenAI 短上下文表一致 |
| `gpt-5.6-terra` | 2 | 0.20 | 2.50 | 12 | 与 OpenAI 当前表一致 |
| `gpt-5.6-luna` | 0.20 | 0.02 | 0.25 | 1.20 | 与 OpenAI 当前表一致 |
| `claude-opus-4.6` / `claude-opus-5` | 5 | 0.50 | 6.25 / 1h 10 | 25 | 与 Anthropic 表一致 |
| `deepseek-v4-flash` | 0.22 | 0.007 | 0.22* | 0.66 | off-peak；*写入按 cache-miss 计 |
| `glm-5.3` / `glm-5.1` | 1.40 | 0.26 | — | 4.40 | 与 Z.AI 表一致 |
| `grok-build-0.1`（含 Grok Code Fast 别名） | 1 | 0.20 | — | 2 | 与 xAI 表一致 |
| `grok-4.6` | 2 | 0.50 | — | 6 | 与 xAI 表一致 |
| `composer-2.5` | 0.50 | 0.20 | — | 2.50 | 与 Cursor 表一致 |
| `kimi-k3` | ¥20 | ¥2 | — | ¥100 | CNY；未转换为 USD |

DeepSeek 官网同时给出 peak/off-peak 和 cache hit/cache miss。本地观测数据没有可靠的请求时间或供应商计价时段，因此使用文档明确的 off-peak 档；缓存命中与缓存未命中分别进入对应字段。Kimi 同理保留官网 CNY 价格，但 VibeMeter 的 UI 合约是 USD，当前不估算 Kimi 成本。

## 实施与回归

- `pricing.rs` 从生成目录读取价格，支持官网模型名、供应商前缀、版本后缀和官方别名；不再维护跨供应商的手写猜测价格。
- Data 页的 agent 过滤在过滤后的 daily points 上重新计算 API 等价成本，回归测试覆盖从两种 agent 到单一 agent 的金额变化。
- Grok Build 的 `modelUsage` 映射、累计用量增量、官方 `grok-4.6` / `grok-build-0.1` 价格和无 cache-write 价格行为均有 Rust 测试。
- `.github/workflows/release.yml` 的 validate 与两个架构构建 job 都在依赖安装/构建前运行 `npm run pricing:update`；任一官方页面结构异常都会使发版停止。

## 保守边界

- 长上下文价格没有混入标准上下文价格；脚本取各官网默认/短上下文表。
- CNY、缺少 cache-write 的价格不转换、不补猜测值。
- 价格目录由官网在线结果生成，不能把本地生成文件或历史快照解释成永久价格承诺；每次发版会重新抓取并记录新 SHA-256。
