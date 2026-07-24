use crate::models::{GitCommitEvidence, GitEvidence, GitFileStat};
use crate::privacy::{safe_project_relative_path, sanitize_git_subject};
use std::path::Path;
use std::process::Command;

const MAX_COMMITS_PER_SESSION: usize = 24;

pub fn inspect(root: &Path, started_at: Option<&str>, ended_at: Option<&str>) -> GitEvidence {
    let repository_root: String = match git_output(root, &["rev-parse", "--show-toplevel"]) {
        Some(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => {
            return GitEvidence {
                available: false,
                state: "no-repository".into(),
                ..GitEvidence::default()
            };
        }
    };
    let repository_root = Path::new(&repository_root);
    let branch = git_output(repository_root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && value != "HEAD");

    let mut arguments = vec![
        "log".to_string(),
        "--no-renames".to_string(),
        "--numstat".to_string(),
        format!("--max-count={MAX_COMMITS_PER_SESSION}"),
        "--format=@@%H%x1f%cI%x1f%s".to_string(),
    ];
    if let Some(started_at) = started_at {
        arguments.push(format!("--since={started_at}"));
    }
    if let Some(ended_at) = ended_at {
        arguments.push(format!("--until={ended_at}"));
    }
    let borrowed = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    let output = git_output(repository_root, &borrowed).unwrap_or_default();

    GitEvidence {
        available: true,
        state: "available".into(),
        branch,
        commits: parse_log(&output, repository_root),
    }
}

fn git_output(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn parse_log(output: &str, repository_root: &Path) -> Vec<GitCommitEvidence> {
    let mut commits = Vec::new();
    let mut current: Option<GitCommitEvidence> = None;
    for line in output.lines() {
        if let Some(header) = line.strip_prefix("@@") {
            if let Some(commit) = current.take() {
                commits.push(commit);
            }
            let mut fields = header.splitn(3, '\u{1f}');
            let hash = fields.next().unwrap_or_default();
            let committed_at = fields.next().unwrap_or_default();
            let subject = fields
                .next()
                .and_then(sanitize_git_subject)
                .unwrap_or_default();
            if hash.is_empty() || committed_at.is_empty() {
                continue;
            }
            current = Some(GitCommitEvidence {
                hash: hash.chars().take(12).collect(),
                subject,
                committed_at: committed_at.into(),
                files: Vec::new(),
            });
            continue;
        }
        let Some(commit) = current.as_mut() else {
            continue;
        };
        let mut fields = line.splitn(3, '\t');
        let added = fields.next().and_then(|value| value.parse().ok());
        let deleted = fields.next().and_then(|value| value.parse().ok());
        let path = fields
            .next()
            .and_then(|value| safe_project_relative_path(Some(repository_root), value));
        if let (Some(lines_added), Some(lines_deleted), Some(path)) = (added, deleted, path) {
            commit.files.push(GitFileStat {
                path,
                lines_added,
                lines_deleted,
            });
        }
    }
    if let Some(commit) = current {
        commits.push(commit);
    }
    commits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commit_headers_and_numstat_without_diff_bodies() {
        let output = "@@abcdef0123456789\u{1f}2026-07-21T10:00:00+08:00\u{1f}Ship review page\n12\t3\tsrc/review.ts\n";
        let commits = parse_log(output, Path::new("/tmp/project"));
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, "abcdef012345");
        assert_eq!(commits[0].files[0].path, "src/review.ts");
        assert_eq!(commits[0].files[0].lines_added, 12);
    }
}
