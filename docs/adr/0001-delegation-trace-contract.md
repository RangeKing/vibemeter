# Delegation Trace 使用规范证据投影，而不是第二套事实账本

VibeMeter 将 `canonical_events` 保持为唯一的事件级事实源，并把委派、创建、交接、恢复、汇合、生命周期结果与异常统一投影为可重建的执行关系。Provider adapter 只提取经过隐私裁剪的源信号，共享 governance 模块负责身份稳定、去重、证据等级、覆盖率、异常规则和查询合同；这比在每个 Provider 或页面内解释私有事件更容易保持一致，也允许重索引时安全 tombstone 自动派生结果而不改写用户确认。

## 后果

- 每条执行关系和异常必须至少引用一条未被删除的规范事件，并返回 evidence level、source coverage、algorithm/rule version 与 confidence。
- 明确父子身份可以标记为 observed；规则合并后的生命周期标记为 derived；缺少可靠父身份或只靠弱信号的候选必须标记为 inferred。任何层都不得把 inferred 呈现为 observed。
- `delegate` 表示把工作交给另一执行分支，`spawn` 表示创建子执行身份，`handoff` 表示责任转移，`resume` 表示既有分支恢复，`join` 表示结果回收到父分支。
- 执行关系使用来源身份的稳定哈希，不使用标题、提示词、回复或自然语言相似度。重复、乱序、重启和来源记录重排不能产生新关系 ID。
- 自动投影可在重索引时重建或 tombstone；用户确认、固定反馈和人工归属拥有独立生命周期。
- Provider 私有字段只存在于 adapter 内，进入规范账本前必须转换为共享信号并完成路径、正文与参数裁剪。
- 前端图模型使用 `delegationGraphModel.ts`。在目标 macOS 的大小写不敏感文件系统上，约定中的 `delegationGraph.ts` 会与组件 `DelegationGraph.tsx` 形成同基名解析冲突并触发 TypeScript TS1149；独立的 `Model` 后缀保持模块边界而不依赖大小写差异。
- Memory Ledger 与 Evolution Loop 后续只通过规范 evidence reference 和稳定执行身份扩展；本轮不创建记忆或评估写模型。
