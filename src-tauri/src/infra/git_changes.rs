//! Staging and committing in the open folder's repository — the Changes
//! panel's four operations, over `git2`.
//!
//! The repository is discovered from the folder, so a folder inside a
//! repository works. Paths go both ways relative to the repository root.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use git2::{Diff, DiffOptions, ErrorCode, Repository, Signature};

use crate::domain::git_changes::{ChangedFile, GitChangesError, WorkingChanges};

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
