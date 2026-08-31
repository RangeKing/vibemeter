# Memory Ledger 只投影可证明的记忆访问，不保存记忆内容

VibeMeter 将 Memory Ledger 作为 `canonical_events` 上可重建的治理投影，而不是第二套记忆库。Provider adapter 只能把来源明确暴露的结构化信号转换为 `memory.read` 或 `memory.write`；共享 governance 模块负责稳定身份、证据链接、覆盖率、tombstone 与查询合同。这样保留可解释的访问历史，同时避免为完整 Prompt、Response、命令、路径或记忆正文建立新的敏感存储面。

## 后果

- 每条记忆访问必须至少引用一条未删除的规范事件，并返回 operation、evidence level、source coverage、algorithm version 与 confidence。
- Codex 首版只有由本机 Hook 工作目录信号派生的 partial read；它不能产生 observed/exact，也不能证明访问了哪条记忆。Codex write、Claude Code read/write 与其他 Provider 均保持 unavailable，直到 adapter 能观察到独立结构化信号。
- adapter 在识别信号后立即丢弃用于判断的路径；共享 DTO 和 UI 只看到 `MemoryAccess`、安全会话引用和规范 evidence ID。
- 不根据标题、自然语言、项目路径相似度、文件读写或沉默猜测记忆访问；读取也不能被推断为写入。
- 自动投影可以在重索引时重建或 tombstone；用户确认记录拥有独立生命周期。失败事务必须保留上一代可见投影。
- Memory Ledger 不进入 Attention 或 Notch，也不评价记忆质量。后续 Evolution Loop 只能通过版本化 evaluation/outcome evidence 引用访问记录，不能反向修改 Agent 配置或用户文件。
