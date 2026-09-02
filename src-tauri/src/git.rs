use std::{
    path::{Component, Path},
    process::{Command, Stdio},
};

use serde::{Deserialize, Serialize};

const MAX_STATUS_BYTES: usize = 8 * 1024 * 1024;
const MAX_CHANGES: usize = 10_000;
const MAX_COMMIT_MESSAGE_BYTES: usize = 16 * 1024;
const STATUS_ARGS: &[&str] = &[
    "--no-optional-locks",
    "-c",
    "core.quotepath=false",
    "-c",
    "core.fsmonitor=false",
    "status",
    "--porcelain=v2",
    "--branch",
    "-z",
    "--untracked-files=all",
];

#[derive(Clone, Debug)]
pub struct GitService {
    root: std::path::PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Untracked,
    Conflicted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileChange {
    pub path: String,
    pub previous_path: Option<String>,
    pub index: Option<ChangeKind>,
    pub worktree: Option<ChangeKind>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusSummary {
    pub branch: String,
    pub commit: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub changes: Vec<GitFileChange>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum GitError {
    NoWorkspace,
    GitUnavailable,
    NotARepository,
    TooManyChanges,
    InvalidOutput,
    InvalidPath,
    InvalidMessage,
    ChangeNotFound,
    CommandFailed,
}

impl GitService {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, GitError> {
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(|_| GitError::NoWorkspace)?;
        if !root.join(".git").exists() {
            return Err(GitError::NotARepository);
        }
        Ok(Self { root })
    }

    pub fn status(&self) -> Result<GitStatusSummary, GitError> {
        let output = Command::new("git")
            .args(STATUS_ARGS)
            .current_dir(&self.root)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("LC_ALL", "C")
            .output()
            .map_err(|_| GitError::GitUnavailable)?;

        if !output.status.success() {
            return Err(GitError::NotARepository);
        }
        if output.stdout.len() > MAX_STATUS_BYTES {
            return Err(GitError::TooManyChanges);
        }
        parse_status(&output.stdout)
    }

    pub fn stage(&self, path: &str) -> Result<GitStatusSummary, GitError> {
        let path = validate_repo_path(path)?;
        let status = self.status()?;
        if !status
            .changes
            .iter()
            .any(|change| change.path == path && change.worktree.is_some())
        {
            return Err(GitError::ChangeNotFound);
        }
        self.run_mutation(["add", "--", path])?;
        self.status()
    }

    pub fn unstage(&self, path: &str) -> Result<GitStatusSummary, GitError> {
        let path = validate_repo_path(path)?;
        let status = self.status()?;
        if !status
            .changes
            .iter()
            .any(|change| change.path == path && change.index.is_some())
        {
            return Err(GitError::ChangeNotFound);
        }
        if status.commit.is_some() {
            self.run_mutation(["restore", "--staged", "--", path])?;
        } else {
            self.run_mutation(["rm", "--cached", "--", path])?;
        }
        self.status()
    }

    pub fn commit(&self, message: &str) -> Result<GitStatusSummary, GitError> {
        let message = message.trim();
        if message.is_empty() || message.len() > MAX_COMMIT_MESSAGE_BYTES || message.contains('\0')
        {
            return Err(GitError::InvalidMessage);
        }
        if !self
            .status()?
            .changes
            .iter()
            .any(|change| change.index.is_some())
        {
            return Err(GitError::ChangeNotFound);
        }
        self.run_mutation([
            "commit",
            "--no-verify",
            "--no-gpg-sign",
            "--message",
            message,
        ])?;
        self.status()
    }

    fn run_mutation<const N: usize>(&self, args: [&str; N]) -> Result<(), GitError> {
        let status = Command::new("git")
            .arg("--no-optional-locks")
            .args(["-c", "core.hooksPath=/dev/null"])
            .args(args)
            .current_dir(&self.root)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| GitError::GitUnavailable)?;
        if status.success() {
            Ok(())
        } else {
            Err(GitError::CommandFailed)
        }
    }
}

fn validate_repo_path(value: &str) -> Result<&str, GitError> {
    if value.is_empty() || value.len() > 16 * 1024 || value.contains('\0') {
        return Err(GitError::InvalidPath);
    }
    let path = Path::new(value);
    if path.is_absolute()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(GitError::InvalidPath);
    }
    Ok(value)
}

fn parse_status(output: &[u8]) -> Result<GitStatusSummary, GitError> {
    let mut branch = "HEAD destacado".to_owned();
    let mut commit = None;
    let mut upstream = None;
    let mut ahead = 0;
    let mut behind = 0;
    let mut changes = Vec::new();
    let records: Vec<_> = output.split(|byte| *byte == 0).collect();
    let mut index = 0;

    while index < records.len() {
        let record = String::from_utf8_lossy(records[index]);
        index += 1;
        if record.is_empty() {
            continue;
        }

        if let Some(value) = record.strip_prefix("# branch.head ") {
            branch = value.to_owned();
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.oid ") {
            if value != "(initial)" {
                commit = Some(value.chars().take(8).collect());
            }
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.upstream ") {
            upstream = Some(value.to_owned());
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.ab ") {
            for part in value.split_whitespace() {
                if let Some(value) = part.strip_prefix('+') {
                    ahead = value.parse().map_err(|_| GitError::InvalidOutput)?;
                } else if let Some(value) = part.strip_prefix('-') {
                    behind = value.parse().map_err(|_| GitError::InvalidOutput)?;
                }
            }
            continue;
        }

        let change = match record.as_bytes().first() {
            Some(b'1') => parse_ordinary(&record)?,
            Some(b'2') => {
                let previous = records.get(index).ok_or(GitError::InvalidOutput)?;
                index += 1;
                parse_renamed(&record, previous)?
            }
            Some(b'u') => parse_unmerged(&record)?,
            Some(b'?') => GitFileChange {
                path: record.get(2..).ok_or(GitError::InvalidOutput)?.to_owned(),
                previous_path: None,
                index: None,
                worktree: Some(ChangeKind::Untracked),
            },
            Some(b'!') => continue,
            _ => return Err(GitError::InvalidOutput),
        };
        changes.push(change);
        if changes.len() > MAX_CHANGES {
            return Err(GitError::TooManyChanges);
        }
    }

    Ok(GitStatusSummary {
        branch,
        commit,
        upstream,
        ahead,
        behind,
        changes,
    })
}

fn parse_ordinary(record: &str) -> Result<GitFileChange, GitError> {
    let fields: Vec<_> = record.splitn(9, ' ').collect();
    if fields.len() != 9 {
        return Err(GitError::InvalidOutput);
    }
    let (index, worktree) = parse_xy(fields[1])?;
    Ok(GitFileChange {
        path: fields[8].to_owned(),
        previous_path: None,
        index,
        worktree,
    })
}

fn parse_renamed(record: &str, previous_path: &[u8]) -> Result<GitFileChange, GitError> {
    let fields: Vec<_> = record.splitn(10, ' ').collect();
    if fields.len() != 10 {
        return Err(GitError::InvalidOutput);
    }
    let (index, worktree) = parse_xy(fields[1])?;
    Ok(GitFileChange {
        path: fields[9].to_owned(),
        previous_path: Some(String::from_utf8_lossy(previous_path).into_owned()),
        index,
        worktree,
    })
}

fn parse_unmerged(record: &str) -> Result<GitFileChange, GitError> {
    let fields: Vec<_> = record.splitn(11, ' ').collect();
    if fields.len() != 11 {
        return Err(GitError::InvalidOutput);
    }
    Ok(GitFileChange {
        path: fields[10].to_owned(),
        previous_path: None,
        index: Some(ChangeKind::Conflicted),
        worktree: Some(ChangeKind::Conflicted),
    })
}

fn parse_xy(value: &str) -> Result<(Option<ChangeKind>, Option<ChangeKind>), GitError> {
    let mut chars = value.chars();
    let index = chars.next().ok_or(GitError::InvalidOutput)?;
    let worktree = chars.next().ok_or(GitError::InvalidOutput)?;
    if chars.next().is_some() {
        return Err(GitError::InvalidOutput);
    }
    Ok((parse_kind(index)?, parse_kind(worktree)?))
}

fn parse_kind(value: char) -> Result<Option<ChangeKind>, GitError> {
    Ok(match value {
        '.' => None,
        'A' => Some(ChangeKind::Added),
        'M' => Some(ChangeKind::Modified),
        'D' => Some(ChangeKind::Deleted),
        'R' => Some(ChangeKind::Renamed),
        'C' => Some(ChangeKind::Copied),
        'T' => Some(ChangeKind::TypeChanged),
        'U' => Some(ChangeKind::Conflicted),
        _ => return Err(GitError::InvalidOutput),
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn test_repository() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "lyrnova-git-test-{}-{}",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(status.success());
        root
    }

    #[test]
    fn parses_branch_tracking_and_worktree_changes() {
        let status = parse_status(
            b"# branch.oid 18bee5b7c160551b219554a1ef284ad7807f3ddd\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\x001 .M N... 100644 100644 100644 aaaaaaa bbbbbbb README.md\0? ui/new file.js\0",
        )
        .unwrap();

        assert_eq!(status.branch, "main");
        assert_eq!(status.commit.as_deref(), Some("18bee5b7"));
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));
        assert_eq!((status.ahead, status.behind), (2, 1));
        assert_eq!(status.changes.len(), 2);
        assert_eq!(status.changes[0].worktree, Some(ChangeKind::Modified));
        assert_eq!(status.changes[1].path, "ui/new file.js");
        assert_eq!(status.changes[1].worktree, Some(ChangeKind::Untracked));
    }

