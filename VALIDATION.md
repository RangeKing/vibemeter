# VibeMeter 验收记录

## v0.6.0 Governance Foundation + Delegation Trace + Memory Ledger

验收日期：2026-08-31（Asia/Shanghai）<br>
平台：macOS 26.5.1（25F80），Apple Silicon arm64

### 范围与结论

本轮在单一 v0.6.0 中完成 Governance Foundation、Delegation Trace 与只读 Memory Ledger。两项治理投影都以 `canonical_events` 为唯一 event-level 事实源，并通过稳定身份、canonical evidence reference、证据等级、来源覆盖、算法版本和置信度返回派生结果。Session Replay 当前为“过程｜上下文｜委派｜记忆”；Delegation 与 Memory 都只在用户打开对应页签时加载。

自动化门禁、schema 30 → 31 migration regression、隔离 SQLite 完整性检查、desktop production web build、真实 Tauri 中英文/深浅主题矩阵，以及本地 Apple Silicon DMG 构建与检查均通过。本轮创建本地 commit 与 DMG，但没有创建 tag、push、GitHub Release 或对用户真实数据库运行重索引。Evolution Loop、记忆正文存储、记忆库编辑、自动学习、Agent 配置修改、OTLP、云同步、通用执行器和聊天界面均未实现。

### 版本合同

| 合同 | 当前值 |
| --- | --- |
| 产品版本 | `0.6.0` |
| SQLite schema | `31` |
| canonical event protocol | `1.0.0` |
| canonical event schema | `20` |
| Parser | `6.11.0` |
| Source Capabilities schema | `2` |
| Delegation projection | `delegation-trace-1.0.0` |
| Delegation anomaly rules | `delegation-anomaly-1.0.0` |
| Memory projection | `memory-ledger-1.0.0` |

### Delegation Provider 支持矩阵

能力只表示当前 adapter 能从已有结构化字段真实提取的信号，不根据 Provider 文档、标题或自然语言相似度推测。

| Provider | Delegation | Handoff | Subagent | 查询行为 |
| --- | --- | --- | --- | --- |
| Codex | `exact` | `partial` | `exact` | 完整规范证据可返回 `ready`；缺少 handoff 证据时如实降级 |
| Claude Code | `derived` | `unavailable` | `derived` | sidechain 信号只形成 `derived` / `partial`，不提升为 observed |
| DeepSeek Harness、Kimi Code、Grok Build、ZCode | `unavailable` | `unavailable` | `unavailable` | `not-recorded` |
| Cursor、OpenClaw、Hermes | `unavailable` | `unavailable` | `unavailable` | `not-recorded` |

### Memory Provider 支持矩阵

能力只表示当前 adapter 能从已有结构化字段真实观察到的信号。读取不会被推断为写入，也不会根据标题、自然语言、文件活动或路径相似度猜测记忆访问。

| Provider | Memory read | Memory write | 查询行为 |
| --- | --- | --- | --- |
| Codex | `partial` / `derived` | `unavailable` | 有可引用读取证据时返回 `partial`；没有证据时返回 `not-recorded` |
| Claude Code | `unavailable` | `unavailable` | `not-recorded` |
| DeepSeek Harness、Kimi Code、Grok Build、ZCode | `unavailable` | `unavailable` | `not-recorded` |
| Cursor、OpenClaw、Hermes | `unavailable` | `unavailable` | `not-recorded` |

Codex 首版信号来自已确认的本机 Hook memory 子活动。adapter 只输出脱敏后的 `memory.read`、安全会话身份和稳定 fingerprint；用于识别活动的目录、子会话原始身份和原始 envelope 不进入 canonical payload、共享 DTO 或 UI。当前没有任何 Provider 可以产生 `memory.write`。

### 确定性 fixture、migration 与生命周期

