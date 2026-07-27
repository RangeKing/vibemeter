pub fn text(locale: &str, key: &str) -> &'static str {
    match (normalize_locale(locale), key) {
        ("zh-CN", "template.usage-overview") => "使用概览",
        ("zh-CN", "template.developer-wrapped") => "开发者阶段回顾",
        ("zh-CN", "template.agent-comparison") => "Agent 对比",
        ("zh-CN", "template.session-recap") => "会话用量回顾",
        ("zh-CN", "template.vcti-card") => "VCTI 人格身份卡",
        ("zh-CN", "template.catchphrases") => "它最离不开的一句",
        ("zh-CN", "metric.sessions") => "会话",
        ("zh-CN", "metric.duration") => "使用时长",
        ("zh-CN", "metric.tokens") => "Token 总量",
        ("zh-CN", "metric.active-days") => "活跃天数",
        ("zh-CN", "metric.tasks") => "任务",
        ("zh-CN", "metric.verified") => "有验证证据",
        ("zh-CN", "metric.projects") => "涉及项目",
        ("zh-CN", "metric.commits") => "Git 提交",
        ("zh-CN", "metric.phases") => "过程阶段",
        ("zh-CN", "metric.input") => "输入",
        ("zh-CN", "metric.output") => "输出",
        ("zh-CN", "metric.cache") => "缓存",
        ("zh-CN", "metric.files") => "修改文件",
        ("zh-CN", "metric.lines") => "补丁行数",
        ("zh-CN", "metric.tool-calls") => "工具调用",
        ("zh-CN", "metric.longest-run") => "最长连续运行",
        ("zh-CN", "metric.added") => "新增",
        ("zh-CN", "metric.deleted") => "删除",
        ("zh-CN", "metric.cost") => "API 等价成本估算",
        ("zh-CN", "metric.share") => "使用占比",
        ("zh-CN", "metric.top-agent") => "最常用 Agent",
        ("zh-CN", "metric.top-model") => "最常用模型",
        ("zh-CN", "metric.active-date") => "最活跃的一天",
        ("zh-CN", "metric.trend") => "使用趋势",
        ("zh-CN", "label.agent") => "Agent",
        ("zh-CN", "label.model") => "模型",
        ("zh-CN", "label.date") => "日期",
        ("zh-CN", "label.project") => "项目",
        ("zh-CN", "label.real-data") => "本机真实数据",
        ("zh-CN", "label.observed") => "已观测",
        ("zh-CN", "label.estimated") => "估算",
        ("zh-CN", "label.usage-overview") => "DATA 01 / 使用概览",
        ("zh-CN", "label.developer-wrapped") => "DATA 02 / 开发者回顾",
        ("zh-CN", "label.agent-comparison") => "DATA 03 / AGENT 对比",
        ("zh-CN", "label.session-recap") => "DATA 04 / 会话用量",
        ("zh-CN", "label.vcti-card") => "VCTI / 真实行为人格",
        ("zh-CN", "label.catchphrases") => "VOICE / 抓包现场",
        ("zh-CN", "catchphrases.mine") => "我的口头禅",
        ("zh-CN", "catchphrases.agent") => "Agent 的口头禅",
        ("zh-CN", "catchphrases.subtitle") => "从真实会话里，抓出 Agent 反复套用的那句话。",
        ("zh-CN", "catchphrases.champion") => "冠军口癖",
        ("zh-CN", "catchphrases.roast") => "不说这句，看来就没法开工。",
        ("zh-CN", "catchphrases.unknown-source") => "Agent 未记录",
        ("zh-CN", "catchphrases.method") => {
            "原话 · 重复次数 · 跨会话统计 · 模型优先归因 · 仅在本机派生"
        }
        ("zh-CN", "catchphrases.insufficient") => "至少需要两个会话重复同一句短语",
        ("zh-CN", "label.focused-build") => "一次有证据的 Vibe Coding 交付",
        ("zh-CN", "label.activity-map") => "活跃节奏",
        ("zh-CN", "label.session-evidence") => "会话证据",
        ("zh-CN", "label.fact") => "事实",
        ("zh-CN", "label.suggestion") => "下次建议",
        ("zh-CN", "label.outcomes") => "主要成果",
        ("zh-CN", "label.process") => "观测过程",
        ("zh-CN", "label.evidence") => "交付证据",
        ("zh-CN", "label.next-week") => "下周改进",
        ("zh-CN", "status.verified") => "已验证",
        ("zh-CN", "status.changed") => "有修改，未见验证",
        ("zh-CN", "status.blocked") => "受阻",
        ("zh-CN", "status.unverified") => "未验证",
        ("zh-CN", "phase.understand") => "理解任务",
        ("zh-CN", "phase.explore") => "探索",
        ("zh-CN", "phase.edit") => "修改",
        ("zh-CN", "phase.verify") => "验证",
        ("zh-CN", "phase.recover") => "排错",
        ("zh-CN", "phase.other") => "其他",
        ("zh-CN", "reason.highTokenLowOutput") => "先限定唯一交付物和验收命令，再扩大上下文。",
        ("zh-CN", "reason.repeatedFileEdits") => "再次修改前先建立更早的检查点。",
        ("zh-CN", "reason.repeatedErrors") => "同类失败第二次出现后，切换为诊断流程。",
        ("zh-CN", "reason.longWithoutVerification") => "第一次成形修改后立即执行聚焦验证。",
        ("zh-CN", "reason.none") => "延续已有验证节奏，并保留可追溯的交付证据。",
        ("zh-CN", "empty.title") => "所选范围暂无有效会话",
        ("zh-CN", "empty.body") => "调整统计范围，或等待本地数据完成索引。",
        ("zh-CN", "brand.tagline") => "量化你的 Vibe Coding。",

        (_, "template.usage-overview") => "Usage Overview",
        (_, "template.developer-wrapped") => "Developer Wrapped",
        (_, "template.agent-comparison") => "Agent Comparison",
        (_, "template.session-recap") => "Session Recap",
        (_, "template.vcti-card") => "VCTI Identity Card",
        (_, "template.catchphrases") => "The line it can't quit",
        (_, "metric.sessions") => "Sessions",
        (_, "metric.duration") => "Time with agents",
        (_, "metric.tokens") => "Total tokens",
        (_, "metric.active-days") => "Active days",
        (_, "metric.tasks") => "Tasks",
        (_, "metric.verified") => "Evidence-backed",
        (_, "metric.projects") => "Projects touched",
        (_, "metric.commits") => "Git commits",
        (_, "metric.phases") => "Process phases",
        (_, "metric.input") => "Input",
        (_, "metric.output") => "Output",
        (_, "metric.cache") => "Cache",
        (_, "metric.files") => "Files changed",
        (_, "metric.lines") => "Patch lines",
        (_, "metric.tool-calls") => "Tool calls",
        (_, "metric.longest-run") => "Longest run",
        (_, "metric.added") => "Added",
        (_, "metric.deleted") => "Deleted",
        (_, "metric.cost") => "API-equivalent cost estimate",
        (_, "metric.share") => "Usage share",
        (_, "metric.top-agent") => "Top agent",
        (_, "metric.top-model") => "Top model",
        (_, "metric.active-date") => "Most active day",
        (_, "metric.trend") => "Usage trend",
        (_, "label.agent") => "Agent",
        (_, "label.model") => "Model",
        (_, "label.date") => "Date",
        (_, "label.project") => "Project",
        (_, "label.real-data") => "Real local data",
        (_, "label.observed") => "Observed",
        (_, "label.estimated") => "Estimated",
        (_, "label.usage-overview") => "DATA 01 / USAGE OVERVIEW",
        (_, "label.developer-wrapped") => "DATA 02 / DEVELOPER WRAPPED",
        (_, "label.agent-comparison") => "DATA 03 / AGENT COMPARISON",
        (_, "label.session-recap") => "DATA 04 / SESSION USAGE",
        (_, "label.vcti-card") => "VCTI / BEHAVIOR IDENTITY",
        (_, "label.catchphrases") => "VOICE / CAUGHT IN THE ACT",
        (_, "catchphrases.mine") => "My Catchphrases",
        (_, "catchphrases.agent") => "Agent Catchphrases",
        (_, "catchphrases.subtitle") => "The line your agent keeps recycling across real sessions.",
        (_, "catchphrases.champion") => "CHAMPION VERBAL TIC",
        (_, "catchphrases.roast") => "Apparently, work cannot start until it says this.",
        (_, "catchphrases.unknown-source") => "Agent not recorded",
        (_, "catchphrases.method") => {
            "Exact line · repeats · sessions · model-first attribution · derived locally"
        }
        (_, "catchphrases.insufficient") => "A phrase must repeat across at least two sessions",
        (_, "label.focused-build") => "An evidence-backed vibe coding delivery",
        (_, "label.activity-map") => "Activity rhythm",
        (_, "label.session-evidence") => "Session evidence",
        (_, "label.fact") => "FACT",
        (_, "label.suggestion") => "NEXT RUN",
        (_, "label.outcomes") => "MAIN OUTCOMES",
        (_, "label.process") => "OBSERVED PROCESS",
        (_, "label.evidence") => "DELIVERY EVIDENCE",
        (_, "label.next-week") => "NEXT WEEK",
        (_, "status.verified") => "Verified",
        (_, "status.changed") => "Changed, not verified",
        (_, "status.blocked") => "Blocked",
        (_, "status.unverified") => "Unverified",
        (_, "phase.understand") => "Understand",
        (_, "phase.explore") => "Explore",
        (_, "phase.edit") => "Edit",
        (_, "phase.verify") => "Verify",
        (_, "phase.recover") => "Recover",
        (_, "phase.other") => "Other",
        (_, "reason.highTokenLowOutput") => {
            "Name one deliverable and one acceptance command before widening context."
        }
        (_, "reason.repeatedFileEdits") => {
            "Create an earlier checkpoint before revisiting the same file."
        }
        (_, "reason.repeatedErrors") => {
            "Switch to diagnosis after the second failure of the same kind."
        }
        (_, "reason.longWithoutVerification") => {
            "Run a focused check after the first coherent edit."
        }
        (_, "reason.none") => {
            "Keep the current verification rhythm and retain traceable delivery evidence."
        }
        (_, "empty.title") => "No valid sessions in this range",
        (_, "empty.body") => "Choose another range or wait for local indexing to finish.",
        (_, "brand.tagline") => "Review the work behind the vibe.",
        _ => "",
    }
}

