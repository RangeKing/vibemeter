# VibeMeter 0.1.0 本机验收记录

验收日期：2026-07-25（Asia/Shanghai）<br>
平台：macOS 26.5.1（25F80），Apple Silicon arm64

## 交付结论

VibeMeter 已从 aftervibe 的本机数据、回放、复盘、分享与 VCTI 能力上完成独立品牌与应用身份迁移，并加入 Claude Code / Codex 的低侵入式实时 Hook、MacBook Notch 状态面板、跳回来源、90 天原始事件保留、长期派生指标，以及数据页和分享页同步的“我的口头禅”/“Agent 的口头禅”。

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
| 可执行文件 SHA-256 | `461c1d7a515ee25624846aedd6de395fa79e4742e5e1c33bec359e622a391e12` |
| Apple notarization | 未执行，符合本机交付边界 |

## 自动化验证

最终源码执行 `npm run ci`：

| 验证项 | 结果 |
| --- | --- |
| TypeScript 编译 | 通过 |
| 中英文本地化资源一致性 | 5/5 通过 |
| 前端组件与数据行为 | 26/26 通过 |
| Rust 格式化 | `cargo fmt --check` 通过 |
| Rust Clippy | `--all-targets -- -D warnings` 通过 |
| Rust 单元与并发测试 | 59/59 通过；1 项真实数据库矩阵按设计默认忽略 |
| Share Guard | 密钥、绝对路径、邮件与仓库 URL 边界通过 |
| Hook 合并 / 修复 / 卸载边界 | 单元测试通过 |
| 迁移优先级与源库只读复制 | 单元测试通过 |
| 口头禅过滤、压缩、跨会话与 Agent 归因 | 单元测试通过 |
| Notch 状态优先级与前台来源判断 | 单元测试通过 |

使用 VibeMeter 数据库的本机快照额外执行真实分享矩阵：

- 10 个后端渲染模板 × 2 种语言 × 8 种画幅 × PNG/SVG，共 320/320 个文件通过；
- 每个请求重复预览，SVG 和模型哈希保持确定一致；
- PNG 签名正确，SVG 可由 resvg 重新解析，不含 `undefined` 或 `NaN`；
- 方形口头禅卡完成字号密度调整后，单独重跑其 32/32 个语言、画幅和格式组合并目视检查；
- 当前产品界面公开 6 个模板，4 个历史复盘渲染器仅保留后端兼容与测试覆盖。

## 真实数据迁移

最终应用从独立 VibeMeter 数据目录启动，当前数据库检查如下：

| 数据 | 数量 / 状态 |
| --- | ---: |
| `PRAGMA integrity_check` | `ok` |
| schema | 8 |
| 会话 | 711 |
| Parser 6.1.0 | 702 |
| 既有 Hermes Parser 5.0.0 | 9 |
| 派生口头禅记录 | 224,469 |
| 含口头禅派生值的会话 | 691 |
| 仅含标点的派生词 | 0 |
| 重索引后压缩数据库 | 约 104.5 MB |
| 验收合成 Live 事件 | 0（已清理） |

Parser 6.1.0 会完整扫描本机可读取的用户/Agent 文本，过滤代码块、路径、密钥形态、工具输出、标记与仅标点内容，只把按角色、日期排名后的派生词频写入数据库。重索引后派生词频从约 125 万行降至 224,469 行；不保存用于分析的历史原文。9 条 Hermes 记录保留其既有解析器版本，不把当前不可重建的来源伪装成已重索引。

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
| 折叠 Notch | 354 × 42 |
| 展开 Notch | 430 × 304 |

真实安装前的两个配置备份 SHA-256 分别为：

- Claude Code：`7018acb5aa5ad9d5524a4fea60826a61ba92517ae869104c37f4b1565bd75d85`
- Codex：`fa6adc42bea83cb281fe1b3d9b720181d1efe9f3039485dd89395dc73aec891d`

安装后多次启动，备份数量保持 2，说明未反复覆盖用户配置。结构化合并、只移除自身 managed command、保留共享 Codex feature flag 的行为另有 Rust 回归测试覆盖。

通过最终安装的 Python Hook 向真实本机 socket 发送 Codex `SessionStart` 与 `PermissionRequest` 后，数据库收到 2 个事件和 1 个等待计数；主界面、折叠 Notch、展开 Notch 与系统通知均更新。展开态显示本地化后的“Bash 需要你批准”，未显示原始 Prompt 或命令正文。验收事件随后从 `live_events` 与 `live_session_metrics` 精确删除。

前台来源判断使用 `NSWorkspace.frontmostApplication`，不再通过 AppleScript 查询前台进程。最终等待态复验未出现 macOS 自动化权限弹窗；通知只在来源位于后台且状态转入 `waiting` / `error` 时触发。

## 数据页与分享页实机验收

- 主导航为 Live、Data、VCTI；Sessions、Aftervibe、Insights、Share、Sources、Settings 保留为二级入口。
- 数据页月视图真实显示 294 个会话，并保留本机 Token、时长、成本、Agent、模型、工具与工作事件总账。
- “我的口头禅”与“Agent 的口头禅”使用最终精确标题；当前范围分别显示 205 与 232 个样本会话。
- Agent 词块使用主要来源 Agent 的底色；悬浮详情显示各 Agent 出现次数，卡片带颜色图例。
- 6.1.0 重索引后的词云未再出现连字符、点号等仅标点候选。
- 1:1 与 16:9 真实分享卡均目视检查：双词云、样本数、Agent 图例、说明和品牌区完整，无裁切或溢出。
- Notch 与主界面只呈现 Agent、项目、阶段、时间、结构化动作和等待原因，不呈现原始 Prompt。

## 已知边界

- 当前交付仅面向 Apple Silicon，不是 Universal Binary。
- 当前是 ad-hoc 签名，不含 Developer ID 与 Apple notarization。
- production JS 主包约 1.614 MB，Vite 给出代码分块优化提示；不影响本轮功能、测试或 bundle 成功。
- 实时精确 Hook 首版只支持 Claude Code 与 Codex；其他 Agent 继续按本机可读取能力进入历史数据与 VCTI。
- Git 证据默认关闭；深度复盘的真实模型调用依赖用户本机 CLI 登录或 API 环境变量。
