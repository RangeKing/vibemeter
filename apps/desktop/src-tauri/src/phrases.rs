use crate::models::{ParseState, PhraseAggregate};
use crate::privacy::stable_hash;
use chrono::{DateTime, Local};
use jieba_rs::Jieba;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{HashMap, HashSet};

const MAX_PHRASES_PER_MESSAGE: usize = 180;
const MAX_PERSISTED_PHRASES_PER_ROLE_DATE: usize = 240;

static JIEBA: Lazy<Jieba> = Lazy::new(Jieba::new);
static CODE_BLOCK_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?s)```.*?```").expect("code block regex"));
static INLINE_CODE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"`[^`\r\n]+`").expect("inline code regex"));
static URL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(?:https?|file)://\S+").expect("url regex"));
static PATH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:^|\s)(?:~?/|[A-Za-z]:\\)[^\s，。！？；：,.!?;:]+").expect("path regex")
});
static SECRET_ASSIGNMENT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?i)\b(?:api[_-]?key|access[_-]?token|secret|password|authorization)\b\s*[:=]\s*["']?\S+"#,
    )
    .expect("secret assignment regex")
});
static HIGH_ENTROPY_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b[A-Za-z0-9_+/=-]{28,}\b").expect("high entropy token regex"));
static MARKUP_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?s)<[^>]{1,240}>").expect("markup regex"));

static ENGLISH_STOPWORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "a", "about", "after", "again", "all", "also", "am", "an", "and", "any", "are", "as", "at",
        "be", "because", "been", "before", "being", "but", "by", "can", "code", "could", "did",
        "do", "does", "doing", "done", "each", "file", "files", "for", "from", "get", "got", "had",
        "has", "have", "here", "how", "i", "if", "in", "into", "is", "it", "its", "just", "let",
        "like", "may", "me", "more", "most", "my", "need", "no", "not", "now", "of", "on", "one",
        "only", "or", "our", "out", "please", "project", "same", "session", "should", "so", "some",
        "than", "that", "the", "their", "them", "then", "there", "these", "they", "this", "those",
        "to", "tool", "up", "use", "used", "using", "very", "want", "was", "we", "were", "what",
        "when", "where", "which", "who", "why", "will", "with", "would", "you", "your",
    ]
    .into_iter()
    .collect()
});

static CHINESE_STOPWORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "一个",
        "一些",
        "以及",
        "但是",
        "不是",
        "不要",
        "不能",
        "不过",
        "为什么",
        "为了",
        "之前",
        "之后",
        "他们",
        "你们",
        "使用",
        "可以",
        "可能",
        "同时",
        "因为",
        "如果",
        "它们",
        "对于",
        "已经",
        "应该",
        "怎么",
        "我们",
        "或者",
        "所以",
        "文件",
        "是否",
        "有些",
        "没有",
        "然后",
        "现在",
        "用户",
        "项目",
        "代码",
        "自己",
        "这个",
        "这些",
        "这里",
        "这样",
        "那个",
        "那些",
        "还是",
        "进行",
        "通过",
        "需要",
    ]
    .into_iter()
    .collect()
});

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Chinese,
    English,
}

#[derive(Clone)]
struct PhraseToken {
    text: String,
    kind: TokenKind,
    stopword: bool,
}

pub fn observe(state: &mut ParseState, role: &str, text: &str) {
    let text = text.trim();
    if text.chars().count() < 2 {
        return;
    }
    let timestamp = state
        .last_timestamp
        .as_deref()
        .or(state.started_at.as_deref())
        .unwrap_or_default();
    let fingerprint = stable_hash(&format!("{role}\u{1f}{timestamp}\u{1f}{text}"));
    if state
        .last_phrase_fingerprints
        .get(role)
        .is_some_and(|previous| previous == &fingerprint)
    {
        return;
    }
    state
        .last_phrase_fingerprints
        .insert(role.to_string(), fingerprint);

    let date = local_date(timestamp);
    for (phrase, occurrences) in extract(text) {
        let key = format!("{date}\u{1f}{role}\u{1f}{phrase}");
        let aggregate = state
            .phrase_counts
            .entry(key)
            .or_insert_with(|| PhraseAggregate {
                date: date.clone(),
                role: role.to_string(),
                phrase,
                occurrences: 0,
            });
        aggregate.occurrences = aggregate.occurrences.saturating_add(occurrences);
    }
}

