//! Recent commits of the repository a folder is in — what each said and which
//! files it touched — for code search to weigh history in its ranking.
//!
//! Only commits that say something about a few files are kept. A merge
//! repeats its branch; an initial commit, a mass rename or a reformat touches
//! everything, and "everything" is no evidence about any one file.

use std::path::Path;

use git2::{Repository, Sort};

use crate::domain::code_search::CommitNote;

/// The commit HEAD points at, to tell whether history changed since it was
/// last read. `None` outside a repository or before the first commit.
pub fn head(root: &Path) -> Option<String> {
    let repo = Repository::discover(root).ok()?;
    let id = repo.head().ok()?.target()?;
    Some(id.to_string())
}

/// Up to `limit` of the newest non-merge commits touching at most
/// `max_files` files, newest first. Paths are relative to `root`, which may
/// be a subdirectory of the repository; files outside it are left out, and a
/// commit left with none is skipped.
pub fn recent_commits(root: &Path, limit: usize, max_files: usize) -> Vec<CommitNote> {
    let Ok(repo) = Repository::discover(root) else {
        return Vec::new();
    };
    let prefix = prefix_of(&repo, root).unwrap_or_default();
    let Ok(mut walk) = repo.revwalk() else {
        return Vec::new();
    };
    if walk.push_head().is_err() {
        return Vec::new();
    }
    let _ = walk.set_sorting(Sort::TIME);

    let mut notes = Vec::new();
    for id in walk.flatten() {
        if notes.len() == limit {
            break;
        }
        let Ok(commit) = repo.find_commit(id) else { continue };
        if commit.parent_count() > 1 {
            continue;
        }
        let Ok(tree) = commit.tree() else { continue };
        let parent = commit.parent(0).ok().and_then(|p| p.tree().ok());
        let Ok(diff) = repo.diff_tree_to_tree(parent.as_ref(), Some(&tree), None) else { continue };
        if diff.deltas().len() > max_files {
            continue;
        }
        let files: Vec<String> = diff
            .deltas()
            .filter_map(|delta| delta.new_file().path().or_else(|| delta.old_file().path()))
            .filter_map(|path| path.to_str()?.strip_prefix(prefix.as_str()).map(str::to_string))
            .collect();
        let message = commit.message().unwrap_or_default().trim().to_string();
        if files.is_empty() || message.is_empty() {
            continue;
        }
        notes.push(CommitNote { message, files });
    }
    notes
}

/// `root` relative to the working tree, `/`-separated and ending in `/`, or
/// empty when they are the same directory.
fn prefix_of(repo: &Repository, root: &Path) -> Option<String> {
    let workdir = repo.workdir()?.canonicalize().ok()?;
    let root = root.canonicalize().ok()?;
    let relative = root.strip_prefix(&workdir).ok()?;
    let parts: Vec<String> = relative.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    Some(if parts.is_empty() { String::new() } else { format!("{}/", parts.join("/")) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;
    use git2::{IndexAddOption, Signature};

    fn commit(repo: &Repository, message: &str) {
        let mut index = repo.index().unwrap();
        index.add_all(["*"], IndexAddOption::DEFAULT, None).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = Signature::now("Test", "test@example.com").unwrap();
        let parents: Vec<git2::Commit> = repo.head().ok().and_then(|h| h.peel_to_commit().ok()).into_iter().collect();
        let parents: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents).unwrap();
    }

    fn write(root: &Path, relative: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, relative).unwrap();
    }

    /// Newest first, each with the files it touched; a commit touching too
    /// many is no evidence about any one of them and is left out.
    #[test]
    fn recent_commits_carry_their_files_and_skip_sweeping_ones() {
        let root = temp_dir("history-recent");
        let repo = Repository::init(&root).unwrap();
        for n in 0..5 {
            write(&root, &format!("bulk/{n}.txt"));
        }
        commit(&repo, "initial import");
        write(&root, "src/retry.rs");
        commit(&repo, "retry failed notifications with backoff");
        write(&root, "src/list.rs");
        commit(&repo, "update get patent list\n\nbody line");

        let notes = recent_commits(&root, 10, 3);

        let seen: Vec<(&str, Vec<&str>)> =
            notes.iter().map(|n| (n.message.as_str(), n.files.iter().map(String::as_str).collect())).collect();
        assert_eq!(
            seen,
            [
                ("update get patent list\n\nbody line", vec!["src/list.rs"]),
                ("retry failed notifications with backoff", vec!["src/retry.rs"]),
            ]
        );
        assert_eq!(recent_commits(&root, 1, 3).len(), 1, "the limit holds");
        assert!(head(&root).is_some());
    }

    /// A folder inside a repository sees its own files, relative to itself.
    #[test]
    fn a_subfolder_sees_only_its_own_files_relative_to_it() {
        let root = temp_dir("history-sub");
        let repo = Repository::init(&root).unwrap();
        write(&root, "app/src/a.rs");
        write(&root, "other/b.rs");
        commit(&repo, "touch both");
        write(&root, "other/c.rs");
        commit(&repo, "only outside");

        let notes = recent_commits(&root.join("app"), 10, 5);

        assert_eq!(notes.len(), 1, "a commit with nothing inside is skipped");
        assert_eq!(notes[0].files, ["src/a.rs"]);
    }

    /// A merge repeats what its branch already said, commit by commit.
    #[test]
    fn a_merge_is_not_read_as_a_commit_of_its_own() {
        let root = temp_dir("history-merge");
        let repo = Repository::init(&root).unwrap();
        write(&root, "a.rs");
        commit(&repo, "base");
        let base = repo.head().unwrap().peel_to_commit().unwrap();
        write(&root, "b.rs");
        let mut index = repo.index().unwrap();
        index.add_all(["*"], IndexAddOption::DEFAULT, None).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = Signature::now("Test", "test@example.com").unwrap();
        let side = repo.find_commit(repo.commit(None, &sig, &sig, "side", &tree, &[&base]).unwrap()).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "merge side", &tree, &[&base, &side]).unwrap();

        let messages: Vec<String> = recent_commits(&root, 10, 5).into_iter().map(|n| n.message).collect();

        assert!(!messages.contains(&"merge side".to_string()), "{messages:?}");
    }

    #[test]
    fn outside_a_repository_there_is_no_history() {
        let root = temp_dir("history-none");
        assert!(recent_commits(&root, 10, 5).is_empty());
        assert!(head(&root).is_none());
    }
}
