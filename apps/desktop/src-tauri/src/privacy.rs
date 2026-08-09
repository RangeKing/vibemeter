use crate::models::{ShareGuardFinding, ShareRenderRequest};
use once_cell::sync::Lazy;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

static ABSOLUTE_PATH: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(?:/Users/[^\s]+|/home/[^\s]+|[A-Z]:\\Users\\[^\s]+)")
        .expect("valid path regex")
});
static EMAIL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b").expect("valid email regex")
});
static SECRET: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(?:sk-[a-z0-9_-]{12,}|api[_-]?key\s*[:=]\s*\S+|authorization:\s*bearer\s+\S+|-----BEGIN [A-Z ]*PRIVATE KEY-----)")
        .expect("valid secret regex")
});
static REPOSITORY_URL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(?:https?://|git@)[^\s]+(?:\.git)?").expect("valid repository regex")
});

pub fn stable_hash(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(&hasher.finalize()[..8])
}

pub fn safe_opaque_identifier(value: &str) -> String {
    let unsafe_value = value.contains(['/', '\\'])
        || value.chars().any(char::is_control)
        || value.chars().count() > 128;
    if unsafe_value {
        format!("private-{}", stable_hash(value))
    } else {
        value.into()
    }
}

static LEADING_MARKER: Lazy<Regex> = Lazy::new(|| {
    // Matches, at the start of a string, one "noise" token that prompts often begin
    // with: a complete markdown/skill link `[label](target)`, a broken link whose target
    // paren was swallowed by path redaction `[label](`, a bracket tag `[$skill]`, a leftover
    // redaction fragment `([path])` / `[path]`, a slash command `/foo`, an `@mention`, a
    // leading session UUID, or a list number like `1.` / `2)`. Applied repeatedly to peel
    // every leading marker. Order matters: complete links are tried before broken ones.
    Regex::new(concat!(
        r"^\s*(?:",
        r"\[[^\]]*\]\([^)]*\)", // [label](target)
        r"|\[[^\]]*\]\(",       // [label]( (redaction ate the ")")
        r"|\[[^\]]*\]",         // [tag]
        r"|\(?\[[a-z]+\]\)?",   // ([path]) / [path]
        r"|/[\w.:-]+",          // /command
        r"|@[^\s]+",            // @mention
        r"|[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}", // uuid
        r"|\d+[.)、]",          // 1. / 2)
        r"|[-*·•]",             // bullet
        r")\s*",
    ))
    .expect("valid leading marker regex")
});

/// Strips leading command/skill/mention/list markers so a raw prompt like
/// `[$grill-me]([path] 1. TokenGraph …` reads as a clean headline `TokenGraph …`.
pub fn clean_display_title(value: &str) -> String {
    let mut current = value.trim().to_string();
    loop {
        let stripped = LEADING_MARKER.replace(&current, "").to_string();
        if stripped == current {
            break;
        }
        current = stripped;
    }
    let cleaned = current.trim().to_string();
    if cleaned.is_empty() {
        value.trim().to_string()
    } else {
        cleaned
    }
}

pub fn sanitize_title(value: &str) -> Option<String> {
    sanitize_bounded_text(value, 96).map(|title| clean_display_title(&title))
}

pub fn sanitize_prompt_excerpt(value: &str) -> Option<String> {
    sanitize_bounded_text(value, 240)
}

pub fn sanitize_result_excerpt(value: &str) -> Option<String> {
    sanitize_bounded_text(value, 800)
}

pub fn sanitize_git_subject(value: &str) -> Option<String> {
    sanitize_bounded_text(value, 160)
}