pub fn normalize_locale(locale: &str) -> &'static str {
    if locale.eq_ignore_ascii_case("zh-CN") || locale.starts_with("zh") {
        "zh-CN"
    } else {
        "en-US"
    }
}

pub fn format_number(locale: &str, value: u64) -> String {
    if normalize_locale(locale) == "zh-CN" {
        if value >= 100_000_000 {
            compact_decimal(value as f64 / 100_000_000.0, " 亿")
        } else if value >= 10_000 {
            compact_decimal(value as f64 / 10_000.0, " 万")
        } else {
            grouped(value, ',')
        }
    } else if value >= 1_000_000_000 {
        compact_decimal(value as f64 / 1_000_000_000.0, "B")
    } else if value >= 1_000_000 {
        compact_decimal(value as f64 / 1_000_000.0, "M")
    } else if value >= 1_000 {
        compact_decimal(value as f64 / 1_000.0, "K")
    } else {
        grouped(value, ',')
    }
}

pub fn format_duration(locale: &str, seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if normalize_locale(locale) == "zh-CN" {
        match (hours, minutes) {
            (0, 0) => format!("{} 秒", seconds),
            (0, m) => format!("{m} 分钟"),
            (h, 0) => format!("{h} 小时"),
            (h, m) => format!("{h} 小时 {m} 分"),
        }
    } else {
        match (hours, minutes) {
            (0, 0) => format!("{} sec", seconds),
            (0, m) => format!("{m} min"),
            (h, 0) => format!("{h} hr"),
            (h, m) => format!("{h} hr {m} min"),
        }
    }
}

