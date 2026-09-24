//! Staging and committing in the open folder's repository — the Changes
//! panel's four operations, over `git2`.
//!
//! The repository is discovered from the folder, so a folder inside a
//! repository works. Paths go both ways relative to the repository root.

use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path};

use git2::{Diff, DiffOptions, ErrorCode, Repository, Signature};

use crate::domain::git_changes::{ChangeTotals, ChangedFile, CommitSummary, GitChangesError, GitHistory, WorkingChanges};

pub fn changes(root: &Path) -> Result<WorkingChanges, GitChangesError> {
    let repo = open(root)?;
    let head = head_tree(&repo)?;
    let index = repo.index().map_err(git)?;

    let staged = repo.diff_tree_to_index(head.as_ref(), Some(&index), None).map_err(git)?;
    // Untracked content implies untracked files; a new folder is listed file
    // by file rather than as one entry.
    let mut options = DiffOptions::new();
    options.show_untracked_content(true).recurse_untracked_dirs(true);
    let unstaged = repo.diff_index_to_workdir(Some(&index), Some(&mut options)).map_err(git)?;

    Ok(WorkingChanges {
        staged: files(&staged)?,
        unstaged: files(&unstaged)?,
    })
}

/// What the working tree holds against HEAD, the index in between — so an
/// edit staged and then edited again counts once — new files included. Only
/// the open folder: a subfolder of a repository counts its own changes.
pub fn totals(root: &Path) -> Result<ChangeTotals, GitChangesError> {
    let repo = open(root)?;
    let head = head_tree(&repo)?;
    let mut options = DiffOptions::new();
    options.show_untracked_content(true).recurse_untracked_dirs(true);
    let workdir = repo.workdir().ok_or(GitChangesError::NotARepository)?;
    let inside = root.canonicalize().ok().and_then(|root| {
        let workdir = workdir.canonicalize().ok()?;
        Some(root.strip_prefix(workdir).ok()?.to_path_buf())
    });
    if let Some(folder) = inside.filter(|folder| !folder.as_os_str().is_empty()) {
        options.pathspec(folder);
    }
    let diff = repo.diff_tree_to_workdir_with_index(head.as_ref(), Some(&mut options)).map_err(git)?;
    let stats = diff.stats().map_err(git)?;
    Ok(ChangeTotals { files: stats.files_changed(), add: stats.insertions(), del: stats.deletions() })
}

/// A path that is gone from disk stages its deletion.
pub fn stage(root: &Path, paths: &[String]) -> Result<(), GitChangesError> {
    let repo = open(root)?;
    let workdir = repo.workdir().ok_or(GitChangesError::NotARepository)?.to_path_buf();
    let mut index = repo.index().map_err(git)?;
    for path in paths {
        let rel = relative(path)?;
        if workdir.join(rel).exists() {
            index.add_path(rel).map_err(git)?;
        } else {
            index.remove_path(rel).map_err(git)?;
        }
    }
    index.write().map_err(git)
}

pub fn unstage(root: &Path, paths: &[String]) -> Result<(), GitChangesError> {
    let repo = open(root)?;
    let rels = paths.iter().map(|p| relative(p)).collect::<Result<Vec<_>, _>>()?;
    // Bound before returning: the head borrows `repo`.
    let done = match repo.head() {
        Ok(head) => {
            let commit = head.peel_to_commit().map_err(git)?;
            repo.reset_default(Some(commit.as_object()), rels).map_err(git)
        }
        // No commit yet: unstaging is taking the file out of the index.
        Err(e) if e.code() == ErrorCode::UnbornBranch => {
            let mut index = repo.index().map_err(git)?;
            for rel in rels {
                index.remove_path(rel).map_err(git)?;
            }
            index.write().map_err(git)
        }
        Err(e) => Err(git(e)),
    };
    done
}

/// Commits the index on HEAD; returns the new commit's short id.
pub fn commit(root: &Path, message: &str) -> Result<String, GitChangesError> {
    let message = message.trim();
    if message.is_empty() {
        return Err(GitChangesError::EmptyMessage);
    }
    let repo = open(root)?;
    let head = head_tree(&repo)?;
    let mut index = repo.index().map_err(git)?;
    let staged = repo.diff_tree_to_index(head.as_ref(), Some(&index), None).map_err(git)?;
    if staged.deltas().len() == 0 {
        return Err(GitChangesError::NothingStaged);
    }

    let tree = repo.find_tree(index.write_tree().map_err(git)?).map_err(git)?;
    let sig = signature(&repo)?;
    let parent = match repo.head() {
        Ok(head) => Some(head.peel_to_commit().map_err(git)?),
        Err(e) if e.code() == ErrorCode::UnbornBranch => None,
        Err(e) => return Err(git(e)),
    };
    let parents: Vec<&git2::Commit> = parent.iter().collect();
    let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents).map_err(git)?;
    Ok(oid.to_string().chars().take(7).collect())
}