pub fn extract(text: &str) -> Vec<(String, u64)> {
    let clean = sanitize(text);
    let mut counts = HashMap::<String, u64>::new();
    let mut segment = Vec::<PhraseToken>::new();

    for token in JIEBA.cut(&clean, false) {
        if token.word.chars().all(char::is_whitespace) {
            continue;
        }
        let Some(normalized) = normalize_token(token.word) else {
            collect_segment(&segment, &mut counts);
            segment.clear();
            continue;
        };
        if segment
            .last()
            .is_some_and(|previous| previous.kind != normalized.kind)
        {
            collect_segment(&segment, &mut counts);
            segment.clear();
        }
        segment.push(normalized);
    }
    collect_segment(&segment, &mut counts);

    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| right.0.chars().count().cmp(&left.0.chars().count()))
            .then_with(|| left.0.cmp(&right.0))
    });
    ranked.truncate(MAX_PHRASES_PER_MESSAGE);
    ranked
}

pub fn compact(state: &mut ParseState) {
    let mut ranked = state.phrase_counts.drain().collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        left.1
            .date
            .cmp(&right.1.date)
            .then_with(|| left.1.role.cmp(&right.1.role))
            .then_with(|| right.1.occurrences.cmp(&left.1.occurrences))
            .then_with(|| {
                right
                    .1
                    .phrase
                    .chars()
                    .count()
                    .cmp(&left.1.phrase.chars().count())
            })
            .then_with(|| left.1.phrase.cmp(&right.1.phrase))
    });

    let mut retained_by_group = HashMap::<(String, String), usize>::new();
    for (key, aggregate) in ranked {
        let group = (aggregate.date.clone(), aggregate.role.clone());
        let retained = retained_by_group.entry(group).or_default();
        if *retained >= MAX_PERSISTED_PHRASES_PER_ROLE_DATE {
            continue;
        }
        *retained += 1;
        state.phrase_counts.insert(key, aggregate);
    }
}

fn sanitize(text: &str) -> String {
    let without_code = CODE_BLOCK_RE.replace_all(text, " ");
    let without_inline = INLINE_CODE_RE.replace_all(&without_code, " ");
    let without_urls = URL_RE.replace_all(&without_inline, " ");
    let without_paths = PATH_RE.replace_all(&without_urls, " ");
    let without_secrets = SECRET_ASSIGNMENT_RE.replace_all(&without_paths, " ");
    let without_entropy = HIGH_ENTROPY_RE.replace_all(&without_secrets, " ");
    MARKUP_RE.replace_all(&without_entropy, " ").into_owned()
}

fn normalize_token(raw: &str) -> Option<PhraseToken> {
    let text = raw
        .trim()
        .trim_matches(|character: char| {
            !character.is_alphanumeric()
                && !is_chinese(character)
                && character != '-'
                && character != '\''
        })
        .trim();
    if text.is_empty() {
        return None;
    }
    if text.chars().all(is_chinese) {
        let count = text.chars().count();
        if count > 8 {
            return None;
        }
        return Some(PhraseToken {
            text: text.to_string(),
            kind: TokenKind::Chinese,
            stopword: CHINESE_STOPWORDS.contains(text),
        });
    }
    if text
        .chars()
        .all(|character| character.is_ascii_alphabetic() || matches!(character, '-' | '\''))
    {
        if !text
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        {
            return None;
        }
        let normalized = text.to_ascii_lowercase();
        if normalized.len() > 28 {
            return None;
        }
        return Some(PhraseToken {
            stopword: ENGLISH_STOPWORDS.contains(normalized.as_str()),
            text: normalized,
            kind: TokenKind::English,
        });
    }
    None
}

