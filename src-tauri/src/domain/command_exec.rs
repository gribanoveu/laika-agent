//! Running a command: what is asked for, what comes back, and how much of it
//! the model is allowed to read.
//!
//! ## This is the tool that ends containment
//!
//! Every other tool resolves its paths against the workspace root, and that
//! root is a real boundary (`services::ai_tools::resolve`). A command line is
//! not: `cat ../../.ssh/id_rsa` is a perfectly ordinary command, and no amount
//! of checking the working directory changes that. So `cwd` is resolved inside
//! the root for the same reason a `cd` is convenient, **not** as a security
//! measure, and the honest statement is that from here on the approval gate is
//! the only boundary there is.
//!
//! A sandbox (CA-4.7) is what would change that. There is a place for one in
//! the runner's interface and no implementation behind it; until there is,
//! nothing here should be described as containing anything.
//!
//! ## Why a shell, and which one
//!
//! The command runs through a shell, because a coding agent's commands are
//! shell commands: `cargo test 2>&1 | tail -20`, `cd crate && make`. Parsing
//! an argv ourselves would reject those, and the model would have to discover
//! by trial which half of its vocabulary works.
//!
//! Which shell is a setting, not a search of `PATH`. On Windows the difference
//! between `cmd`, PowerShell and Git Bash is not cosmetic — the same line means
//! different things in each — and a tool that silently picks one produces
//! failures nobody can reproduce.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Used when a call names no timeout. Long enough for a test suite, short
/// enough that a command waiting on input nobody will type does not hold the
/// turn all day.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// The ceiling a call cannot raise. A build that genuinely takes longer is a
/// background process (CA-4.5), not a tool call the turn waits on.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(600);

/// Per stream. Beyond this the middle is dropped — see [`truncate_output`].
pub const MAX_OUTPUT_CHARS: usize = 30_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CommandRequest {
    /// One command line, as it would be typed.
    pub command: String,
    /// Where to run it, relative to the workspace root. `None` is the root.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Seconds. `None` — and `0`, which is what a model sends when it means
    /// "no limit" — both take [`DEFAULT_TIMEOUT`]: of the two readings of an
    /// ambiguous zero, "kill it instantly" is the one that is never wanted.
    #[serde(default, deserialize_with = "crate::domain::flexible_args::opt_u32")]
    pub timeout_seconds: Option<u32>,
    /// Keep it running after the call returns — see `domain::background`.
    /// The timeout does not apply.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::domain::flexible_args::opt_bool")]
    pub background: Option<bool>,
}

impl CommandRequest {
    pub fn timeout(&self) -> Duration {
        match self.timeout_seconds {
            None | Some(0) => DEFAULT_TIMEOUT,
            Some(seconds) => Duration::from_secs(seconds as u64).min(MAX_TIMEOUT),
        }
    }
}

/// Which shell runs the command line, and how it is told to.
///
/// A setting rather than a search of `PATH`, for the reason in the module
/// doc. The defaults are only defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shell {
    pub program: String,
    /// Everything before the command line itself — `["-c"]`, `["/C"]`.
    pub args: Vec<String>,
}

impl Default for Shell {
    fn default() -> Self {
        #[cfg(unix)]
        {
            Self {
                // `sh`, not the user's login shell: a command that works here
                // works for everyone on the project, which is not true of a
                // line that quietly depends on someone's zsh functions.
                program: "/bin/sh".to_string(),
                args: vec!["-c".to_string()],
            }
        }
        #[cfg(windows)]
        {
            Self {
                program: "cmd.exe".to_string(),
                args: vec!["/C".to_string()],
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// Output as it arrives, so a long command shows progress instead of looking
/// like a hang. One event type rather than two callbacks: a service that
/// reports several kinds of thing reports them through one enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandEvent {
    pub stream: OutputStream,
    pub chunk: String,
}

pub type CommandSink = Arc<dyn Fn(CommandEvent) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    /// `None` when nothing exited normally — killed by the timeout, or by a
    /// signal. Distinguishing that from `Some(0)` is the whole question the
    /// model is asking.
    #[serde(default)]
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// Either stream had its middle dropped. Stated separately from the
    /// markers inside the text so a reader does not have to search for them.
    pub truncated: bool,
}

impl CommandOutput {
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0)
    }
}

#[derive(Debug, Error)]
pub enum CommandError {
    /// The command never started: no such shell, no such directory, no
    /// permission. Distinct from a command that started and failed, which is
    /// an ordinary [`CommandOutput`] with a non-zero code.
    #[error("could not start the command: {0}")]
    NotStarted(String),
    #[error("{0}")]
    Cwd(String),
    #[error("the command was started but could not be read: {0}")]
    Io(String),
}

