# VibeMeter Governance Architecture

VibeMeter 是本地优先、只观察的 Personal Agent Governance & Evolution Layer。它记录、解释、提醒并跳回来源 Agent，但不执行通用 Agent 工作、不代替用户审批、不写入用户项目或 Agent 配置，也不依赖外部 AI API。

## 证据与派生模型

`canonical_events` 是唯一的 event-level 事实源。Provider adapter 只能从本机已有结构化记录或已安装 Hook 中提取最小源信号；规范层在写入前裁剪正文、命令、工具参数、环境变量、密钥和绝对路径。Delegation Trace、Memory Ledger、Attention、过程回放与后续 Evolution Loop 都是引用规范证据的派生模型，不能反向改写事实。

证据等级严格区分：

- `observed`：来源明确记录了身份或生命周期事实；
- `derived`：由一条或多条 observed 证据按版本化规则合并；
- `inferred`：来源不完整，只能形成低置信度候选；
- `user-confirmed`：用户明确确认或修正；
- `unavailable`：没有可引用的规范证据。

查询状态严格区分：

- `ready`：根会话存在且关系证据覆盖足以形成可用轨迹；
- `partial`：存在可引用证据，但父身份、生命周期或某类关系不完整；
- `not-recorded`：该来源没有可用于 Delegation Trace 的规范证据。

Memory Ledger 使用同一组查询状态：只有来源对已返回的读写操作具备 exact coverage 且证据为 observed 或 user-confirmed 时才是 `ready`；存在可引用但不完整的活动证据时为 `partial`；没有可引用证据时为 `not-recorded`。

Source 页面使用 per-signal capability：`exact`、`derived`、`partial`、`unavailable`。能力描述只反映当前 adapter 真正可观察的结构化字段，不根据 Provider 宣传或自然语言内容推测。

## 关系语义

- `delegate`：父分支把工作交给另一执行分支；不等于创建新身份。
- `spawn`：父分支创建新的子执行身份。
- `handoff`：执行责任从一个分支转移到另一个既有或新分支。
- `resume`：既有执行分支在暂停或交接后恢复。
- `join`：子分支的结果被父分支回收或确认接收。

首版状态只允许 `started`、`running`、`waiting`、`failed`、`completed`、`unknown`。完成表示来源生命周期结束或结果被回收，不证明工作单元成功。

## 稳定身份、去重与重索引

执行关系 ID 使用算法版本、Agent、根/父/子来源身份、工作单元身份和关系类型的稳定哈希。规范事件的 stable source fingerprint 负责记录级去重；投影层再按关系身份合并重复 start/stop，并按时间和确定性优先级处理乱序。应用重启与 source record reorder 不改变 relation ID。

重索引在同一 SQLite 事务中写入新一代规范证据与执行关系：新关系 upsert，上一代未重现的自动关系设置 `deleted_at` tombstone。失败事务回滚并保留上一代可见结果。`user-confirmed` 关系、Attention feedback、人工工作单元归属和长期快照不由自动重索引删除。

## Provider 私有字段隔离

Claude Code 的 `agentId` / sidechain 标记与 Codex 的 child/parent thread 字段只在各自 adapter 内读取。进入共享层后只保留经过 `safe_opaque_identifier` 处理的父子身份、共享 relation type、共享 lifecycle signal、可信时间、覆盖率和 canonical evidence ID。共享 DTO 与 UI 不暴露 Provider 私有事件名、原始 Prompt/Response、工具参数、命令、输出或绝对路径。

## Delegation Trace 模块

```text
Provider records / exact Hook
  -> adapter source signal
  -> canonical_events
  -> governance::delegation projection
  -> execution_relations + evidence links
  -> get_delegation_trace
  -> Session Replay graph / timeline / evidence drawer
  -> takeover-only Attention / compact Notch summary
```

`governance::delegation` 是深模块：调用者只提供规范信号并接收确定性的关系、覆盖和异常。`delegation_store` 是 SQLite adapter，负责有限批量查询、事务 upsert/tombstone 与 evidence integrity。Tauri command 在 `spawn_blocking` 中调用数据库薄接口，避免在异步等待期间持有 SQLite mutex。

异常规则固定版本并始终引用 evidence：`orphan-child`、`spawn-failed`、`handoff-unfinished`、`blocked-branch`、`fanout-spike`、`unverified-completion`、`duplicate-delegation`。partial 来源不能生成高置信度 takeover 提醒；沉默不等于 stuck；正常并行不等于 fanout spike；成功 verification evidence 会抑制 unverified completion。

## Memory Ledger 模块

```text
Provider structured activity
  -> adapter memory signal
  -> canonical_events
  -> governance::memory projection
  -> memory_accesses + evidence links
  -> get_memory_ledger
  -> Session Replay timeline / evidence drawer
```

`governance::memory` 只接受 `memory.read` 与 `memory.write` 规范信号，并生成 `memory-ledger-1.0.0` 稳定访问身份。`memory_ledger_store` 在同一事务内 upsert 新投影、tombstone 未重现的自动记录并保留 `user-confirmed` 记录；查询经一个 `spawn_blocking` command 使用有限批量 SQL。每条访问至少引用一个未删除的 canonical event，并返回 operation、evidence level、source coverage、algorithm version 与 confidence。

Codex 首版只把已确认的本机 Hook 记忆活动映射为 `partial-memory-read` / `derived`；它不证明读取了哪条记忆，也不能推断写入。Codex write、Claude Code read/write 和其他 Provider 均为 `unavailable`。识别活动所需的来源路径在 adapter 内立即丢弃，不进入 canonical payload、共享 DTO 或 UI。Memory Ledger 不进入 Attention 或 Notch，也不评价记忆质量和采用效果。

## 与 trace/span 的映射

根会话可映射为 trace，执行分支可映射为 span，`delegate` / `spawn` / `handoff` / `resume` / `join` 可映射为有方向的 span link 或 lifecycle event。当前稳定 identity、时间、状态、attribute coverage 和 evidence reference 为未来 OTLP export 保留映射点；本轮不实现 OTLP、collector 或网络导出。

## 后续扩展点

Memory Ledger 已通过 `sessionId`、`workUnitId`、stable access ID 与 canonical evidence ID 提供只读访问记录；未来可增加来源真实提供的 memory identity 或 relation reference，但不能回填正文或依据读取推断写入。Evolution Loop 后续可引用同一稳定身份、访问记录和结果证据，比较用户确认的改变前后表现；不会把自动配置修改、用户文件写入或外部评估执行加入 governance 读模型。