- schema 30 forward-only migration 新增 `execution_relations`、`execution_relation_evidence`、root/parent/child/active-status 索引、canonical evidence reverse index，并为 Attention 增加 `affected_branch_count`。
- Delegation fixtures 覆盖单子分支、三个并行子分支、正常返回、失败、waiting、handoff/resume/join、orphan、parent 已结束而 child 仍运行、重复 start/stop、乱序、source reorder、tombstone、隐私敏感 payload 和 unsupported Provider。
- 七条版本化 anomaly rule 覆盖 `orphan-child`、`spawn-failed`、`handoff-unfinished`、`blocked-branch`、`fanout-spike`、`unverified-completion` 与 `duplicate-delegation`；正常并行、partial silence 和 successful verification 反例不会被提升为高置信度异常。
- schema 29 → 30 回归校验 relation 表、约束、索引、integrity 与 foreign keys；reindex 失败回滚不会破坏上一代关系，用户确认与 Attention feedback 保留。
- schema 31 forward-only migration 新增 `memory_accesses`、`memory_access_evidence`，以及 session、source-session、operation、active 和 reverse-evidence 五个索引。
- schema 30 → 31 回归在临时数据库上保留既有 session，并验证 `PRAGMA quick_check`、`integrity_check`、foreign keys、2 张表与 5 个索引；旧版 v13–v30 的 staged-copy、failure recovery 与 startup retry 测试继续通过。
- 每条 access 至少引用一条未删除的 canonical event；重复事件、乱序输入、来源重排和重启收敛到稳定 ID 与确定时间顺序。
- 自动访问在重索引时可以 tombstone；`user-confirmed` 访问和既有 Attention feedback 不被删除。trigger 注入的失败事务回滚后，上一代两条可见记录保持不变。
- `memory.read` 只生成 read；ordinary tool/file event 不生成 access；`unavailable` 证据不生成记录；unsupported Provider 返回 truthful `not-recorded`。
- Memory Ledger 不生成 Attention / Notch 项，不评价召回内容、采用效果或记忆质量。

### 查询、并发与 UI 验收

`get_delegation_trace` 与 `get_memory_ledger` 都在 `spawn_blocking` command 中调用数据库薄接口，并遵守现有 SQLite connection mutex。Delegation 使用 session identity、root relation、批量 relation/evidence、批量 verification 与批量 session metadata 五类有限查询，identity 按 400 分块；Memory 使用一次 session/capability 查询和一次 access/evidence/canonical join。两者都不按节点或 access 逐条读取。查询计划 fixtures 确认 root/session 与 reverse evidence lookup 命中专用索引；两类投影各自完成 8 个线程 × 20 次并发读取，不产生重复投影。该测试验证锁与收敛合同，不代表真实设备 latency benchmark。

前端自动化覆盖：

- Memory 页签打开前不调用 API，query key 为 `['memory-ledger', sessionId]`；
- `ready`、`partial`、`not-recorded`、loading、error/retry 与空筛选结果；
- read/write 筛选、timeline/access 选择、evidence drawer、72% confidence、历史 Session 与 Process evidence 跳转；
- 键盘焦点、可访问标签、light/dark token、reduced-motion，以及 raw Prompt、命令、工具参数和绝对路径不渲染；
- Process 中的规范 `MemoryAccess / memory.read` 使用独立语言资源显示为中文“读取记忆 / 访问信号”和英文“Memory read / Access signal”，技术标识不直接混入用户界面；
- Delegation 跨 Session evidence 跳转会保留待定位证据，打开目标 Session 后切换至 Process 并定位对应 phase；Process 查询只补入匹配当前 agent 与 source-session identity 的 Delegation / Memory live canonical evidence，排除普通 live 事件并抑制同时间、同类型 history 重复；
- Sources 页面展示 Delegation 与 Memory reads：Claude Code 为 `Derived / Not recorded`，Codex 为 `Exact / Partial`，其余 Provider 为 `Not recorded`。

真实 Tauri 验收使用隔离数据库 `.scratch/memory-ledger-qa.sqlite` 和禁用后台索引的 QA 开关。数据库固定为 2 sessions、1 canonical event、1 memory access、1 evidence；中文浅色与英文深色分别检查 partial、evidence drawer、Process evidence 定位和 `not-recorded`，英文深色额外检查 Sources capability。退出后 `diagnostic_live_envelopes`、`live_session_metrics`、`attention_events`、`attention_feedback` 均为 0，schema 31 的 quick/integrity/foreign-key checks 通过。QA 截图保存在被忽略的 `.scratch/`，不进入版本库。

Delegation Trace 真实 Tauri 验收另使用隔离数据库 `.scratch/governance-qa.sqlite`，并禁用后台索引。英文深色 `ready` 状态固定返回 5 条关系、7 条 evidence 与 3 个并行子分支，覆盖 spawn、handoff、resume、join、waiting、completed、timeline、anomaly、graph/list selection 和 evidence drawer；抽查的 observed edge 为 98% confidence、2 条 canonical evidence reference。跨 Session “Show in Process” 可打开 waiting child，并显示本地化后的 2 条 evidence（“Subagent spawn started / Child branch waiting”），不渲染 Provider 私有事件名、`Delegation` 或 `delegation.*` 技术标识。Claude Code fixture 在无关系证据时返回 truthful `not-recorded`，未按标题或自然语言猜测父子关系。