/// Keeps the beginning and the end of `text`, dropping the middle.
///
/// Head-only truncation is what a diff wants and the wrong shape here. A test
/// run's verdict is in the last lines (`test result: FAILED. 3 passed; 497
/// failed`) and the first real failure is near the top; cutting either end
/// throws away half of what was asked for. The marker says how many lines went
/// missing, so the model can ask for them specifically rather than re-running
/// the whole thing blind.
pub fn truncate_output(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }
    let lines: Vec<&str> = text.lines().collect();
    // Half the budget each way, less a little for the marker itself.
    let half = max_chars / 2;

    let mut head: Vec<&str> = Vec::new();
    let mut head_chars = 0;
    for line in &lines {
        let cost = line.chars().count() + 1;
        if head_chars + cost > half {
            break;
        }
        head.push(line);
        head_chars += cost;
    }

    let mut tail: Vec<&str> = Vec::new();
    let mut tail_chars = 0;
    for line in lines.iter().rev() {
        if tail.len() + head.len() >= lines.len() {
            break;
        }
        let cost = line.chars().count() + 1;
        if tail_chars + cost > half {
            break;
        }
        tail.push(line);
        tail_chars += cost;
    }
    tail.reverse();

    let dropped = lines.len() - head.len() - tail.len();
    let mut out = head.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!("[... {dropped} lines omitted ...]\n"));
    out.push_str(&tail.join("\n"));
    (out, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(timeout_seconds: Option<u32>) -> CommandRequest {
        CommandRequest {
            command: "cargo test".to_string(),
            cwd: None,
            timeout_seconds,
            background: None,
        }
    }

    #[test]
    fn an_unspecified_timeout_is_the_default() {
        assert_eq!(request(None).timeout(), DEFAULT_TIMEOUT);
    }

    /// A model that sends `0` means "no limit", never "kill it immediately" —
    /// and of the two readings only one is ever wanted.
    #[test]
    fn a_zero_timeout_is_read_as_unspecified() {
        assert_eq!(request(Some(0)).timeout(), DEFAULT_TIMEOUT);
    }

    #[test]
    fn a_timeout_is_honoured_up_to_the_ceiling() {
        assert_eq!(request(Some(5)).timeout(), Duration::from_secs(5));
        assert_eq!(request(Some(100_000)).timeout(), MAX_TIMEOUT);
    }

    /// "Started and failed" and "never started" are different answers, and
    /// only one of them is about the command the model wrote.
    #[test]
    fn a_non_zero_exit_is_a_result_not_an_error() {
        let failed = CommandOutput {
            exit_code: Some(1),
            ..Default::default()
        };
        assert!(!failed.succeeded());
        assert!(!failed.timed_out);
    }

    #[test]
    fn a_killed_command_has_no_exit_code() {
        let killed = CommandOutput {
            timed_out: true,
            exit_code: None,
            ..Default::default()
        };
        assert!(!killed.succeeded(), "silence is not success");
    }

    #[test]
    fn short_output_is_left_alone() {
        let (out, truncated) = truncate_output("one\ntwo\n", 100);
        assert_eq!(out, "one\ntwo\n");
        assert!(!truncated);
    }

    /// The verdict is at the end and the first failure is near the top. Losing
    /// either end throws away half of what was asked for.
    #[test]
    fn truncation_keeps_both_ends() {
        let mut lines: Vec<String> = vec!["first failure: assertion failed".to_string()];
        lines.extend((0..500).map(|i| format!("noise line {i}")));
        lines.push("test result: FAILED. 3 passed; 497 failed".to_string());
        let text = lines.join("\n");

        let (out, truncated) = truncate_output(&text, 400);

        assert!(truncated);
        assert!(out.starts_with("first failure"), "{out}");
        assert!(out.trim_end().ends_with("497 failed"), "{out}");
        assert!(out.chars().count() < text.chars().count());
    }

    /// The model has to be able to ask for what it did not get, rather than
    /// re-running the whole thing to see the middle.
    #[test]
    fn truncation_says_how_much_went_missing() {
        let text: String = (0..100)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");

        let (out, _) = truncate_output(&text, 120);

        let marker = out
            .lines()
            .find(|l| l.contains("omitted"))
            .expect("the cut is marked");
        let dropped: usize = marker
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .expect("a count");
        let kept = out.lines().count() - 1;
        assert_eq!(dropped + kept, 100, "every line is either kept or counted");
    }

    /// Cutting by bytes would panic here, and cutting mid-line would make the
    /// output read as corrupted rather than merely incomplete.
    #[test]
    fn truncation_is_safe_on_multibyte_text() {
        let text: String = (0..200)
            .map(|i| format!("строка {i} — проверка"))
            .collect::<Vec<_>>()
            .join("\n");

        let (out, truncated) = truncate_output(&text, 300);

        assert!(truncated);
        assert!(out.contains("строка 0"));
        assert!(out.contains("строка 199"));
    }

    /// One line longer than the whole budget still has to come back as
    /// something, and still has to say that it was cut.
    #[test]
    fn a_single_enormous_line_is_still_reported_as_cut() {
        let text = "x".repeat(10_000);
        let (out, truncated) = truncate_output(&text, 100);
        assert!(truncated);
        assert!(out.contains("omitted"), "{out}");
    }
}
