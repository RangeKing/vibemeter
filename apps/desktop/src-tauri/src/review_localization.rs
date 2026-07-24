use crate::models::{ReviewContent, ReviewFinding};
use crate::review_engine::ReviewEvidence;

pub fn review_title(locale: &str, review_type: &str, evidence: &ReviewEvidence) -> String {
    let kind = match (locale, review_type) {
        ("zh-CN", "daily") => "每日复盘",
        ("zh-CN", "weekly") => "每周复盘",
        ("zh-CN", "task") => "工作复盘",
        ("zh-CN", _) => "会话复盘",
        (_, "daily") => "Daily review",
        (_, "weekly") => "Weekly review",
        (_, "task") => "Work review",
        _ => "Session review",
    };
    let subject = match (evidence.project_label.trim(), evidence.title.trim()) {
        ("", "") => return kind.into(),
        (project, "") => project.to_string(),
        ("", title) => title.to_string(),
        (project, title) => format!("{project} · {title}"),
    };
    if locale == "zh-CN" {
        format!("{kind}：{subject}")
    } else {
        format!("{kind}: {subject}")
    }
}

pub fn finding(locale: &str, rule_id: &str, evidence: &ReviewEvidence) -> (String, String) {
    let edits = evidence.lines_added.saturating_add(evidence.lines_deleted);
    let path = evidence.max_file_path.as_deref().unwrap_or("a file");
    if locale == "zh-CN" {
        match rule_id {
            "verified-result" => (
                "结果有验证证据".into(),
                if evidence.has_commit {
                    "时间窗口内检测到 Git 提交，交付结果可以追溯。".into()
                } else {
                    format!(
                        "检测到 {} 次测试、构建或检查事件。",
                        evidence.verification_events
                    )
                },
            ),
            "high-token-low-output" => (
                "投入与可观察产出不成比例".into(),
                format!(
                    "消耗 {} Token，但只观察到 {} 个文件、{} 行修改。",
                    evidence.total_tokens, evidence.files_changed, edits
                ),
            ),
            "high-token-low-output-action" => (
                "先缩小下一次任务的验证范围".into(),
                "开始编辑前先写明唯一交付物和一个可执行验收命令；若第一次验证失败，再扩大上下文。"
                    .into(),
            ),
            "repeated-file-edits" => (
                "同一文件发生高频返工".into(),
                format!(
                    "`{path}` 在本次工作中被修改 {} 次。",
                    evidence.max_modification_count
                ),
            ),
            "repeated-file-edits-action" => (
                "在再次修改前建立更早的检查点".into(),
                format!("下次处理 `{path}` 时，先运行最小检查并确认接口约束，再开始连续修改。"),
            ),
            "repeated-errors" => (
                "错误或重试重复出现".into(),
                format!(
                    "观察到 {} 次错误和 {} 次重试。",
                    evidence.errors, evidence.retries
                ),
            ),
            "repeated-errors-action" => (
                "第二次同类失败后切换为诊断流程".into(),
                "记录失败条件、最小复现和一个待证伪假设，避免原样重复执行。".into(),
            ),
            "long-without-verification" => (
                "修改持续较久但没有验证事件".into(),
                format!(
                    "活跃修改约 {} 分钟，未检测到测试、构建、类型检查或提交证据。",
                    evidence.active_seconds / 60
                ),
            ),
            "long-without-verification-action" => (
                "把验证前移到第一次成形修改之后".into(),
                "下一次在第一个可运行切片完成时立即执行最小验证，不等到任务末尾。".into(),
            ),
            "model-switches" => (
                "任务中多次切换模型".into(),
                format!(
                    "观察到 {} 次模型切换；需要确认切换是否解决了具体阻塞。",
                    evidence.model_switches
                ),
            ),
            _ => ("证据不足".into(), "当前记录不足以形成可靠结论。".into()),
        }
    } else {
        match rule_id {
            "verified-result" => (
                "The result has verification evidence".into(),
                if evidence.has_commit {
                    "A Git commit was observed in the task window, making the delivery traceable.".into()
                } else {
                    format!("{} test, build, or check events were observed.", evidence.verification_events)
                },
            ),
            "high-token-low-output" => (
                "Observed output was small relative to the input".into(),
                format!(
                    "The task used {} tokens while showing {} changed files and {} changed lines.",
                    evidence.total_tokens, evidence.files_changed, edits
                ),
            ),
            "high-token-low-output-action" => (
                "Narrow the verification target before the next run".into(),
                "Name one deliverable and one executable acceptance command before editing; expand context only after the first failed check.".into(),
            ),
            "repeated-file-edits" => (
                "One file accumulated repeated rework".into(),
                format!("`{path}` was modified {} times during this work.", evidence.max_modification_count),
            ),
            "repeated-file-edits-action" => (
                "Create an earlier checkpoint before editing again".into(),
                format!("For the next change to `{path}`, run the smallest check and confirm the interface constraint before making consecutive edits."),
            ),
            "repeated-errors" => (
                "Errors or retries repeated".into(),
                format!("{} errors and {} retries were observed.", evidence.errors, evidence.retries),
            ),
            "repeated-errors-action" => (
                "Switch to diagnosis after the second similar failure".into(),
                "Record the failure condition, a minimal reproduction, and one falsifiable hypothesis instead of repeating the same action.".into(),
            ),
            "long-without-verification" => (
                "Changes continued without verification evidence".into(),
                format!("About {} active minutes included changes, but no test, build, type-check, or commit evidence was detected.", evidence.active_seconds / 60),
            ),
            "long-without-verification-action" => (
                "Move verification to the first working slice".into(),
                "Run the smallest verification as soon as the first executable slice exists instead of waiting until the end.".into(),
            ),
            "model-switches" => (
                "The task switched models repeatedly".into(),
                format!("{} model switches were observed; check whether each switch resolved a specific blocker.", evidence.model_switches),
            ),
            _ => ("Evidence unavailable".into(), "The current records do not support a reliable conclusion.".into()),
        }
    }
}

