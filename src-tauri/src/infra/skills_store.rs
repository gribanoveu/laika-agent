//! The user's skills: `<app dir>/skills/<name>/SKILL.md`, plus whatever files
//! a skill keeps beside it.
//!
//! Ported from Alfa Atlas `infra/user_skills_store.rs`, without import,
//! removal and the Settings preview — those arrive with the skills tab.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::domain::skills::{ParsedSkill, SkillError, SkillMeta, parse_skill_md, validate_skill_name};
use crate::infra::app_dir;

const SKILL_MD: &str = "SKILL.md";

/// How many companion files a loaded skill lists. A skill folder is written
/// by hand and should hold a handful; one that holds a `node_modules` would
/// otherwise put thousands of paths into the model's context.
const MAX_LISTED_FILES: usize = 100;

pub fn dir() -> Result<PathBuf, SkillError> {
    Ok(app_dir::dir().map_err(SkillError::Io)?.join("skills"))
}

/// Each folder under the skills directory, parsed or not. A folder whose
/// `SKILL.md` is broken is still an entry: the caller decides whether it
/// is shown (a settings list should say what is wrong) or skipped (the
/// model's catalog must not offer it).
pub struct SkillEntry {
    pub dir_name: String,
    pub parsed: Result<ParsedSkill, SkillError>,
}

/// A missing directory is no skills yet, not an error. Sorted by folder name
/// so the catalog the model sees is the same bytes from one turn to the next.
pub fn scan() -> Result<Vec<SkillEntry>, SkillError> {
    let dir = dir()?;
    let read = match fs::read_dir(&dir) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io(&dir, e)),
    };
    let mut entries = Vec::new();
    for entry in read {
        let path = entry.map_err(|e| io(&dir, e))?.path();
        let Some(dir_name) = path.file_name().and_then(|s| s.to_str()) else { continue };
        if !path.is_dir() || dir_name.starts_with('.') {
            continue;
        }
        let parsed = match fs::read_to_string(path.join(SKILL_MD)) {
            Ok(contents) => parse_skill_md(&contents, dir_name),
            Err(e) => Err(io(&path.join(SKILL_MD), e)),
        };
        entries.push(SkillEntry { dir_name: dir_name.to_string(), parsed });
    }
    entries.sort_by(|a, b| a.dir_name.cmp(&b.dir_name));
    Ok(entries)
}

/// The skills the model may load: every one that parses.
pub fn catalog() -> Result<Vec<SkillMeta>, SkillError> {
    Ok(scan()?.into_iter().filter_map(|e| e.parsed.ok().map(|p| p.meta)).collect())
}

/// A skill's instructions, and the files beside them it may point to.
pub fn load(name: &str) -> Result<(ParsedSkill, Vec<String>), SkillError> {
    let root = skill_root(name)?;
    let contents = fs::read_to_string(root.join(SKILL_MD)).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => SkillError::NotFound(name.to_string()),
        _ => io(&root, e),
    })?;
    let parsed = parse_skill_md(&contents, name)?;
    let mut files = Vec::new();
    collect_files(&root, &root, &mut files)?;
    files.sort();
    files.truncate(MAX_LISTED_FILES);
    Ok((parsed, files))
}

/// One companion file, by its path inside the skill folder.
pub fn read(name: &str, relative: &str) -> Result<String, SkillError> {
    let root = skill_root(name)?;
    if !root.join(SKILL_MD).is_file() {
        return Err(SkillError::NotFound(name.to_string()));
    }
    let target = resolve_under(&root, relative)?;
    fs::read_to_string(&target).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => SkillError::Io(format!("{name} has no file {relative}")),
        std::io::ErrorKind::InvalidData => SkillError::Io(format!("{name}/{relative} is not UTF-8 text")),
        _ => io(&target, e),
    })
}

/// Validated before joining: the spec's name rules admit no separator and
/// no `..`, which is what keeps a name from leaving the skills folder.
fn skill_root(name: &str) -> Result<PathBuf, SkillError> {
    validate_skill_name(name).map_err(|_| SkillError::NotFound(name.to_string()))?;
    Ok(dir()?.join(name))
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), SkillError> {
    for entry in fs::read_dir(dir).map_err(|e| io(dir, e))? {
        let entry = entry.map_err(|e| io(dir, e))?;
        let path = entry.path();
        if path.file_name().and_then(|s| s.to_str()).is_some_and(|n| n.starts_with('.')) {
            continue;
        }
        // Not `path.is_dir()`: that follows a symlink, and a link to a parent
        // is a walk that never ends.
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            collect_files(root, &path, out)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            if rel != SKILL_MD {
                out.push(rel);
            }
        }
    }
    Ok(())
}

/// `relative` under `root`, refusing anything that leaves it: `..` and
/// absolute paths before the disk is touched, and a symlink pointing out
/// after it resolves.
fn resolve_under(root: &Path, relative: &str) -> Result<PathBuf, SkillError> {
    let escape = || SkillError::PathEscape(relative.to_string());
    let rel = Path::new(relative);
    if relative.is_empty() || !rel.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Err(escape());
    }
    let joined = root.join(rel);
    let Ok(canon) = joined.canonicalize() else {
        // Missing: let the read report it by name.
        return Ok(joined);
    };
    let root = root.canonicalize().map_err(|e| io(root, e))?;
    if canon.starts_with(&root) { Ok(canon) } else { Err(escape()) }
}