fn collect_segment(segment: &[PhraseToken], counts: &mut HashMap<String, u64>) {
    for start in 0..segment.len() {
        for length in 1..=3 {
            let end = start + length;
            if end > segment.len() {
                break;
            }
            let slice = &segment[start..end];
            if slice.iter().all(|token| token.stopword) {
                continue;
            }
            let phrase = match slice[0].kind {
                TokenKind::Chinese => slice
                    .iter()
                    .map(|token| token.text.as_str())
                    .collect::<String>(),
                TokenKind::English => slice
                    .iter()
                    .map(|token| token.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            };
            let characters = phrase.chars().count();
            let valid = match slice[0].kind {
                TokenKind::Chinese => (2..=8).contains(&characters),
                TokenKind::English => {
                    (length > 1 || phrase.len() >= 4)
                        && phrase.len() <= 72
                        && slice.iter().filter(|token| !token.stopword).count() >= 1
                }
            };
            if valid {
                *counts.entry(phrase).or_default() += 1;
            }
        }
    }
}

fn local_date(timestamp: &str) -> String {
    if let Ok(value) = DateTime::parse_from_rfc3339(timestamp) {
        return value.with_timezone(&Local).format("%Y-%m-%d").to_string();
    }
    if timestamp.len() >= 10
        && timestamp.as_bytes().get(4) == Some(&b'-')
        && timestamp.as_bytes().get(7) == Some(&b'-')
    {
        return timestamp[..10].to_string();
    }
    Local::now().format("%Y-%m-%d").to_string()
}

fn is_chinese(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AgentKind;

    #[test]
    fn extracts_useful_bilingual_phrases_without_code_or_paths() {
        let phrases = extract(
            "请先运行测试验证，然后修复失败。路径 /Users/test/private.rs。\
             I will run tests before shipping. `secret_value`",
        );
        let values = phrases
            .iter()
            .map(|(phrase, _)| phrase.as_str())
            .collect::<HashSet<_>>();
        assert!(values.contains("测试验证") || values.contains("运行测试"));
        assert!(values.contains("run tests"));
        assert!(!values.iter().any(|phrase| phrase.contains("private")));
        assert!(!values.iter().any(|phrase| phrase.contains("secret")));
    }

    #[test]
    fn records_phrase_role_and_observed_local_date() {
        let mut state = ParseState::new(AgentKind::Codex, "session".into());
        state.last_timestamp = Some("2026-07-25T04:00:00Z".into());
        observe(&mut state, "user", "请运行测试验证，运行测试验证。");
        assert!(
            state
                .phrase_counts
                .values()
                .any(|item| item.role == "user" && item.date == "2026-07-25")
        );
    }

    #[test]
    fn deduplicates_adjacent_parser_replays() {
        let mut state = ParseState::new(AgentKind::ClaudeCode, "session".into());
        state.last_timestamp = Some("2026-07-25T04:00:00Z".into());
        observe(&mut state, "agent", "Run tests before shipping.");
        let before = state
            .phrase_counts
            .values()
            .map(|item| item.occurrences)
            .sum::<u64>();
        observe(&mut state, "agent", "Run tests before shipping.");
        let after = state
            .phrase_counts
            .values()
            .map(|item| item.occurrences)
            .sum::<u64>();
        assert_eq!(before, after);
    }

    #[test]
    fn rejects_punctuation_only_english_tokens() {
        assert!(extract("---- --- '--' ··").is_empty());
    }

    #[test]
    fn compacts_persisted_candidates_per_role_and_date() {
        let mut state = ParseState::new(AgentKind::Codex, "session".into());
        for role in ["user", "agent"] {
            for index in 0..400_u64 {
                let phrase = format!("phrase-{index:03}");
                state.phrase_counts.insert(
                    format!("2026-07-25\u{1f}{role}\u{1f}{phrase}"),
                    PhraseAggregate {
                        date: "2026-07-25".into(),
                        role: role.into(),
                        phrase,
                        occurrences: index + 1,
                    },
                );
            }
        }

        compact(&mut state);

        assert_eq!(
            state.phrase_counts.len(),
            MAX_PERSISTED_PHRASES_PER_ROLE_DATE * 2
        );
        assert!(
            state
                .phrase_counts
                .values()
                .any(|item| item.role == "user" && item.occurrences == 400)
        );
        assert!(
            !state
                .phrase_counts
                .values()
                .any(|item| item.role == "agent" && item.occurrences == 1)
        );
    }
}