pub fn format_range(locale: &str, range: &str) -> String {
    match (normalize_locale(locale), range) {
        ("zh-CN", "today") => "今天".into(),
        ("zh-CN", "7d") => "最近 7 天".into(),
        ("zh-CN", "30d") => "最近 30 天".into(),
        ("zh-CN", "90d") => "最近 90 天".into(),
        ("zh-CN", "180d") => "最近半年".into(),
        ("zh-CN", "year") => "最近一年".into(),
        ("zh-CN", "all") => "全部时间".into(),
        (_, "today") => "Today".into(),
        (_, "7d") => "Last 7 days".into(),
        (_, "30d") => "Last 30 days".into(),
        (_, "90d") => "Last 90 days".into(),
        (_, "180d") => "Last 6 months".into(),
        (_, "year") => "Last year".into(),
        (_, "all") => "All time".into(),
        _ => range.into(),
    }
}

pub fn format_sessions(locale: &str, sessions: u64) -> String {
    if normalize_locale(locale) == "zh-CN" {
        format!("{sessions} 个会话")
    } else if sessions == 1 {
        "1 session".into()
    } else {
        format!("{sessions} sessions")
    }
}

pub fn usage_overview_summary(locale: &str, sessions: u64, active_days: u64) -> String {
    if normalize_locale(locale) == "zh-CN" {
        format!(
            "{} 段本机会话分布在 {} 个活跃日，按 Agent、模型与日期还原使用轨迹。",
            grouped(sessions, ','),
            grouped(active_days, ',')
        )
    } else {
        format!(
            "{} local sessions across {} active days, mapped by agent, model, and date.",
            grouped(sessions, ','),
            grouped(active_days, ',')
        )
    }
}