    #[test]
    fn parses_renames_and_original_path() {
        let status = parse_status(
            b"# branch.head feature\x002 R. N... 100644 100644 100644 aaaaaaa bbbbbbb R100 docs/new name.md\0docs/old name.md\0",
        )
        .unwrap();

        assert_eq!(status.changes[0].path, "docs/new name.md");
        assert_eq!(
            status.changes[0].previous_path.as_deref(),
            Some("docs/old name.md")
        );
        assert_eq!(status.changes[0].index, Some(ChangeKind::Renamed));
    }

    #[test]
    fn rejects_unknown_status_codes() {
        let error = parse_status(b"1 Z. N... 100644 100644 100644 aaaaaaa bbbbbbb README.md\0")
            .unwrap_err();
        assert_eq!(error, GitError::InvalidOutput);
    }

    #[test]
    fn git_status_command_is_read_only() {
        assert!(STATUS_ARGS.contains(&"status"));
        for mutation in ["add", "commit", "push", "reset", "restore", "checkout"] {
            assert!(!STATUS_ARGS.contains(&mutation));
        }
        assert!(STATUS_ARGS.contains(&"--no-optional-locks"));
    }

    #[test]
    fn mutation_paths_must_be_relative_normal_repository_paths() {
        assert_eq!(validate_repo_path("src/main.rs"), Ok("src/main.rs"));
        for path in [
            "",
            "/etc/passwd",
            "../secret",
            "src/../secret",
            "./README.md",
        ] {
            assert_eq!(validate_repo_path(path), Err(GitError::InvalidPath));
        }
    }

    #[test]
    fn stages_unstages_and_commits_only_explicit_changes() {
        let root = test_repository();
        fs::write(root.join("README.md"), "# Teste\n").unwrap();
        let service = GitService::new(&root).unwrap();

        let staged = service.stage("README.md").unwrap();
        assert_eq!(staged.changes[0].index, Some(ChangeKind::Added));
        let unstaged = service.unstage("README.md").unwrap();
        assert_eq!(unstaged.changes[0].worktree, Some(ChangeKind::Untracked));

        service.stage("README.md").unwrap();
        for (key, value) in [
            ("user.name", "Lyrnova Test"),
            ("user.email", "lyrnova-test@example.invalid"),
        ] {
            assert!(
                Command::new("git")
                    .args(["config", key, value])
                    .current_dir(&root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let committed = service.commit("Primeiro commit").unwrap();
        assert!(committed.changes.is_empty());
        assert!(committed.commit.is_some());

        fs::remove_dir_all(root).unwrap();
    }
}
