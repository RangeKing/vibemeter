use crate::models::ParseState;
use regex::Regex;
use std::collections::{BTreeSet, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use walkdir::WalkDir;

static KNOWN_SKILLS: OnceLock<HashSet<String>> = OnceLock::new();
static EXPLICIT_SKILL_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
static FRONTMATTER_NAME: OnceLock<Regex> = OnceLock::new();

fn explicit_skill_patterns() -> &'static [Regex] {
    EXPLICIT_SKILL_PATTERNS.get_or_init(|| {
        [
            r"\[\$([A-Za-z0-9][A-Za-z0-9._:-]{0,79})\]\([^\n)]*SKILL\.md[^\n)]*\)",
            r"\$([A-Za-z0-9][A-Za-z0-9._:-]{0,79})",
            r"(?s)<skill>.*?<name>/?([A-Za-z0-9][A-Za-z0-9._:-]{0,79})</name>.*?</skill>",
            r"<command-name>/?([A-Za-z0-9][A-Za-z0-9._:-]{0,79})</command-name>",
        ]
        .into_iter()
        .map(|pattern| Regex::new(pattern).expect("explicit Skill regex must compile"))
        .collect()
    })
}

fn skill_roots() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    [
        home.join(".agents/skills"),
        home.join(".codex/skills"),
        home.join(".claude/skills"),
        home.join(".codex/plugins/cache"),
    ]
    .into_iter()
    .filter(|path| path.is_dir())
    .collect()
}

fn plugin_prefix(path: &Path) -> Option<String> {
    let components = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str().map(str::to_owned),
            _ => None,
        })
        .collect::<Vec<_>>();
    let plugins = components.iter().position(|part| part == "plugins")?;
    if components.get(plugins + 1).map(String::as_str) != Some("cache") {
        return None;
    }
    components.get(plugins + 3).cloned()
}

fn skill_name_from_file(path: &Path) -> Option<String> {
    let source = std::fs::read_to_string(path).ok()?;
    let name_regex = FRONTMATTER_NAME.get_or_init(|| {
        Regex::new(r#"(?m)^name:\s*[\"']?([^\"'\r\n]+)"#)
            .expect("Skill frontmatter regex must compile")
    });
    let raw = name_regex
        .captures(&source)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().trim().to_ascii_lowercase())
        .or_else(|| {
            path.parent()?
                .file_name()?
                .to_str()
                .map(str::to_ascii_lowercase)
        })?;
    if let Some(prefix) = plugin_prefix(path) {
        Some(format!("{}:{raw}", prefix.to_ascii_lowercase()))
    } else {
        Some(raw)
    }
}

pub fn installed_skill_names() -> Vec<String> {
    let mut names = BTreeSet::new();
    for root in skill_roots() {
        for entry in WalkDir::new(root)
            .follow_links(false)
            .max_depth(9)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file() && entry.file_name() == "SKILL.md")
        {
            if let Some(name) = skill_name_from_file(entry.path()) {
                names.insert(name);
            }
        }
    }
    names.into_iter().collect()
}

fn known_skills() -> &'static HashSet<String> {
    KNOWN_SKILLS.get_or_init(|| installed_skill_names().into_iter().collect())
}

fn extract_explicit_invocations(text: &str, known: &HashSet<String>) -> BTreeSet<String> {
    explicit_skill_patterns()
        .iter()
        .flat_map(|pattern| pattern.captures_iter(text))
        .filter_map(|captures| captures.get(1))
        .map(|value| value.as_str().trim().to_ascii_lowercase())
        .filter(|name| known.contains(name))
        .collect()
}

pub fn observe_explicit_invocations(state: &mut ParseState, text: &str) {
    for skill in extract_explicit_invocations(text, known_skills()) {
        let count = state.skill_counts.entry(skill).or_insert(0);
        *count = count.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(values: &[&str]) -> HashSet<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn extracts_explicit_forms_once_per_prompt() {
        let known = names(&["grill-me", "pdf", "github:yeet"]);
        let text = r#"Use [$grill-me](/tmp/grill-me/SKILL.md), then $pdf and $grill-me.
<command-name>/github:yeet</command-name>"#;
        assert_eq!(
            extract_explicit_invocations(text, &known),
            BTreeSet::from([
                "github:yeet".to_owned(),
                "grill-me".to_owned(),
                "pdf".to_owned(),
            ])
        );
    }

    #[test]
    fn ignores_shell_variables_and_unknown_names() {
        let known = names(&["pdf"]);
        assert_eq!(
            extract_explicit_invocations("$HOME $PATH $not-a-skill and $pdf", &known),
            BTreeSet::from(["pdf".to_owned()])
        );
    }
}