fn sanitize_bounded_text(value: &str, max_chars: usize) -> Option<String> {
    let normalized = value
        .replace(['\r', '\n', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        return None;
    }
    let redacted = ABSOLUTE_PATH.replace_all(&normalized, "[path]");
    let redacted = EMAIL.replace_all(&redacted, "[email]");
    let redacted = SECRET.replace_all(&redacted, "[secret]");
    let redacted = REPOSITORY_URL.replace_all(&redacted, "[url]");
    let mut chars = redacted.chars();
    let compact = chars.by_ref().take(max_chars).collect::<String>();
    let suffix = if chars.next().is_some() { "…" } else { "" };
    Some(format!("{compact}{suffix}"))
}

pub fn sanitize_tool_name(value: &str) -> String {
    if ABSOLUTE_PATH.is_match(value)
        || EMAIL.is_match(value)
        || SECRET.is_match(value)
        || REPOSITORY_URL.is_match(value)
    {
        return "other".into();
    }
    let compact = value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        .take(48)
        .collect::<String>();
    if compact.is_empty() {
        "other".into()
    } else {
        compact
    }
}

pub fn safe_project_relative_path(project_root: Option<&Path>, raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches(['\'', '"']);
    if trimmed.is_empty() || SECRET.is_match(trimmed) {
        return None;
    }
    let input = Path::new(trimmed);
    let relative = if input.is_absolute() {
        if let Some(root) = project_root.and_then(|root| input.strip_prefix(root).ok()) {
            root.to_path_buf()
        } else {
            let name = input.file_name()?.to_string_lossy();
            return Some(format!("[external]/{name}"));
        }
    } else {
        input.to_path_buf()
    };

    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::ParentDir => return None,
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

pub fn inspect_share(request: &ShareRenderRequest) -> Vec<ShareGuardFinding> {
    let mut findings = Vec::new();
    let custom_text = format!(
        "{}\n{}\n{}",
        request.title, request.summary, request.project_name
    );

    if SECRET.is_match(&custom_text) {
        findings.push(finding("secret", "block", "share.guard.secretDetected"));
    }
    if ABSOLUTE_PATH.is_match(&custom_text) {
        findings.push(finding(
            "absolute-path",
            "block",
            "share.guard.pathDetected",
        ));
    }
    if EMAIL.is_match(&custom_text) {
        findings.push(finding("email", "review", "share.guard.emailDetected"));
    }
    if REPOSITORY_URL.is_match(&custom_text) {
        findings.push(finding(
            "repository-url",
            "review",
            "share.guard.urlDetected",
        ));
    }
    if !custom_text.trim().is_empty() && !request.privacy_reviewed {
        findings.push(finding(
            "custom-text",
            "review",
            "share.guard.customTextReview",
        ));
    }
    if request.show_project && !request.project_name.trim().is_empty() {
        findings.push(finding(
            "project-visible",
            "review",
            "share.guard.projectVisible",
        ));
    }

    if findings.is_empty() {
        findings.push(finding("safe", "safe", "share.guard.safe"));
    }
    findings
}

pub fn export_allowed(findings: &[ShareGuardFinding], reviewed: bool) -> bool {
    !findings.iter().any(|finding| finding.level == "block")
        && (reviewed || !findings.iter().any(|finding| finding.level == "review"))
}

fn finding(id: &str, level: &str, message_key: &str) -> ShareGuardFinding {
    ShareGuardFinding {
        id: id.into(),
        level: level.into(),
        message_key: message_key.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(title: &str) -> ShareRenderRequest {
        ShareRenderRequest {
            template_id: "usage-overview".into(),
            locale: "en-US".into(),
            aspect_ratio: "landscape".into(),
            theme: "light".into(),
            range: "today".into(),
            session_id: None,
            compare_ids: Vec::new(),
            title: title.into(),
            summary: String::new(),
            project_name: String::new(),
            metrics: Vec::new(),
            show_brand: true,
            show_model: false,
            show_cost: false,
            show_project: false,
            show_behavior_evidence: false,
            privacy_reviewed: false,
        }
    }

    #[test]
    fn sanitizes_private_text_without_retaining_the_value() {
        let title = sanitize_title("Fix /Users/alice/Secret/app.rs with sk-test_1234567890")
            .expect("title");
        assert_eq!(title, "Fix [path] with [secret]");
    }

    #[test]
    fn strips_command_and_skill_markers_from_titles() {
        assert_eq!(
            clean_display_title("[$grill-me]([path] 1. TokenGraph 原有的数据显示功能"),
            "TokenGraph 原有的数据显示功能"
        );
        assert_eq!(
            clean_display_title("/goal [$thesis-writer]([path] 根据 AGENT.md 要求"),
            "根据 AGENT.md 要求"
        );
        assert_eq!(
            clean_display_title(
                "[$skill-creator]([path] 019f7feb-5887-7860-b7c1-df993a646019 会话生成的结果"
            ),
            "会话生成的结果"
        );
        assert_eq!(
            clean_display_title("[label](https://example.com/x) real headline"),
            "real headline"
        );
        // A title that is nothing but markers falls back to the original, never empty.
        assert_eq!(clean_display_title("[$only-marker]"), "[$only-marker]");
    }

    #[test]
    fn share_guard_blocks_secrets_and_absolute_paths_even_after_review() {
        for title in [
            "Use api_key=top-secret-token-value",
            "Review /Users/alice/private/repo/main.rs",
        ] {
            let findings = inspect_share(&request(title));
            assert!(findings.iter().any(|item| item.level == "block"));
            assert!(!export_allowed(&findings, true));
        }
    }

    #[test]
    fn share_guard_requires_confirmation_for_email_and_repository_url() {
        for title in [
            "Send to person@example.com",
            "Publish https://example.com/private.git",
        ] {
            let findings = inspect_share(&request(title));
            assert!(findings.iter().any(|item| item.level == "review"));
            assert!(!export_allowed(&findings, false));
            assert!(export_allowed(&findings, true));
        }
    }
}
