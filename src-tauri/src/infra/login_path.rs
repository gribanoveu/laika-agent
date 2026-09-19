//! The `PATH` an app started from the Dock or Finder does not get.
//!
//! macOS gives such an app `/usr/bin:/bin:/usr/sbin:/sbin` — no Homebrew, no
//! `nvm`, no `~/.cargo/bin` — so `npx` for an MCP server and `cargo test`
//! for `runCommand` are "not found" in the app while working in a terminal.
//! The user's own `PATH` is what their login shell builds; it is asked for
//! once and given to every process the app starts.
//!
//! Not asked when the app was started from a terminal: that `PATH` is the
//! user's already, perhaps with a virtualenv on it that a fresh login shell
//! would lose.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::Duration;

use super::process_runner::{kill_tree, set_process_group};

const MARK: &str = "__ATLAS_PATH__";
/// A login shell that loads `nvm` or `conda` takes a second or two; one that
/// takes longer is not waited for.
const TIMEOUT: Duration = Duration::from_secs(5);

/// The `PATH` to start processes with, or `None` to leave them the app's own.
pub fn path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(resolve).as_deref()
}

/// Sets it on a command about to start; an explicit `PATH` set afterwards,
/// such as one from an MCP server's `env`, still wins.
pub fn apply(command: &mut Command) {
    if let Some(path) = path() {
        command.env("PATH", path);
    }
}

#[cfg(unix)]
fn resolve() -> Option<String> {
    if std::env::var_os("TERM").is_some() {
        return None;
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let login = from_shell(&shell, TIMEOUT)?;
    Some(merge(&login, &std::env::var("PATH").unwrap_or_default()))
}

#[cfg(not(unix))]
fn resolve() -> Option<String> {
    None
}

/// Interactive as well as login: `nvm` and friends are usually set up in
/// `.zshrc`, which a login shell alone does not read. What such a shell
/// prints around the value — a banner, a warning — is cut off by the marks.
fn from_shell(shell: &str, timeout: Duration) -> Option<String> {
    let mut command = Command::new(shell);
    command
        .args(["-ilc", &format!("printf '{MARK}%s{MARK}' \"$PATH\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    set_process_group(&mut command);
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = String::new();
        let _ = stdout.read_to_string(&mut output);
        let _ = tx.send(output);
    });
    let output = rx.recv_timeout(timeout).ok();
    kill_tree(&mut child);
    let _ = child.kill();
    let _ = child.wait();
    parse(&output?)
}

fn parse(output: &str) -> Option<String> {
    let (_, rest) = output.split_once(MARK)?;
    let (path, _) = rest.split_once(MARK)?;
    (!path.is_empty()).then(|| path.to_string())
}

/// The shell's entries first, then the app's own that it lacks.
fn merge(login: &str, current: &str) -> String {
    let mut entries: Vec<&str> = login.split(':').filter(|e| !e.is_empty()).collect();
    for entry in current.split(':') {
        if !entry.is_empty() && !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    entries.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_value_is_read_between_the_marks_whatever_the_shell_prints() {
        assert_eq!(parse(&format!("Welcome!\n{MARK}/opt/bin:/usr/bin{MARK}bye")).as_deref(), Some("/opt/bin:/usr/bin"));
        assert_eq!(parse("no marks at all"), None);
        assert_eq!(parse(&format!("{MARK}{MARK}")), None);
    }

    #[test]
    fn the_shells_entries_come_first_and_the_apps_own_are_kept() {
        assert_eq!(merge("/opt/homebrew/bin:/usr/bin", "/usr/bin:/bin"), "/opt/homebrew/bin:/usr/bin:/bin");
    }

    #[cfg(unix)]
    #[test]
    fn a_real_login_shell_is_asked() {
        let path = from_shell("/bin/sh", TIMEOUT).expect("sh answers");
        assert!(path.split(':').any(|e| e == "/usr/bin" || e == "/bin"), "{path}");
    }

    #[cfg(unix)]
    #[test]
    fn a_shell_that_does_not_answer_in_time_is_not_waited_for() {
        let dir = crate::testing::temp_dir("login-path-slow");
        let shell = dir.join("slow.sh");
        std::fs::write(&shell, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&shell, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let began = std::time::Instant::now();
        assert_eq!(from_shell(shell.to_str().unwrap(), Duration::from_millis(300)), None);
        assert!(began.elapsed() < Duration::from_secs(5), "{:?}", began.elapsed());
    }
}