中文浅色矩阵将单条 handoff relation 降级为 `inferred`、45% confidence 与 partial coverage，界面明确显示“部分记录”、4 条已观测和 1 条已推断；推断关系保持虚线视觉，不会显示为已观测。关系筛选选择“交接”后，graph 只保留该条 handoff edge。退出应用后 schema 31 的 `quick_check` 与 `integrity_check` 均为 `ok`，`foreign_key_check` 无记录；`diagnostic_live_envelopes`、`live_session_metrics` 和全部 Attention / feedback 表均为 0 行，并与修改前 ready 备份逐表一致。截图保存在被忽略的 `.scratch/delegation-qa-*.png`。

### 隐私检查

- 规范事件只保存 `memory.read`、`MemoryAccess`、稳定私有 identity、时间、证据等级和覆盖率；不保存 memory 正文或原始访问目标。
- fixture 主动注入 Prompt、命令、工具参数、token 和绝对路径，序列化 DTO 与真实 UI 均不包含这些值。
- Provider 私有事件名、memory 子会话原始 ID 与识别路径在 adapter 边界内丢弃；共享 UI 只接收规范 operation 和 canonical evidence ID。
- 项目继续使用现有 hash/sanitize 规则；Memory 不进入 Notch，Raw live envelope 继续遵守显式诊断、加密七天与提前清理合同。
- 历史目录和项目仓库保持只读；本轮只写 VibeMeter 自有隔离数据库。

### 实际执行命令

| 命令 | 最终结果 |
| --- | --- |
| `npm ci` | 通过；188 packages audited，0 vulnerabilities |
| `npm run check` | 通过；TypeScript build 与 i18n 10/10 |
| `npm test` | 通过；32 files、142 tests |
| `npm run rust:check`（稳定 Rust PATH） | fmt、Clippy 通过；library 271 passed、0 failed、3 ignored；另有 2 个 opt-in integration audit ignored |
| `npm run ci`（稳定 Rust PATH） | 通过；实际链为 `check → test → rust:check`，重复得到前述计数 |
| `npm run build --workspace @vibemeter/desktop` | 通过；2,319 modules transformed |
| `npm run tauri --workspace @vibemeter/desktop -- build --bundles dmg`（稳定 Rust PATH） | 通过；release app 与 `VibeMeter_0.6.0_aarch64.dmg` 构建成功，ad-hoc 签名；未配置 notarization 凭据，因此明确跳过公证 |
| `npm run dmg:finalize -- <dmg>` | 通过；背景与卷图标写入固定位置并隐藏 |
| `npm run dmg:check -- <dmg>` | 通过；`.background` 与 `.VolumeIcon.icns` 布局检查均为 PASS |
| `hdiutil verify`、挂载内容与 `codesign --verify --deep --strict` | 通过；版本、bundle、架构、Applications 链接与签名均符合合同 |

production web build 仍提示主 JS chunk 大于 500 kB：约 1,761.94 kB，gzip 563.77 kB，source map 8,270.67 kB。它是已知分包优化项，本记录不把该警告描述为性能通过。

本地安装包位于 `apps/desktop/src-tauri/target/release/bundle/dmg/VibeMeter_0.6.0_aarch64.dmg`，大小 21,257,758 bytes，SHA-256 为 `fa630eb451d2d61bcf846da6376b07b17c82372835ad48fb9de5d37d35617d4a`。挂载后 `CFBundleShortVersionString` 与 `CFBundleVersion` 均为 `0.6.0`，bundle ID 为 `com.vibemeter.desktop`，可执行文件为 Mach-O 64-bit arm64，`Applications` 指向 `/Applications`，严格签名检查通过且签名类型为 ad-hoc。本轮未执行 Apple notarization、DMG 安装启动冒烟或发布验收。

### 后续扩展边界

- Memory Ledger 已提供只读 `sessionId`、`workUnitId`、stable access ID、operation、coverage 和 canonical evidence reference。未来只有 Provider 提供独立结构化证据时，才能加入真实 memory identity、write 或 relation reference；不能回填正文或由 read 推断 write。
- Evolution Loop 只预留 versioned evaluation/outcome evidence 与 user-confirmed feedback 的引用点。下一轮若实现，必须保持本地优先、只观察，不能自动修改 Agent 配置、用户项目或记忆内容。
- 下一轮最小建议任务是 Evolution Loop 的证据合同与只读评估投影规格，不先实现自动执行或配置回写。