pub fn developer_wrapped_summary(locale: &str, tokens: u64, active_days: u64) -> String {
    if normalize_locale(locale) == "zh-CN" {
        format!(
            "{} Token 与 {} 个活跃日，组成这段时间的开发者工作侧写。",
            format_number(locale, tokens),
            grouped(active_days, ',')
        )
    } else {
        format!(
            "{} tokens and {} active days form this period's developer portrait.",
            format_number(locale, tokens),
            grouped(active_days, ',')
        )
    }
}

pub fn agent_comparison_summary(locale: &str, agents: u64) -> String {
    if normalize_locale(locale) == "zh-CN" {
        format!(
            "对比 {} 个 Agent 的 Token、时长、会话与文件活动。",
            grouped(agents, ',')
        )
    } else {
        format!(
            "Comparing tokens, time, sessions, and file activity across {} agents.",
            grouped(agents, ',')
        )
    }
}

pub fn session_recap_summary(locale: &str, agent: &str, tokens: u64) -> String {
    if normalize_locale(locale) == "zh-CN" {
        format!(
            "{agent} 单次会话用量回顾：{} Token，并列出已观测工具与文件活动。",
            format_number(locale, tokens)
        )
    } else {
        format!(
            "One {agent} session used {} tokens, with observed tool and file activity.",
            format_number(locale, tokens)
        )
    }
}

fn compact_decimal(value: f64, suffix: &str) -> String {
    let formatted = if value >= 100.0 || (value.fract()).abs() < 0.05 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };
    format!("{formatted}{suffix}")
}

fn grouped(value: u64, separator: char) -> String {
    let source = value.to_string();
    let mut out = String::with_capacity(source.len() + source.len() / 3);
    for (index, character) in source.chars().enumerate() {
        if index > 0 && (source.len() - index).is_multiple_of(3) {
            out.push(separator);
        }
        out.push(character);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_compact_numbers_for_both_locales() {
        assert_eq!(format_number("zh-CN", 12_400), "1.2 万");
        assert_eq!(format_number("en-US", 12_400), "12.4K");
    }

    #[test]
    fn locale_normalization_is_stable() {
        assert_eq!(normalize_locale("zh-Hans"), "zh-CN");
        assert_eq!(normalize_locale("fr-FR"), "en-US");
    }
}