/// Where HEAD is and up to `limit` commits reachable from it, newest first;
/// no commits before the first one.
pub fn history(root: &Path, limit: usize) -> Result<GitHistory, GitChangesError> {
    let repo = open(root)?;
    let branch = crate::infra::git_head::current_branch(root);
    let mut history = GitHistory { branch, upstream: None, ahead: 0, behind: 0, commits: Vec::new(), more: false };
    if head_tree(&repo)?.is_none() {
        return Ok(history);
    }
    let head = repo.head().map_err(git)?;
    let head_id = head.target();
    let checked_out = if head.is_branch() { head.name().ok().map(String::from) } else { None };
    if head.is_branch() {
        if let Ok(upstream) = git2::Branch::wrap(head).upstream() {
            history.upstream = upstream.name().ok().flatten().map(String::from);
            if let (Some(local), Some(remote)) = (head_id, upstream.get().target()) {
                (history.ahead, history.behind) = repo.graph_ahead_behind(local, remote).map_err(git)?;
            }
        }
    }

    // Every branch and tag by the commit it points at; a remote's HEAD only
    // repeats the branch it names.
    let mut refs: HashMap<git2::Oid, Vec<String>> = HashMap::new();
    for reference in repo.references().map_err(git)?.flatten() {
        let name = reference.name().unwrap_or_default();
        if reference.is_remote() && name.ends_with("/HEAD") || checked_out.as_deref() == Some(name) {
            continue;
        }
        let (Ok(commit), Ok(short)) = (reference.peel_to_commit(), reference.shorthand()) else { continue };
        if reference.is_branch() || reference.is_remote() || reference.is_tag() {
            refs.entry(commit.id()).or_default().push(short.to_string());
        }
    }

    let mut walk = repo.revwalk().map_err(git)?;
    walk.push_head().map_err(git)?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME).map_err(git)?;
    for id in walk {
        if history.commits.len() == limit {
            history.more = true;
            break;
        }
        let commit = repo.find_commit(id.map_err(git)?).map_err(git)?;
        let author = commit.author().name().unwrap_or_default().to_string();
        let mut on_it = refs.remove(&commit.id()).unwrap_or_default();
        on_it.sort();
        history.commits.push(CommitSummary {
            id: commit.id().to_string().chars().take(7).collect(),
            summary: commit.summary().map_err(git)?.unwrap_or_default().to_string(),
            author,
            time: commit.time().seconds(),
            head: Some(commit.id()) == head_id,
            refs: on_it,
        });
    }
    Ok(history)
}

/// The `.git` directory of the repository `root` is in — what staging and
/// committing write, and the tree's watcher leaves out. `None` outside one.
pub fn git_dir(root: &Path) -> Option<std::path::PathBuf> {
    Repository::discover(root).ok().map(|repo| repo.path().to_path_buf())
}

fn open(root: &Path) -> Result<Repository, GitChangesError> {
    Repository::discover(root).map_err(|_| GitChangesError::NotARepository)
}

fn head_tree(repo: &Repository) -> Result<Option<git2::Tree<'_>>, GitChangesError> {
    match repo.head() {
        Ok(head) => Ok(Some(head.peel_to_tree().map_err(git)?)),
        Err(e) if e.code() == ErrorCode::UnbornBranch || e.code() == ErrorCode::NotFound => Ok(None),
        Err(e) => Err(git(e)),
    }
}

/// Each file of a diff with its added and removed lines; a binary file counts
/// none. Sorted by path.
fn files(diff: &Diff) -> Result<Vec<ChangedFile>, GitChangesError> {
    let mut counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for delta in diff.deltas() {
        if let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path()) {
            counts.entry(path.to_string_lossy().into_owned()).or_default();
        }
    }
    diff.foreach(
        &mut |_, _| true,
        None,
        None,
        Some(&mut |delta, _, line| {
            let path = delta.new_file().path().or_else(|| delta.old_file().path());
            if let Some(entry) = path.and_then(|p| counts.get_mut(&*p.to_string_lossy())) {
                match line.origin() {
                    '+' => entry.0 += 1,
                    '-' => entry.1 += 1,
                    _ => {}
                }
            }
            true
        }),
    )
    .map_err(git)?;
    Ok(counts.into_iter().map(|(path, (add, del))| ChangedFile { path, add, del }).collect())
}

/// A repository-relative path, refusing anything that would leave it — the
/// frontend sends these back, so they are checked rather than trusted.
fn relative(path: &str) -> Result<&Path, GitChangesError> {
    let p = Path::new(path);
    if path.is_empty() || !p.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(GitChangesError::InvalidPath(path.to_string()));
    }
    Ok(p)
}