## Delegation Trace 阶段性源码验收（历史记录，已并入上方 v0.6.0）

验收日期：2026-08-30（Asia/Shanghai）<br>
平台：macOS 26.5.1（25F80），Apple Silicon arm64

### 范围与结论

本轮完成 Governance Foundation 与 Delegation Trace 的源码级 vertical slice。`canonical_events` 仍是唯一 event-level 事实源；执行关系、异常与 Attention 投影均可回到规范事件证据。Session Replay 增加按需加载的“委派”页签，并提供关系图、时间轨迹、筛选、证据详情、过程/历史会话跳转与大图列表回退。Memory Ledger、Evolution Loop、OTLP、云同步、通用执行器和聊天界面均未实现。

最终自动化门禁、migration regression 与 desktop production web build 通过。本轮没有创建 tag、push、发布 Release、构建 DMG，也没有在用户真实数据库上执行破坏性重索引；因此以下结论只覆盖源码、确定性夹具、临时 SQLite 数据库与 production web bundle，不把未执行的安装包或真实数据目视验收写成已通过。

### 版本合同

| 合同 | 当前值 |
| --- | --- |
| 产品版本 | `0.6.0` |
| SQLite schema | `30` |
| canonical event protocol | `1.0.0` |
| canonical event schema | `20` |
| Parser | `6.11.0` |
| Source Capabilities schema | `2` |
| Delegation projection | `delegation-trace-1.0.0` |
| Delegation anomaly rules | `delegation-anomaly-1.0.0` |

### Provider 支持矩阵

能力只表示当前 adapter 能从已有结构化字段真实提取的信号，不根据 Provider 文档、标题或自然语言相似度推测。

| Provider | Delegation | Handoff | Subagent | 查询行为 |
| --- | --- | --- | --- | --- |
| Codex | `exact` | `partial` | `exact` | 完整规范证据可返回 `ready`；缺少 handoff 证据时如实降级 |
| Claude Code | `derived` | `unavailable` | `derived` | sidechain 信号只形成 `derived` / `partial`，不提升为 observed |
| DeepSeek Harness | `unavailable` | `unavailable` | `unavailable` | `not-recorded` |
| Kimi Code | `unavailable` | `unavailable` | `unavailable` | `not-recorded` |
| Grok Build | `unavailable` | `unavailable` | `unavailable` | `not-recorded` |
| ZCode | `unavailable` | `unavailable` | `unavailable` | `not-recorded` |
| Cursor | `unavailable` | `unavailable` | `unavailable` | `not-recorded` |
| OpenClaw | `unavailable` | `unavailable` | `unavailable` | `not-recorded` |
| Hermes | `unavailable` | `unavailable` | `unavailable` | `not-recorded` |

`source-capabilities.json` 同时保留 history、live lifecycle、jump、memory read/write、skill use 与 evaluation 的逐信号合同；Rust 与 TypeScript schema tests 会拒绝未知枚举值，并校验每个 Provider 恰好出现一次。

### 确定性 fixture 与异常规则

| 场景 | 覆盖结果 |
| --- | --- |
| 单父分支 → 单子分支 | stable relation ID、根/子节点和 canonical evidence link |
| 单父分支 → 三个并行子分支 | 并行分支、waiting/error/completed 与受影响分支计数 |
| 子分支正常返回、失败、等待 | lifecycle/outcome 合并，状态与证据等级保持一致 |
| handoff 后 resume、join | 关系语义和时间顺序确定性覆盖 |
| child 没有 parent | orphan candidate 保持 `inferred` / `partial` |
| parent 结束但 child 仍运行 | 只报告证据支持的 branch 状态，不把沉默推成 stuck |
| 重复 start/stop、stop 早于 start、来源重排 | 去重、乱序收敛与稳定身份覆盖 |
| reindex tombstone | 自动关系可 tombstone；`user-confirmed` 与固定反馈保留 |
| 失败事务 | trigger 注入失败后上一代关系仍可见 |
| 隐私敏感 payload | Prompt、工具参数、命令、绝对路径与 Provider 私有事件名不进入 DTO/UI |
| unsupported Provider | 返回 `not-recorded`，不构造无证据关系 |

七条 anomaly rule 均为版本化、确定性规则：`orphan-child`、`spawn-failed`、`handoff-unfinished`、`blocked-branch`、`fanout-spike`、`unverified-completion`、`duplicate-delegation`。反例覆盖正常并行不触发 fanout、partial source 不产生高置信度告警、silence 不等于 stuck，以及 successful verification 抑制 unverified completion。

