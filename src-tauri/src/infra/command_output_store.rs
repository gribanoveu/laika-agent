//! `<app dir>/command-output/`: the whole output of a command whose result
//! was cut, kept for [`RETENTION_DAYS`].
//!
//! The model sees the head and the tail of a long log; what it needs is often
//! in the middle — the first error of a build that failed five hundred lines
//! later. With the whole of it on disk it greps that instead of running the
//! build again. Best-effort, like the tool call log: a save that fails leaves
//! the result as cut as it was, never fails the command. Trimmed on write, so
//! no background job.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::infra::app_dir;

const DIR: &str = "command-output";
pub const RETENTION_DAYS: u64 = 7;
const RETENTION: Duration = Duration::from_secs(RETENTION_DAYS * 24 * 60 * 60);

/// Writes both streams to a new file and returns its path, or `None` if it
/// could not be written. A stream is under a heading only when the other one
/// said something too — a lone stdout is just the log.
pub fn save(stdout: &str, stderr: &str) -> Option<PathBuf> {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = app_dir::ensure().ok()?.join(DIR);
    fs::create_dir_all(&dir).ok()?;
    prune(&dir);

    let text = match (stdout.is_empty(), stderr.is_empty()) {
        (false, false) => format!("--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"),
        (true, _) => stderr.to_string(),
        (_, true) => stdout.to_string(),
    };
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_millis();
    let path = dir.join(format!("{millis}-{}-{}.log", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    app_dir::write_private(&path, text.as_bytes()).ok()?;
    Some(path)
}

fn prune(dir: &std::path::Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|modified| now.duration_since(modified).is_ok_and(|age| age > RETENTION));
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::with_app_dir;

    #[test]
    fn both_streams_are_kept_whole_and_labelled() {
        with_app_dir("cmd-out-save", || {
            let path = save("out\n", "err\n").expect("saved");
            assert_eq!(fs::read_to_string(&path).unwrap(), "--- stdout ---\nout\n\n--- stderr ---\nerr\n");
            let lone = save("only\n", "").expect("saved");
            assert_eq!(fs::read_to_string(&lone).unwrap(), "only\n");
            assert_ne!(path, lone, "each save is its own file");
        });
    }

    #[test]
    fn files_past_retention_go_on_the_next_save() {
        with_app_dir("cmd-out-retention", || {
            let old = save("old", "").expect("saved");
            let recent = save("recent", "").expect("saved");
            let past = SystemTime::now() - RETENTION - Duration::from_secs(60);
            fs::File::options().write(true).open(&old).unwrap().set_modified(past).unwrap();

            save("new", "").expect("saved");

            assert!(!old.exists(), "past retention");
            assert!(recent.exists(), "within retention");
        });
    }
}
