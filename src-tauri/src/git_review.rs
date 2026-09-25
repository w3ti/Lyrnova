use std::{
    fs,
    io::Read,
    process::{Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use super::{ChangeKind, GitError, GitService, validate_repo_path};
use crate::workspace::{WorkspaceError, WorkspaceService};

const MAX_DIFF_BYTES: usize = 1024 * 1024;
const MAX_DIFF_LINES: usize = 12_000;
const GIT_TIMEOUT: Duration = Duration::from_secs(15);
const REVIEW_TTL: Duration = Duration::from_secs(300);
const DIFF_OPTIONS: &[&str] = &[
    "--no-ext-diff",
    "--no-textconv",
    "--no-color",
    "--no-relative",
    "--src-prefix=a/",
    "--dst-prefix=b/",
    "--unified=3",
    "--find-renames",
    "--submodule=short",
    "--ignore-submodules=none",
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitDiffScope {
    Worktree,
    Index,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiff {
    pub patch: String,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitReview {
    pub token: String,
    pub branch: String,
    pub message: String,
    pub files: usize,
    pub diff: GitDiff,
}

#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    tree: String,
    parent: Option<String>,
    reference: String,
}

#[derive(Debug)]
pub(super) struct PendingCommit {
    token: String,
    message: String,
    snapshot: Snapshot,
    created: Instant,
}

pub(super) struct GitOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub truncated: bool,
}

impl GitService {
    // No pager, hooks, external diff/textconv, fsmonitor or inherited Git routing.
    // Drain both streams with bounded capture; never surface stderr (may contain secrets).
    pub(super) fn git_output(&self, args: &[&str], limit: usize) -> Result<GitOutput, GitError> {
        let mut command = Command::new("git");
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        command
            .args([
                "--no-pager",
                "--literal-pathspecs",
                "--no-optional-locks",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.quotePath=true",
                "-c",
                "diff.ignoreSubmodules=none",
                "-c",
                "diff.renames=true",
                "-c",
                "diff.algorithm=myers",
                "-c",
                "color.ui=false",
                "-c",
                "core.untrackedCache=false",
            ])
            .args(args)
            .current_dir(&self.root)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(|_| GitError::GitUnavailable)?;
        let stdout = child.stdout.take().ok_or(GitError::CommandFailed)?;
        let stderr = child.stderr.take().ok_or(GitError::CommandFailed)?;
        let out = thread::spawn(move || capture(stdout, limit));
        let err = thread::spawn(move || capture(stderr, 0));
        let started = Instant::now();
        let result = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if started.elapsed() < GIT_TIMEOUT => {
                    thread::sleep(Duration::from_millis(10))
                }
                Ok(None) => break Err(GitError::TimedOut),
                Err(_) => break Err(GitError::CommandFailed),
            }
        };
        if result.is_err() {
            #[cfg(unix)]
            // SAFETY: the child owns a new process group; a negative PID targets it.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
        }
        let captured = out.join().map_err(|_| GitError::CommandFailed)?;
        let _ = err.join();
        let status = result?;
        let (stdout, truncated) = captured?;
        Ok(GitOutput {
            status,
            stdout,
            truncated,
        })
    }

    fn git_text(&self, args: &[&str]) -> Result<String, GitError> {
        let result = self.git_output(args, 16 * 1024)?;
        if !result.status.success() || result.truncated {
            return Err(GitError::CommandFailed);
        }
        String::from_utf8(result.stdout)
            .map(|text| text.trim_end().to_owned())
            .map_err(|_| GitError::InvalidOutput)
    }

    fn diff_output(&self, args: &[&str]) -> Result<GitDiff, GitError> {
        let output = self.git_output(args, MAX_DIFF_BYTES)?;
        if !output.status.success() {
            return Err(GitError::CommandFailed);
        }
        let mut bytes = output.stdout;
        // A byte limit may bisect UTF-8; only discard an incomplete final code point.
        if output.truncated
            && let Err(error) = std::str::from_utf8(&bytes)
            && error.error_len().is_none()
        {
            bytes.truncate(error.valid_up_to());
        }
        let patch = String::from_utf8(bytes).map_err(|_| GitError::NonTextDiff)?;
        if patch.contains('\0') {
            return Err(GitError::NonTextDiff);
        }
        Ok(limit_patch(patch, output.truncated))
    }

    pub fn diff(&self, path: &str, scope: GitDiffScope) -> Result<GitDiff, GitError> {
        validate_repo_path(path)?;
        let status = self.status()?;
        let change = status
            .changes
            .iter()
            .find(|change| {
                change.path == path
                    && match scope {
                        GitDiffScope::Index => change.index.is_some(),
                        GitDiffScope::Worktree => change.worktree.is_some(),
                    }
            })
            .ok_or(GitError::ChangeNotFound)?;
        if scope == GitDiffScope::Worktree && change.worktree == Some(ChangeKind::Untracked) {
            let workspace = WorkspaceService::new(&self.root).map_err(|_| GitError::InvalidPath)?;
            return match workspace.read(path) {
                Ok(document) => {
                    let mut patch = format!(
                        "Arquivo novo: {path:?}\n--- /dev/null\n+++ {path:?}\n@@ -0,0 +1,{} @@\n",
                        document.content.lines().count()
                    );
                    for line in document.content.split_inclusive('\n') {
                        patch.push('+');
                        patch.push_str(line);
                    }
                    if !document.content.is_empty() && !document.content.ends_with('\n') {
                        patch.push_str("\n\\ No newline at end of file\n");
                    }
                    Ok(limit_patch(patch, false))
                }
                Err(WorkspaceError::BinaryFile) => Ok(GitDiff {
                    patch: "Arquivo binário novo; conteúdo não exibido.".into(),
                    truncated: false,
                }),
                Err(WorkspaceError::DocumentTooLarge) => Err(GitError::DiffTooLarge),
                Err(WorkspaceError::NotUtf8) => Err(GitError::NonTextDiff),
                Err(_) => Err(GitError::InvalidPath),
            };
        }
        // Git reads symlink objects as link text; it must not traverse a replaced ancestor.
        if scope == GitDiffScope::Worktree {
            self.validate_ancestors(path)?;
        }
        let mut args = vec!["diff"];
        args.extend_from_slice(DIFF_OPTIONS);
        if scope == GitDiffScope::Index {
            args.push("--cached");
        }
        args.extend(["--", path]);
        if scope == GitDiffScope::Index
            && let Some(previous) = &change.previous_path
        {
            validate_repo_path(previous)?;
            args.push(previous);
        }
        self.diff_output(&args)
    }

    fn validate_ancestors(&self, path: &str) -> Result<(), GitError> {
        let mut current = self.root.clone();
        let parts: Vec<_> = std::path::Path::new(path).components().collect();
        for part in parts.iter().take(parts.len().saturating_sub(1)) {
            current.push(part);
            match fs::symlink_metadata(&current) {
                Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                _ => return Err(GitError::InvalidPath),
            }
        }
        Ok(())
    }

    fn snapshot(&self) -> Result<Snapshot, GitError> {
        let git_dir = self.git_text(&["rev-parse", "--absolute-git-dir"])?;
        for marker in [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "rebase-merge",
            "rebase-apply",
            "sequencer",
        ] {
            if std::path::Path::new(&git_dir).join(marker).exists() {
                return Err(GitError::OperationInProgress);
            }
        }
        let reference = self.git_output(&["symbolic-ref", "--quiet", "HEAD"], 4096)?;
        let reference = if reference.status.success() && !reference.truncated {
            String::from_utf8(reference.stdout)
                .map_err(|_| GitError::InvalidOutput)?
                .trim()
                .to_owned()
        } else if reference.status.code() == Some(1) {
            "HEAD".into()
        } else {
            return Err(GitError::CommandFailed);
        };
        let head = self.git_output(&["rev-parse", "--verify", "--quiet", "HEAD"], 256)?;
        let parent = if head.status.success() {
            Some(parse_oid(&head.stdout)?)
        } else if head.status.code() == Some(1) && reference != "HEAD" {
            None
        } else {
            return Err(GitError::CommandFailed);
        };
        let tree = self.git_text(&["write-tree"])?;
        parse_oid(tree.as_bytes())?;
        Ok(Snapshot {
            tree,
            parent,
            reference,
        })
    }

    pub fn review_commit(&self, message: &str) -> Result<GitCommitReview, GitError> {
        let mut pending = self
            .pending_commit
            .lock()
            .map_err(|_| GitError::CommandFailed)?;
        *pending = None;
        let message = message.trim();
        if message.is_empty() || message.len() > 16 * 1024 || message.contains('\0') {
            return Err(GitError::InvalidMessage);
        }
        let status = self.status()?;
        let files = status
            .changes
            .iter()
            .filter(|change| change.index.is_some())
            .count();
        if files == 0 {
            return Err(GitError::ChangeNotFound);
        }
        if status
            .changes
            .iter()
            .any(|change| change.index == Some(ChangeKind::Conflicted))
        {
            return Err(GitError::OperationInProgress);
        }
        let snapshot = self.snapshot()?;
        let mut args = vec!["diff"];
        args.extend_from_slice(DIFF_OPTIONS);
        // For an unborn branch compare against an empty tree generated for this repository's object format.
        let empty_tree;
        if let Some(parent) = &snapshot.parent {
            args.push(parent);
        } else {
            empty_tree = self.git_text(&["hash-object", "-t", "tree", "-w", "--stdin"])?;
            args.push(&empty_tree);
        }
        args.push(&snapshot.tree);
        let diff = self.diff_output(&args)?;
        if diff.truncated {
            return Err(GitError::DiffTooLarge);
        }
        args.extend(["--name-only", "-z"]);
        let names = self.git_output(&args, MAX_DIFF_BYTES)?;
        if !names.status.success() || names.truncated {
            return Err(GitError::DiffTooLarge);
        }
        let files = names
            .stdout
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            .count();
        if files == 0 {
            return Err(GitError::ReviewChanged);
        }
        if self.snapshot()? != snapshot {
            return Err(GitError::ReviewChanged);
        }
        let token = uuid::Uuid::new_v4().to_string();
        let review = GitCommitReview {
            token: token.clone(),
            branch: snapshot
                .reference
                .strip_prefix("refs/heads/")
                .unwrap_or(&snapshot.reference)
                .into(),
            message: message.into(),
            files,
            diff,
        };
        *pending = Some(PendingCommit {
            token,
            message: message.into(),
            snapshot,
            created: Instant::now(),
        });
        Ok(review)
    }

    pub fn discard_commit_review(&self, token: &str) {
        if let Ok(mut pending) = self.pending_commit.lock()
            && pending.as_ref().is_some_and(|review| review.token == token)
        {
            *pending = None;
        }
    }

    pub fn commit(&self, token: &str) -> Result<String, GitError> {
        let mut pending = self
            .pending_commit
            .lock()
            .map_err(|_| GitError::CommandFailed)?;
        if !pending.as_ref().is_some_and(|review| review.token == token) {
            return Err(GitError::ReviewExpired);
        }
        let review = pending.take().ok_or(GitError::ReviewExpired)?;
        if review.created.elapsed() >= REVIEW_TTL {
            return Err(GitError::ReviewExpired);
        }
        if self.snapshot()? != review.snapshot {
            return Err(GitError::ReviewChanged);
        }
        let mut args = vec!["commit-tree", "--no-gpg-sign", &review.snapshot.tree];
        if let Some(parent) = &review.snapshot.parent {
            args.extend(["-p", parent]);
        }
        args.extend(["-m", &review.message]);
        let commit = self.git_text(&args)?;
        parse_oid(commit.as_bytes())?;
        if self.snapshot()? != review.snapshot {
            return Err(GitError::ReviewChanged);
        }
        // Commit the immutable reviewed tree, then CAS the exact reviewed reference.
        // Concurrent index changes can never enter this commit or be overwritten.
        let zero = "0".repeat(review.snapshot.tree.len());
        let old = review.snapshot.parent.as_deref().unwrap_or(&zero);
        let output = self.git_output(
            &[
                "update-ref",
                "--no-deref",
                "-m",
                "commit: Lyrnova",
                &review.snapshot.reference,
                &commit,
                old,
            ],
            4096,
        )?;
        if !output.status.success() {
            return Err(GitError::ReviewChanged);
        }
        // A later status failure must not report a successfully published commit as failed.
        Ok(commit)
    }
}

fn parse_oid(bytes: &[u8]) -> Result<String, GitError> {
    let oid = std::str::from_utf8(bytes)
        .map_err(|_| GitError::InvalidOutput)?
        .trim();
    if ![40, 64].contains(&oid.len()) || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(GitError::InvalidOutput);
    }
    Ok(oid.into())
}