只有需要接管且具有 exact coverage 的 active waiting、blocking error、三次重复 spawn failure、达到阈值的 unfinished handoff 与 active orphan 会进入既有 Attention / Notch。既有 priority、feedback、jump、notification claim、foreground silence 与 duplicate suppression 仍由原链路处理。

### Migration、完整性与生命周期

- forward-only schema 30 新增 `execution_relations`、`execution_relation_evidence`、root/parent/child/active-status 索引与 canonical evidence reverse index，并为 Attention 增加 `affected_branch_count`。
- schema 29 → 30 临时数据库回归会校验 `PRAGMA quick_check`、`integrity_check`、foreign keys、必需列、约束与索引。
- v13 → 30、v14 → 30、v15 → 30、v17/v18 failure recovery、staged-copy rollback、persistent migration marker 与 startup retry 均由临时数据库测试覆盖。
- v13 fixture 曾残留当前 schema 30 的 relation tables，使三项升级恢复测试报 `table execution_relations already exists`；fixture 删除 v29/v30-only 表后，三个测试分别复跑通过，最终全量 Rust 结果为 0 failure。迁移本身没有改成宽泛 `IF NOT EXISTS`，因此半迁移仍会失败并触发 rollback。
- reindex 在一个事务内重建自动投影；失败回滚，不破坏上一代可见关系。自动关系可以 tombstone，用户确认、Attention feedback 与人工归属不被删除。
- 需要 `VIBEMETER_AUDIT_DATABASE` / `VIBEMETER_TEST_DB` 的真实数据库与分享/VCTI 审计保持 opt-in，本轮未执行，也未改写用户历史目录或项目仓库。

### 查询、并发与 UI 矩阵

Delegation 查询由一个 `spawn_blocking` command 执行，并遵守现有 SQLite connection mutex。一次常规 trace 读取使用 session identity、root relation、批量 relation/evidence、批量 verification 与批量 session metadata 共五类有限查询；identity 列表按 400 分块，不按节点逐条读取。`EXPLAIN QUERY PLAN` fixture 确认 root relation 与 reverse evidence lookup 命中专用索引。8 个线程 × 20 次读取的并发 fixture 完成 160 次 trace 读取，始终返回一条关系且没有重复投影；该测试验证锁与收敛合同，不作为真实设备 latency benchmark。

前端自动化覆盖：

- Delegation 页签选中前不调用 API，query key 为 `['delegation-trace', sessionId]`；
- loading → ready、ready、partial、not-recorded、error → retry；
- graph node/edge、timeline 与 evidence drawer 共用选择状态；
- Agent / relation / status filter 与无匹配状态；
- inferred 虚线与明确标签、关系/状态颜色、active theme tokens、reduced-motion CSS；
- zoom/pan/fit、30 节点以上自动展开的 button-based list fallback、可聚焦控件和 aria labels；
- evidence → Process 与 node → 历史 Session 跳转；
- Notch 的短 anomaly summary、影响分支数、固定反馈和 jump；
- 未建模 raw Prompt、tool input 与绝对路径不能渲染。

本轮没有启动真实 Tauri 窗口做 light/dark 目视矩阵；深浅主题结论来自共享 theme token contract、graph option tests、CSS 与 production build，不能替代下一轮安装包视觉 QA。

前端图模型保留文件名 `delegationGraphModel.ts`：目标 macOS 的大小写不敏感文件系统无法让 `delegationGraph.ts` 与 `DelegationGraph.tsx` 作为两个可靠的 TypeScript 模块共存。一次命名一致性试验在 `check` 阶段稳定触发 TS1149，恢复独立 `Model` 后缀后复跑通过；该平台例外已写入 ADR。

### 隐私检查

- 共享 DTO 只包含稳定哈希身份、safe label、结构化状态、证据等级、coverage、version、confidence 与 canonical evidence reference。
- Provider 私有 child/parent 字段只在 adapter 内读取；`rootSessionId` 和 evidence session jump 只返回 VibeMeter 内部 session ID。
- 项目字段沿用 hash/sanitize；路径形态只生成 private hash label，Notch 不使用 conversation title 展示异常。
- Prompt、Response、完整 diff、工具参数/输出、命令、环境变量、密钥和绝对路径不进入 Delegation relation、Attention 摘要或 UI。
- Raw live envelope 的显式诊断、加密七天保留与提前清理合同未改变。