fn io(path: &Path, e: std::io::Error) -> SkillError {
    SkillError::Io(format!("{}: {e}", path.display()))
}

#[cfg(test)]
pub mod test_support {
    use super::*;

    /// Writes `<skills>/<name>/SKILL.md` and returns the folder.
    pub fn write_skill(name: &str, description: &str, body: &str) -> PathBuf {
        let root = dir().unwrap().join(name);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(SKILL_MD), format!("---\nname: {name}\ndescription: {description}\n---\n{body}")).unwrap();
        root
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::write_skill;
    use super::*;
    use crate::testing::with_app_dir;

    #[test]
    fn no_skills_folder_is_an_empty_catalog() {
        with_app_dir("skills-none", || assert!(catalog().unwrap().is_empty()));
    }

    /// A broken skill is an entry — the tab will want to say what is wrong —
    /// but never something the model is offered.
    #[test]
    fn the_catalog_is_the_valid_skills_in_name_order() {
        with_app_dir("skills-catalog", || {
            write_skill("zeta", "Last.", "");
            write_skill("alpha", "First.", "");
            let broken = dir().unwrap().join("broken");
            fs::create_dir_all(&broken).unwrap();
            fs::write(broken.join(SKILL_MD), "no frontmatter").unwrap();
            fs::create_dir_all(dir().unwrap().join("empty")).unwrap();
            fs::write(dir().unwrap().join("stray.md"), "not a folder").unwrap();

            let names: Vec<String> = catalog().unwrap().into_iter().map(|m| m.name).collect();
            assert_eq!(names, ["alpha", "zeta"]);
            let scanned: Vec<String> = scan().unwrap().into_iter().map(|e| e.dir_name).collect();
            assert_eq!(scanned, ["alpha", "broken", "empty", "zeta"]);
        });
    }

    #[test]
    fn a_skill_loads_with_its_files_and_reads_one_of_them() {
        with_app_dir("skills-load", || {
            let root = write_skill("my-skill", "Does a thing.", "# Steps\n");
            fs::create_dir_all(root.join("references")).unwrap();
            fs::write(root.join("references/notes.md"), "extra").unwrap();
            fs::write(root.join(".DS_Store"), "junk").unwrap();

            let (parsed, files) = load("my-skill").unwrap();
            assert_eq!(parsed.body, "# Steps\n");
            assert_eq!(files, ["references/notes.md"]);
            assert_eq!(read("my-skill", "references/notes.md").unwrap(), "extra");
        });
    }

    #[test]
    fn an_unknown_or_malformed_name_is_not_found() {
        with_app_dir("skills-unknown", || {
            assert_eq!(load("nope").unwrap_err(), SkillError::NotFound("nope".into()));
            assert_eq!(load("../skills").unwrap_err(), SkillError::NotFound("../skills".into()));
            assert_eq!(read("nope", "x.md").unwrap_err(), SkillError::NotFound("nope".into()));
        });
    }

    /// A name is joined onto the skills folder, so one that climbs out of it
    /// would reach any skill-shaped folder in the app directory.
    #[test]
    fn a_name_cannot_climb_out_of_the_skills_folder() {
        with_app_dir("skills-name-escape", || {
            // The folder has to exist: `skills/../x` resolves only through it.
            write_skill("my-skill", "Does a thing.", "");
            let beside = dir().unwrap().parent().unwrap().join("elsewhere");
            fs::create_dir_all(&beside).unwrap();
            fs::write(beside.join(SKILL_MD), "---\nname: elsewhere\ndescription: Not a skill.\n---\n").unwrap();
            fs::write(beside.join("secret.txt"), "secret").unwrap();

            assert_eq!(read("../elsewhere", "secret.txt").unwrap_err(), SkillError::NotFound("../elsewhere".into()));
        });
    }

    #[test]
    fn a_path_outside_the_skill_folder_is_refused() {
        with_app_dir("skills-escape", || {
            write_skill("my-skill", "Does a thing.", "");
            write_skill("other", "Another.", "");
            for path in ["../other/SKILL.md", "/etc/hosts", "", "a/../../other/SKILL.md"] {
                assert!(
                    matches!(read("my-skill", path), Err(SkillError::PathEscape(_))),
                    "{path:?} was not refused"
                );
            }
        });
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_skill_folder_is_refused() {
        with_app_dir("skills-symlink", || {
            let root = write_skill("my-skill", "Does a thing.", "");
            let outside = crate::testing::temp_dir("skills-outside").join("secret.txt");
            fs::write(&outside, "secret").unwrap();
            std::os::unix::fs::symlink(&outside, root.join("link.txt")).unwrap();
            assert!(matches!(read("my-skill", "link.txt"), Err(SkillError::PathEscape(_))));
        });
    }
}
