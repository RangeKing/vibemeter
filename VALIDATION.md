# VibeMeter 0.1.0 本机验收记录

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