### 实际执行命令

| 命令 | 最终结果 |
| --- | --- |
| `npm ci` | 通过；188 packages audited，0 vulnerabilities |
| `npm run check` | 通过；TypeScript build 与 i18n 10/10 |
| `npm test` | 通过；31 files、132 tests |
| `cargo check --all-targets` | 通过 |
| 三个 migration failure 定向复跑 | 3/3 通过 |
| `npm run rust:check`（稳定 Rust PATH） | fmt、Clippy 通过；library 258 passed、0 failed、3 ignored；另有 2 个 opt-in integration audit ignored |
| `npm run ci`（首次） | TypeScript/i18n/frontend 通过；随后因非交互 shell 找不到 `cargo` 非零退出 |
| `npm run ci`（文件命名试验） | `check` 以 TS1149 非零退出；恢复 `delegationGraphModel.ts` 并记录 ADR |
| `npm run ci`（稳定 Rust PATH 复跑） | 通过；实际链为 `check` → `test` → `rust:check` |
| `npm run build --workspace @vibemeter/desktop` | 通过；2,315 modules transformed |

production web build 仍提示主 JS chunk 大于 500 kB（约 1,747.95 kB，gzip 561.33 kB）。这是已知分包优化项，不影响本轮构建成功；本轮未把它描述为性能通过。

### 后续扩展边界

- Memory Ledger 只预留 canonical evidence reference、stable execution identity、work unit 与 source capability 扩展点；本轮无 memory 表、写 API 或 UI。
- Evolution Loop 只预留 versioned outcome/evaluation signal 与 user-confirmed feedback 扩展点；本轮无自动评估、配置修改或闭环执行。
- 下一轮进入安装包或真实数据验收前，应先对 disposable v0.5 database copy 执行 opt-in migration audit，再做 light/dark、键盘和大图的真实 Tauri 目视/交互矩阵。

---

## v0.1.0 本机交付验收（历史记录）

验收日期：2026-07-27（Asia/Shanghai）<br>
平台：macOS 26.5.1（25F80），Apple Silicon arm64

## 交付结论

VibeMeter 已从 aftervibe 的本机数据、回放、分享与 VCTI 能力上完成独立品牌与应用身份迁移，并加入 Claude Code / Codex 的低侵入式实时 Hook、MacBook Notch 状态面板、跳回来源、90 天原始事件保留、长期派生指标，以及数据页和分享页同步的“我的口头禅”/“Agent 的口头禅”。复盘工作区及非公开旧卡片渲染器已从 VibeMeter 移除，旧实现保留在 TokenGraph。

最终 `release/VibeMeter.app` 已完成 production 编译、显式 ad-hoc 签名、严格签名校验，并从交付目录直接启动。

| 项目 | 结果 |
| --- | --- |
| 显示名 | `VibeMeter` |
| Bundle ID | `com.vibemeter.desktop` |
| 数据库 | `vibemeter.sqlite` |
| 架构 | Mach-O 64-bit arm64 |
| 应用包大小 | 34 MB |
| 最低系统 | macOS 14 |
| 签名 | ad-hoc；`codesign --verify --deep --strict` 通过 |
| 可执行文件 SHA-256 | `62fef231b9b4934d3ddb30dddd9a23c5af54b031afc3c3a0455e54d4f2545ce0` |
| Apple notarization | 未执行，符合本机交付边界 |

## 自动化验证

最终源码分别执行前端测试、TypeScript / i18n 检查、production 构建，以及 Rust fmt、Clippy 和测试：

| 验证项 | 结果 |
| --- | --- |
| TypeScript 编译 | 通过 |
| 中英文本地化资源一致性 | 6/6 通过 |
| 前端组件与数据行为 | 36/36 通过 |
| Rust 格式化 | `cargo fmt --check` 通过 |
| Rust Clippy | `--all-targets -- -D warnings` 通过 |
| Rust 单元与并发测试 | 78/78 通过；1 项示例卡按设计默认忽略 |
| Share Guard | 密钥、绝对路径、邮件与仓库 URL 边界通过 |
| Hook 合并 / 修复 / 卸载边界 | 单元测试通过 |
| 迁移优先级与源库只读复制 | 单元测试通过 |
| 口头禅过滤、压缩、跨会话与 Agent 归因 | 单元测试通过 |
| Notch 状态优先级与前台来源判断 | 单元测试通过 |

使用 VibeMeter 数据库的本机快照额外执行真实分享矩阵：