fn capture(mut reader: impl Read, limit: usize) -> Result<(Vec<u8>, bool), GitError> {
    let mut bytes = Vec::new();
    let mut truncated = false;
    let mut buffer = [0; 8192];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| GitError::CommandFailed)?;
        if count == 0 {
            break;
        }
        let keep = count.min(limit.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..keep]);
        truncated |= keep < count;
    }
    Ok((bytes, truncated))
}

fn limit_patch(mut patch: String, mut truncated: bool) -> GitDiff {
    let line_limit = patch
        .match_indices('\n')
        .nth(MAX_DIFF_LINES - 1)
        .map(|(index, _)| index + 1);
    let mut limit = line_limit.unwrap_or(patch.len()).min(MAX_DIFF_BYTES);
    if limit < patch.len() {
        while !patch.is_char_boundary(limit) {
            limit -= 1;
        }
        patch.truncate(limit);
        truncated = true;
    }
    GitDiff { patch, truncated }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Repo {
        service: GitService,
    }
    impl Repo {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("lyrnova-git-review-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            assert!(
                Command::new("git")
                    .args(["init", "--quiet", "--initial-branch=main"])
                    .current_dir(&root)
                    .status()
                    .unwrap()
                    .success()
            );
            let repo = Self {
                service: GitService::new(root).unwrap(),
            };
            repo.git(&["config", "user.name", "Lyrnova Test"]);
            repo.git(&["config", "user.email", "test@example.invalid"]);
            repo
        }
        fn git(&self, args: &[&str]) -> String {
            self.service.git_text(args).unwrap()
        }
        fn write(&self, name: &str, text: impl AsRef<[u8]>) {
            fs::write(self.service.root.join(name), text).unwrap();
        }
        fn initial(&self) {
            self.write("file.txt", "base\n");
            self.service.stage("file.txt").unwrap();
            self.commit("Initial");
        }
        fn commit(&self, message: &str) {
            let review = self.service.review_commit(message).unwrap();
            self.service.commit(&review.token).unwrap();
        }
    }
    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.service.root);
        }
    }

    #[test]
    fn initial_commit_review_and_commit_preserve_unstaged_content() {
        let repo = Repo::new();
        repo.write("file.txt", "staged\n");
        repo.service.stage("file.txt").unwrap();
        repo.write("file.txt", "unstaged\n");
        let staged = repo.service.diff("file.txt", GitDiffScope::Index).unwrap();
        let worktree = repo
            .service
            .diff("file.txt", GitDiffScope::Worktree)
            .unwrap();
        assert!(staged.patch.contains("+staged") && !staged.patch.contains("unstaged"));
        assert!(worktree.patch.contains("-staged") && worktree.patch.contains("+unstaged"));
        let review = repo.service.review_commit("First commit").unwrap();
        assert_eq!(review.files, 1);
        assert_eq!(review.branch, "main");
        assert!(review.diff.patch.contains("+staged"));
        repo.service.commit(&review.token).unwrap();
        let status = repo.service.status().unwrap();
        assert_eq!(status.changes[0].worktree, Some(ChangeKind::Modified));
        assert_eq!(repo.git(&["show", "HEAD:file.txt"]), "staged");
        assert_eq!(
            repo.service.commit(&review.token),
            Err(GitError::ReviewExpired)
        );
    }

    #[test]
    fn changed_index_with_same_status_requires_new_review() {
        let repo = Repo::new();
        repo.initial();
        repo.write("file.txt", "reviewed\n");
        repo.service.stage("file.txt").unwrap();
        let review = repo.service.review_commit("Change").unwrap();
        repo.write("file.txt", "not reviewed\n");
        repo.service.stage("file.txt").unwrap();
        assert_eq!(
            repo.service.commit(&review.token),
            Err(GitError::ReviewChanged)
        );
        assert_eq!(repo.git(&["show", "HEAD:file.txt"]), "base");
        assert_eq!(repo.git(&["show", ":file.txt"]), "not reviewed");
    }

    #[test]
    fn branch_changes_and_expired_or_canceled_reviews_do_not_commit() {
        let repo = Repo::new();
        repo.initial();
        repo.write("file.txt", "next\n");
        repo.service.stage("file.txt").unwrap();
        let review = repo.service.review_commit("Change").unwrap();
        repo.git(&["switch", "-c", "other"]);
        assert_eq!(
            repo.service.commit(&review.token),
            Err(GitError::ReviewChanged)
        );
        let review = repo.service.review_commit("Change").unwrap();
        repo.service.discard_commit_review(&review.token);
        assert_eq!(
            repo.service.commit(&review.token),
            Err(GitError::ReviewExpired)
        );
        let review = repo.service.review_commit("Change").unwrap();
        repo.service
            .pending_commit
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .created = Instant::now() - REVIEW_TTL;
        assert_eq!(
            repo.service.commit(&review.token),
            Err(GitError::ReviewExpired)
        );
        assert_eq!(repo.git(&["rev-list", "--count", "HEAD"]), "1");
    }

    #[test]
    fn review_is_bound_to_its_repository_and_superseded_tokens_are_rejected() {
        let repo = Repo::new();
        repo.initial();
        repo.write("file.txt", "next\n");
        repo.service.stage("file.txt").unwrap();
        let old = repo.service.review_commit("Old").unwrap();
        let latest = repo.service.review_commit("Latest").unwrap();
        repo.service.discard_commit_review(&old.token);
        assert_eq!(
            repo.service.commit(&old.token),
            Err(GitError::ReviewExpired)
        );
        assert_eq!(
            Repo::new().service.commit(&latest.token),
            Err(GitError::ReviewExpired)
        );
        repo.service.clone().commit(&latest.token).unwrap();
        assert_eq!(repo.git(&["log", "-1", "--format=%s"]), "Latest");
    }

    #[test]
    fn literal_unicode_and_pathspec_names_never_select_other_files() {
        let repo = Repo::new();
        let name = "arquivo [*] ç.txt";
        repo.write(name, "chosen\n");
        repo.write("arquivo x ç.txt", "other\n");
        repo.service.stage(name).unwrap();
        let review = repo.service.review_commit("Special path").unwrap();
        assert_eq!(review.files, 1);
        assert!(review.diff.patch.contains("+chosen") && !review.diff.patch.contains("+other"));
        for path in ["../secret", "/etc/passwd", ":(glob)*", "missing"] {
            assert!(repo.service.diff(path, GitDiffScope::Worktree).is_err());
        }
        repo.commit("First");
        repo.write(name, "new chosen\n");
        repo.write("arquivo x ç.txt", "new other\n");
        let diff = repo.service.diff(name, GitDiffScope::Worktree).unwrap();
        assert!(diff.patch.contains("+new chosen") && !diff.patch.contains("other"));
    }

    #[test]
    fn rename_review_includes_both_paths_and_unstage_restores_both() {
        let repo = Repo::new();
        repo.initial();
        repo.git(&["mv", "file.txt", "renamed.txt"]);
        let diff = repo
            .service
            .diff("renamed.txt", GitDiffScope::Index)
            .unwrap();
        assert!(diff.patch.contains("rename from file.txt"));
        assert!(diff.patch.contains("rename to renamed.txt"));
        let status = repo.service.unstage("renamed.txt").unwrap();
        assert!(status.changes.iter().all(|change| change.index.is_none()));
        let diff = repo
            .service
            .diff("file.txt", GitDiffScope::Worktree)
            .unwrap();
        assert!(diff.patch.contains("deleted file mode") && diff.patch.contains("-base"));
        repo.service.stage("file.txt").unwrap();
        assert!(
            repo.service
                .review_commit("Delete")
                .unwrap()
                .diff
                .patch
                .contains("-base")
        );
    }

    #[test]
    fn binary_untracked_and_empty_files_have_honest_previews() {
        let repo = Repo::new();
        repo.write("binary.bin", b"\0SECRET\xff");
        let diff = repo
            .service
            .diff("binary.bin", GitDiffScope::Worktree)
            .unwrap();
        assert!(diff.patch.contains("binário") && !diff.patch.contains("SECRET"));
        repo.service.stage("binary.bin").unwrap();
        let diff = repo
            .service
            .diff("binary.bin", GitDiffScope::Index)
            .unwrap();
        assert!(diff.patch.contains("Binary files") && !diff.patch.contains("SECRET"));
        repo.write("empty", "");
        assert!(
            repo.service
                .diff("empty", GitDiffScope::Worktree)
                .unwrap()
                .patch
                .contains("Arquivo novo")
        );
        repo.write("no-newline", "<script>alert(1)</script>");
        let diff = repo
            .service
            .diff("no-newline", GitDiffScope::Worktree)
            .unwrap();
        assert!(diff.patch.contains("+<script>") && diff.patch.contains("No newline"));
    }

    #[test]
    fn diff_drivers_textconv_and_hooks_are_never_executed_for_review_or_commit() {
        let repo = Repo::new();
        repo.initial();
        repo.git(&["config", "diff.external", "false"]);
        repo.git(&["config", "diff.evil.textconv", "false"]);
        repo.git(&["config", "diff.evil.command", "false"]);
        repo.write(".gitattributes", "file.txt diff=evil\n");
        repo.write("file.txt", "safe\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for hook in ["pre-commit", "reference-transaction"] {
                let path = repo.service.root.join(".git/hooks").join(hook);
                fs::write(&path, "#!/bin/sh\nexit 1\n").unwrap();
                fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
        let diff = repo
            .service
            .diff("file.txt", GitDiffScope::Worktree)
            .unwrap();
        assert!(diff.patch.contains("+safe"));
        repo.service.stage("file.txt").unwrap();
        repo.commit("Safe");
        assert_eq!(repo.git(&["show", "HEAD:file.txt"]), "safe");
    }

    #[test]
    fn oversized_previews_are_partial_and_cannot_authorize_a_commit() {
        let repo = Repo::new();
        repo.write("large.txt", "line\n".repeat(MAX_DIFF_LINES + 1));
        repo.service.stage("large.txt").unwrap();
        assert!(
            repo.service
                .diff("large.txt", GitDiffScope::Index)
                .unwrap()
                .truncated
        );
        assert!(
            repo.service
                .diff("large.txt", GitDiffScope::Index)
                .unwrap()
                .patch
                .lines()
                .count()
                <= MAX_DIFF_LINES
        );
        assert_eq!(
            repo.service.review_commit("Large").unwrap_err(),
            GitError::DiffTooLarge
        );
        assert!(repo.service.pending_commit.lock().unwrap().is_none());
        let limited = limit_patch("é".repeat(MAX_DIFF_BYTES), false);
        assert!(limited.truncated && limited.patch.len() <= MAX_DIFF_BYTES);
        assert_eq!(
            capture(&b"123456789"[..], 3).unwrap(),
            (b"123".to_vec(), true)
        );
    }

    #[test]
    fn merge_and_invalid_text_reviews_fail_closed() {
        let repo = Repo::new();
        repo.initial();
        repo.write("file.txt", "next\n");
        repo.service.stage("file.txt").unwrap();
        repo.write(".git/MERGE_HEAD", repo.git(&["rev-parse", "HEAD"]));
        assert_eq!(
            repo.service.review_commit("Merge").unwrap_err(),
            GitError::OperationInProgress
        );
        fs::remove_file(repo.service.root.join(".git/MERGE_HEAD")).unwrap();
        repo.write("file.txt", b"invalid\xff\n");
        repo.service.stage("file.txt").unwrap();
        assert_eq!(
            repo.service.review_commit("Invalid UTF8").unwrap_err(),
            GitError::NonTextDiff
        );
    }

    #[test]
    fn actual_conflicts_are_visible_but_cannot_be_committed_by_the_simple_flow() {
        let repo = Repo::new();
        repo.initial();
        repo.git(&["switch", "-c", "other"]);
        repo.write("file.txt", "other\n");
        repo.service.stage("file.txt").unwrap();
        repo.commit("Other");
        repo.git(&["switch", "main"]);
        repo.write("file.txt", "main\n");
        repo.service.stage("file.txt").unwrap();
        repo.commit("Main");
        assert!(
            !repo
                .service
                .git_output(&["merge", "other"], 4096)
                .unwrap()
                .status
                .success()
        );
        let diff = repo
            .service
            .diff("file.txt", GitDiffScope::Worktree)
            .unwrap();
        assert!(diff.patch.contains("<<<<<<<") && diff.patch.contains("other"));
        assert_eq!(
            repo.service.review_commit("Conflict").unwrap_err(),
            GitError::OperationInProgress
        );
    }

    #[test]
    fn metadata_only_and_submodule_changes_are_present_in_review() {
        let repo = Repo::new();
        repo.initial();
        repo.git(&["update-index", "--chmod=+x", "file.txt"]);
        let commit = repo.git(&["rev-parse", "HEAD"]);
        repo.git(&[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{commit},module"),
        ]);
        let review = repo.service.review_commit("Metadata").unwrap();
        assert_eq!(review.files, 2);
        assert!(review.diff.patch.contains("new mode 100755"));
        assert!(review.diff.patch.contains("Subproject commit"));
        repo.service.commit(&review.token).unwrap();
        assert!(repo.git(&["ls-tree", "HEAD", "module"]).contains("160000"));
    }

    #[test]
    fn a_different_head_invalidates_review_even_when_the_index_matches() {
        let repo = Repo::new();
        repo.initial();
        repo.write("file.txt", "next\n");
        repo.service.stage("file.txt").unwrap();
        let review = repo.service.review_commit("Reviewed").unwrap();
        let tree = repo.git(&["rev-parse", "HEAD^{tree}"]);
        let parent = repo.git(&["rev-parse", "HEAD"]);
        let external = repo.git(&["commit-tree", &tree, "-p", &parent, "-m", "External"]);
        repo.git(&["update-ref", "HEAD", &external, &parent]);
        assert_eq!(
            repo.service.commit(&review.token),
            Err(GitError::ReviewChanged)
        );
        assert_eq!(repo.git(&["rev-parse", "HEAD"]), external);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_never_expose_external_file_contents() {
        use std::os::unix::fs::symlink;
        let repo = Repo::new();
        symlink("/etc/passwd", repo.service.root.join("link")).unwrap();
        assert!(repo.service.diff("link", GitDiffScope::Worktree).is_err());
        repo.git(&["add", "link"]);
        let diff = repo.service.diff("link", GitDiffScope::Index).unwrap();
        assert!(diff.patch.contains("+/etc/passwd") && !diff.patch.contains("root:"));
        fs::create_dir(repo.service.root.join("dir")).unwrap();
        repo.write("dir/file", "local\n");
        repo.git(&["add", "dir/file"]);
        repo.commit("Links");
        fs::remove_dir_all(repo.service.root.join("dir")).unwrap();
        symlink("/etc", repo.service.root.join("dir")).unwrap();
        assert_eq!(
            repo.service
                .diff("dir/file", GitDiffScope::Worktree)
                .unwrap_err(),
            GitError::InvalidPath
        );
    }
}