pub fn review_content(
    locale: &str,
    evidence: &ReviewEvidence,
    findings: &[ReviewFinding],
) -> ReviewContent {
    let facts = findings
        .iter()
        .filter(|item| item.tier == "fact")
        .map(|item| format!("• {}", item.detail))
        .collect::<Vec<_>>()
        .join("\n");
    let friction = findings
        .iter()
        .filter(|item| item.tier == "inference")
        .map(|item| format!("• {}", item.detail))
        .collect::<Vec<_>>()
        .join("\n");
    let next_run = findings
        .iter()
        .filter(|item| item.tier == "suggestion")
        .map(|item| format!("• {}", item.detail))
        .collect::<Vec<_>>()
        .join("\n");
    let objective = evidence.objectives.first().map(|value| bounded(value, 220));
    let result = evidence
        .result_excerpts
        .first()
        .map(|value| bounded(value, 280));
    let commits = evidence
        .commit_subjects
        .iter()
        .take(3)
        .map(|value| bounded(value, 100))
        .collect::<Vec<_>>()
        .join("；");
    let tools = evidence
        .top_tools
        .iter()
        .take(4)
        .map(|(name, count)| format!("{name} × {count}"))
        .collect::<Vec<_>>()
        .join("、");
    if locale == "zh-CN" {
        let outcome = if !commits.is_empty() {
            format!("有可追溯的提交：{commits}。")
        } else if evidence.verification_events > 0 {
            format!(
                "跑过 {} 次测试、构建或检查；{}",
                evidence.verification_events,
                result
                    .as_deref()
                    .map(|value| format!("最终回复摘要：{value}"))
                    .unwrap_or_else(|| "没有留下可用的最终回复摘要。".into())
            )
        } else if evidence.files_changed > 0 {
            format!(
                "改了 {} 个文件、{} 行，但没看到测试或提交。",
                evidence.files_changed,
                evidence.lines_added.saturating_add(evidence.lines_deleted)
            )
        } else if let Some(result) = result.as_deref() {
            format!("最终回复摘要：{result}；没有文件、测试或提交可以单独确认交付。")
        } else {
            "没有可确认的文件修改、测试或提交结果。".into()
        };
        let mut happened = Vec::new();
        if let Some(objective) = objective.as_deref() {
            happened.push(format!("目标摘要：{objective}"));
        }
        happened.push(format!(
            "共归并 {} 个工作事件、{} 段会话；活跃约 {} 分钟，涉及 {} 个文件，新增 {} 行、删除 {} 行。",
            evidence.task_count.max(1),
            evidence.session_count,
            evidence.active_seconds / 60,
            evidence.files_changed,
            evidence.lines_added,
            evidence.lines_deleted
        ));
        if evidence.total_tokens > 0 {
            happened.push(format!(
                "本次约消耗 {} token；错误 {} 次、重试 {} 次。",
                compact_count(evidence.total_tokens),
                evidence.errors,
                evidence.retries
            ));
        }
        if !tools.is_empty() {
            happened.push(format!("主要工具证据：{tools}。"));
        }
        ReviewContent {
            outcome,
            what_happened: happened.join("\n"),
            what_worked: facts,
            friction,
            lessons: String::new(),
            next_run,
        }
    } else {
        let outcome = if !commits.is_empty() {
            format!("Traceable commits: {commits}.")
        } else if evidence.verification_events > 0 {
            format!(
                "{} test, build, or check runs showed up. {}",
                evidence.verification_events,
                result
                    .as_deref()
                    .map(|value| format!("Final-response excerpt: {value}"))
                    .unwrap_or_else(|| "No usable final-response excerpt was kept.".into())
            )
        } else if evidence.files_changed > 0 {
            format!(
                "{} files and {} lines changed, but no test or commit showed up.",
                evidence.files_changed,
                evidence.lines_added.saturating_add(evidence.lines_deleted)
            )
        } else if let Some(result) = result.as_deref() {
            format!(
                "Final-response excerpt: {result} No file, test, or commit evidence independently confirms delivery."
            )
        } else {
            "No file change, test, or commit result could be confirmed.".into()
        };
        let mut happened = Vec::new();
        if let Some(objective) = objective.as_deref() {
            happened.push(format!("Objective excerpt: {objective}"));
        }
        happened.push(format!(
            "{} work events combine {} sessions over about {} active minutes, touching {} files with {} added and {} removed lines.",
            evidence.task_count.max(1),
            evidence.session_count,
            evidence.active_seconds / 60,
            evidence.files_changed,
            evidence.lines_added,
            evidence.lines_deleted
        ));
        if evidence.total_tokens > 0 {
            happened.push(format!(
                "About {} tokens were spent, with {} errors and {} retries.",
                compact_count(evidence.total_tokens),
                evidence.errors,
                evidence.retries
            ));
        }
        if !tools.is_empty() {
            happened.push(format!("Observed tools: {tools}."));
        }
        ReviewContent {
            outcome,
            what_happened: happened.join("\n"),
            what_worked: facts,
            friction,
            lessons: String::new(),
            next_run,
        }
    }
}

fn compact_count(value: u64) -> String {
    if value >= 1_000_000_000 {
        format!("{:.1}B", value as f64 / 1e9)
    } else if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1e6)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1e3)
    } else {
        value.to_string()
    }
}

fn bounded(value: &str, max_chars: usize) -> String {
    let mut characters = value.chars();
    let text = characters.by_ref().take(max_chars).collect::<String>();
    if characters.next().is_some() {
        format!("{text}…")
    } else {
        text
    }
}