- 当前矩阵合同为 6 个公开模板 × 2 种语言 × 8 种画幅 × PNG/SVG，共 192 个文件；
- 使用本机真实数据库完整重跑，192/192 个文件生成并通过验证；
- 每个请求重复预览，SVG 和模型哈希保持确定一致；
- PNG 签名正确，SVG 可由 resvg 重新解析，不含 `undefined` 或 `NaN`；
- 口头禅卡完成居中、字号与留白调整后，再次完整重跑 192/192 个真实数据文件，并目视检查横版、方形和手机竖版；
- VCTI 行为指纹标签修正后，单独重跑中英文 × 8 种画幅 × PNG/SVG 共 32/32 个真实数据文件；中文与英文竖版均完成目视检查；
- 当前产品界面和后端只保留同一组 6 个公开模板。

## 真实数据迁移

最终应用从独立 VibeMeter 数据目录启动，当前数据库检查如下：

| 数据 | 数量 / 状态 |
| --- | ---: |
| `PRAGMA integrity_check` | `ok` |
| schema | 9 |
| 会话 | 715 |
| Parser 6.2.1 | 706 |
| 既有 Hermes Parser 5.0.0 | 9 |
| 派生口头禅记录 | 223,843 |
| 含口头禅派生值的会话 | 693 |
| 仅含标点的派生词 | 0 |
| 当前数据库文件 | 约 164 MB |
| 验收合成 Live 事件 | 0（已清理） |

Parser 6.2.1 会完整扫描本机可读取的用户/Agent 文本，过滤代码块、路径、密钥形态、工具输出、标记与仅标点内容，只把按角色、日期排名后的派生词频写入数据库。当前派生词频为 223,843 行；不保存用于分析的历史原文。9 条 Hermes 记录保留其既有解析器版本，不把当前不可重建的来源伪装成已重索引。

首次迁移使用 SQLite online backup，优先 aftervibe、其次 TokenGraph；迁移单元测试同时核验源数据库不被改写。独立 `/Users/rangeking/Code/aftervibe` 代码检出未参与构建或修改。

## Hook 与 Notch 实机验收

最终 release 在当前 Mac 上完成真实 Hook 安装与事件注入：

| 项目 | 结果 |
| --- | --- |
| Hook 脚本 | `~/.vibemeter/hooks/vibemeter_hook.py`，权限 `0700` |
| Unix socket | `~/.vibemeter/vibemeter.sock`，权限 `0600` |
| Claude Code managed Hook | 9 个事件条目 |
| Codex managed Hook | 3 个事件条目 |
| Codex feature | `[features] codex_hooks = true` |
| 首次配置备份 | 2 个；Claude/Codex 各 1 个 |
| 重复启动后的新增备份 | 0 |
| 折叠 Notch | 365 × 32；物理缺口区域保持透明 |
| 展开 Notch | 单实例 440 × 168；高度随实例数动态计算 |

真实安装前的两个配置备份 SHA-256 分别为：

- Claude Code：`7018acb5aa5ad9d5524a4fea60826a61ba92517ae869104c37f4b1565bd75d85`
- Codex：`fa6adc42bea83cb281fe1b3d9b720181d1efe9f3039485dd89395dc73aec891d`

安装后多次启动，备份数量保持 2，说明未反复覆盖用户配置。结构化合并、只移除自身 managed command、保留共享 Codex feature flag 的行为另有 Rust 回归测试覆盖。

通过最终安装的 Python Hook 向真实本机 socket 发送 Codex `SessionStart` 与 `PermissionRequest` 后，数据库收到 2 个事件和 1 个等待计数；主界面、折叠 Notch、展开 Notch 与系统通知均更新。展开态显示本地化后的“Bash 需要你批准”，未显示原始 Prompt 或命令正文。验收事件随后从 `live_events` 与 `live_session_metrics` 精确删除。

实时页现在直接提供 Notch 开关，并复用与设置页相同的 `notchEnabled` 原生开关链路。多 Agent 折叠态使用 88 pt 左翼与 98 pt 右翼，在保留物理缺口居中的前提下居中排列 Provider 集群；Codex 与 Claude Code 图标分别做了光学尺寸校正。最终向真实 socket 短暂注入一个 Claude Code 读取事件，与正在运行的 Codex 组成双 Agent 状态，截图确认两个图标大小一致、集群居中；该会话的事件和派生计数随后精确删除并重启应用，验收记录为 0。