fn signature(repo: &Repository) -> Result<Signature<'static>, GitChangesError> {
    let sig = repo.signature().map_err(|_| GitChangesError::MissingIdentity)?;
    let (Some(name), Some(email)) = (sig.name().ok(), sig.email().ok()) else {
        return Err(GitChangesError::MissingIdentity);
    };
    if name.trim().is_empty() || email.trim().is_empty() {
        return Err(GitChangesError::MissingIdentity);
    }
    Signature::now(name, email).map_err(git)
}

fn git(err: git2::Error) -> GitChangesError {
    GitChangesError::Git(err.message().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;
    use std::fs;

    fn repo() -> (std::path::PathBuf, Repository) {
        let dir = temp_dir("git-changes");
        let repo = Repository::init(&dir).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();
        (dir, repo)
    }

    fn file(path: &str, add: usize, del: usize) -> ChangedFile {
        ChangedFile { path: path.into(), add, del }
    }

    #[test]
    fn a_new_file_is_unstaged_until_staged_then_committed() {
        let (dir, _repo) = repo();
        fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        fs::create_dir(dir.join("new")).unwrap();
        fs::write(dir.join("new/b.txt"), "b\n").unwrap();

        let before = changes(&dir).unwrap();
        assert_eq!(before.unstaged, vec![file("a.txt", 2, 0), file("new/b.txt", 1, 0)]);
        assert!(before.staged.is_empty());

        stage(&dir, &["a.txt".into(), "new/b.txt".into()]).unwrap();
        let staged = changes(&dir).unwrap();
        assert_eq!(staged.staged, vec![file("a.txt", 2, 0), file("new/b.txt", 1, 0)]);
        assert!(staged.unstaged.is_empty());

        let empty = history(&dir, 10).unwrap();
        assert!(empty.commits.is_empty() && !empty.more);
        let id = commit(&dir, "  first\n").unwrap();
        assert_eq!(id.len(), 7);
        assert_eq!(changes(&dir).unwrap(), WorkingChanges { staged: vec![], unstaged: vec![] });
    }

    #[test]
    fn counts_each_side_and_unstages_back() {
        let (dir, _repo) = repo();
        fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        stage(&dir, &["a.txt".into()]).unwrap();
        commit(&dir, "first").unwrap();

        fs::write(dir.join("a.txt"), "one\n2\nthree\n").unwrap();
        stage(&dir, &["a.txt".into()]).unwrap();
        fs::write(dir.join("a.txt"), "one\n2\nthree\nfour\n").unwrap();
        let both = changes(&dir).unwrap();
        assert_eq!(both.staged, vec![file("a.txt", 2, 1)]);
        assert_eq!(both.unstaged, vec![file("a.txt", 1, 0)]);

        unstage(&dir, &["a.txt".into()]).unwrap();
        let back = changes(&dir).unwrap();
        assert!(back.staged.is_empty());
        assert_eq!(back.unstaged, vec![file("a.txt", 3, 1)]);
    }

    #[test]
    fn totals_count_staged_and_unstaged_once_new_files_too_within_the_folder() {
        let (dir, _repo) = repo();
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        fs::write(dir.join("sub/b.txt"), "b\n").unwrap();
        stage(&dir, &["a.txt".into(), "sub/b.txt".into()]).unwrap();
        commit(&dir, "first").unwrap();
        assert_eq!(totals(&dir).unwrap(), ChangeTotals { files: 0, add: 0, del: 0 });

        // Staged, then edited again: one change against HEAD, not two.
        fs::write(dir.join("a.txt"), "one\n2\n").unwrap();
        stage(&dir, &["a.txt".into()]).unwrap();
        fs::write(dir.join("a.txt"), "one\n2\n3\n").unwrap();
        fs::write(dir.join("sub/new.txt"), "x\ny\n").unwrap();
        fs::remove_file(dir.join("sub/b.txt")).unwrap();
        assert_eq!(totals(&dir).unwrap(), ChangeTotals { files: 3, add: 4, del: 2 });
        // The subfolder alone: its new file and its deletion.
        assert_eq!(totals(&dir.join("sub")).unwrap(), ChangeTotals { files: 2, add: 2, del: 1 });

        // An ignored file added anyway goes into the next commit, so it counts:
        // on disk alone it is ignored, and only the index says otherwise.
        fs::write(dir.join(".gitignore"), "*.log\n").unwrap();
        fs::write(dir.join("keep.log"), "l\n").unwrap();
        stage(&dir, &["keep.log".into()]).unwrap();
        assert_eq!(totals(&dir).unwrap(), ChangeTotals { files: 5, add: 6, del: 2 });
    }

    #[test]
    fn a_deleted_file_stages_its_deletion() {
        let (dir, _repo) = repo();
        fs::write(dir.join("a.txt"), "one\n").unwrap();
        stage(&dir, &["a.txt".into()]).unwrap();
        commit(&dir, "first").unwrap();
        fs::remove_file(dir.join("a.txt")).unwrap();

        stage(&dir, &["a.txt".into()]).unwrap();
        assert_eq!(changes(&dir).unwrap().staged, vec![file("a.txt", 0, 1)]);
    }

    #[test]
    fn unstaging_before_the_first_commit_takes_it_out_of_the_index() {
        let (dir, _repo) = repo();
        fs::write(dir.join("a.txt"), "one\n").unwrap();
        stage(&dir, &["a.txt".into()]).unwrap();
        unstage(&dir, &["a.txt".into()]).unwrap();
        let after = changes(&dir).unwrap();
        assert!(after.staged.is_empty());
        assert_eq!(after.unstaged, vec![file("a.txt", 1, 0)]);
    }

    fn commit_file(dir: &Path, n: usize, message: &str) -> String {
        fs::write(dir.join("a.txt"), format!("{n}\n")).unwrap();
        stage(dir, &["a.txt".into()]).unwrap();
        commit(dir, message).unwrap()
    }

    #[test]
    fn the_history_lists_commits_newest_first_up_to_the_limit() {
        let (dir, _repo) = repo();
        for (n, message) in ["first", "second\n\nbody", "third"].iter().enumerate() {
            commit_file(&dir, n, message);
        }
        let all = history(&dir, 10).unwrap();
        let summaries: Vec<&str> = all.commits.iter().map(|c| c.summary.as_str()).collect();
        assert_eq!(summaries, ["third", "second", "first"]);
        assert_eq!(all.commits[0].author, "Test");
        assert_eq!(all.commits[0].id.len(), 7);
        assert!(all.commits[0].time > 0);
        assert!(!all.more);
        let two = history(&dir, 2).unwrap();
        assert_eq!(two.commits.len(), 2);
        assert!(two.more);
        assert!(!history(&dir, 3).unwrap().more);
        assert_eq!(history(&temp_dir("git-log-plain"), 10), Err(GitChangesError::NotARepository));
    }

    #[test]
    fn marks_head_and_the_other_refs_and_counts_against_the_upstream() {
        let (dir, repo) = repo();
        let base = commit_file(&dir, 0, "base");
        let base = repo.revparse_single(&base).unwrap().peel_to_commit().unwrap();
        // Named to sort last, though git lists local branches first.
        repo.branch("zeta", &base, false).unwrap();
        repo.tag_lightweight("v1", base.as_object(), false).unwrap();
        // A remote branch at the base, tracked by the checked-out one.
        repo.reference("refs/remotes/origin/main", base.id(), false, "").unwrap();
        repo.reference_symbolic("refs/remotes/origin/HEAD", "refs/remotes/origin/main", false, "").unwrap();
        let branch = repo.head().unwrap().shorthand().unwrap().to_string();
        let mut config = repo.config().unwrap();
        config.set_str(&format!("branch.{branch}.remote"), "origin").unwrap();
        config.set_str(&format!("branch.{branch}.merge"), "refs/heads/main").unwrap();
        repo.remote("origin", "https://example.com/repo.git").unwrap();
        commit_file(&dir, 1, "tip");

        let h = history(&dir, 10).unwrap();
        assert_eq!(h.branch.as_deref(), Some(branch.as_str()));
        assert_eq!(h.upstream.as_deref(), Some("origin/main"));
        assert_eq!((h.ahead, h.behind), (1, 0));
        let marks: Vec<(bool, Vec<String>)> = h.commits.iter().map(|c| (c.head, c.refs.clone())).collect();
        assert_eq!(marks, [(true, vec![]), (false, vec!["origin/main".into(), "v1".into(), "zeta".into()])]);
    }

    #[test]
    fn refuses_what_it_cannot_commit_or_reach() {
        let (dir, _repo) = repo();
        assert_eq!(commit(&dir, "x"), Err(GitChangesError::NothingStaged));
        fs::write(dir.join("a.txt"), "one\n").unwrap();
        stage(&dir, &["a.txt".into()]).unwrap();
        assert_eq!(commit(&dir, "  \n"), Err(GitChangesError::EmptyMessage));
        for bad in ["../x", "/etc/passwd", "./a.txt", ""] {
            assert_eq!(stage(&dir, &[bad.into()]), Err(GitChangesError::InvalidPath(bad.into())));
        }
        let plain = temp_dir("git-changes-plain");
        assert_eq!(changes(&plain), Err(GitChangesError::NotARepository));
    }
}