误显示 Claude Code 的根因已确认：Cursor 会把包含 `cursor_version`、`composer_mode`、`conversation_id` 等字段的事件送入 Claude 配置目录中的共享 Hook，旧逻辑只信任命令参数中的 Provider。当前实现会先校验 Provider 对应的事件名与负载来源；Cursor 负载不会再创建 Claude Code 会话。启动时还会删除历史上带 `cursor_version`、却被标成 `claude-code` 的 Live 事件及其派生计数。真实数据库复验该类记录为 0，Rust 同时覆盖“拒绝误报、保留真实 Claude”与“清理历史误记”两条回归测试。

Codex Desktop 调用内部 memory 时会启动独立子会话 ID，工作目录位于 `~/.codex/memories`。VibeMeter 现在按同一进程和最近活动父会话建立稳定别名，把它显示为父 Codex 实例中的 `reading / Memory` 活动；子会话结束不会把父实例标成完成，无法确定父实例的孤立后台 memory 任务也不会生成 Notch 实例。启动时会清理旧版本误记的 memory Live 事件与指标，真实数据库复验两者均为 0；三条回归测试覆盖折叠、结束状态和孤立子任务。最终安装的 Hook 另发送一组父会话与 memory 子会话事件，数据库只生成父 `source_session_id` 下的 2 条事件、子 ID 为 0；验收记录随后精确清理并重启应用。

前台来源判断使用 `NSWorkspace.frontmostApplication`，不再通过 AppleScript 查询前台进程。最终等待态复验未出现 macOS 自动化权限弹窗；通知只在来源位于后台且状态转入 `waiting` / `error` 时触发。

## 数据页与分享页实机验收

- 主导航为 Data、Live、VCTI；Insights、Sessions、Share、Sources、Settings 保留为二级入口。
- 数据页月视图真实显示 298 个会话，并保留本机 Token、时长、成本、Agent、模型、工具与工作事件总账。
- 数据源页默认选中磁盘中发现的 5 个 Agent；实机取消 Hermes 后，数据页来源条、图例、汇总与工作事件同步排除，再次选中后持久化恢复。冷启动索引完成会立即刷新来源与汇总，不再等待 30 秒轮询。
- Cursor 账户 Token 与成本未开启提示仅保留在 Cursor 数据源卡片，数据页与洞察页不再重复显示。
- “我的口头禅”与“Agent 的口头禅”使用最终精确标题；当前范围分别显示 205 与 232 个样本会话。
- Agent 词块使用主要来源 Agent 的底色；悬浮详情显示各 Agent 出现次数，卡片带颜色图例。
- 6.2.1 重索引后的词云未再出现连字符、点号等仅标点候选。
- “口癖抓包”把标题、副标题、冠军口癖、模型、重复次数、跨会话数、点评与方法说明重新按画幅居中排布；最终真实数据 192 文件矩阵全部通过，横版、方形、竖版完成目视检查，左下角使用真实 VibeMeter 图标。
- VCTI 分享卡的 18 根行为指纹不再使用 `01–18` 数字编号，改为“目标清晰”“探索倾向”“Agent 放权”“自动验证”等真实维度名称；标签从卡片底部向内排布，并为中英文分别预留标签带，中英文 8 种画幅均通过导出验证。
- 选择“会话用量回顾”后，D4 模板卡下方会立即展开会话下拉菜单；用户选择的会话 ID 同步驱动实时预览、复制图片和 PNG/SVG 导出，时间范围变化时会自动清除不在当前范围内的旧选择。
- 会话页打开时会立即触发增量索引；索引完成后会话页与分享页会话选择器同步刷新。跨天持续运行的会话按最后活动时间进入所选范围并排序，因此本轮从 7 月 25 日开始、持续到 7 月 27 日的 Codex 对话会出现在列表顶部，而不会按首次开始时间被埋入旧记录。
- 1:1 与 16:9 真实分享卡均目视检查：双词云、样本数、Agent 图例、说明和品牌区完整，无裁切或溢出。
- Notch 与主界面只呈现 Agent、项目、阶段、时间、结构化动作和等待原因，不呈现原始 Prompt。

## 已知边界

- 当前交付仅面向 Apple Silicon，不是 Universal Binary。
- 当前是 ad-hoc 签名，不含 Developer ID 与 Apple notarization。
- production JS 主包约 1.614 MB，Vite 给出代码分块优化提示；不影响本轮功能、测试或 bundle 成功。
- 实时精确 Hook 首版只支持 Claude Code 与 Codex；其他 Agent 继续按本机可读取能力进入历史数据与 VCTI。
- Git 证据默认关闭。
