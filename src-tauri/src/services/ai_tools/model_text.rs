//! What the model reads back from a tool: plain text, in the shape the same
//! answer takes where the model learned it — a diff as a diff, a search as
//! `rg` prints one, a status as `git status` does.
//!
//! Serialized JSON would carry the same facts, but every newline and quote in
//! a file or a command's output becomes an escape the model pays for and has
//! to read through. The UI and hooks still get the structure; only this
//! reading changes.
//!
//! The `match` is exhaustive on purpose: a new result kind has to decide how
//! it reads, rather than fall back to JSON unnoticed.

use crate::domain::background::ProcessOutput;
use crate::domain::code_search::{CodeMatch, SearchMeta};
use crate::domain::command_exec::CommandOutput;
use crate::domain::tools::{BlameHunk, GitFileStatus, GrepMatch, OutlineEntry, Task, TodoStatus, ToolResult};
use crate::services::ai_tools::tools::list_files::render_file_tree;
use crate::services::text_diff::render_for_model;

pub fn for_model(result: &ToolResult) -> String {
    match result {
        ToolResult::File { content, start_line, end_line, total_lines } => file(content, *start_line, *end_line, *total_lines),
        ToolResult::FileOutline { path, entries, total_lines } => outline(path, entries, *total_lines),
        ToolResult::GrepResults { matches, truncated } => grep(matches, *truncated),
        ToolResult::FileList { entries, truncated } => render_file_tree(entries, *truncated),
        ToolResult::FileWritten { path, diff } => render_for_model("Wrote", path, diff, true),
        ToolResult::FileEdited { path, diff } => render_for_model("Edited", path, diff, true),
        // How much went, not the whole file read back.
        ToolResult::FileDeleted { path, diff } => render_for_model("Deleted", path, diff, false),
        ToolResult::DirectoryCreated { path } => format!("Created directory {path}"),
        ToolResult::DirectoryDeleted { path } => format!("Deleted directory {path}"),
        ToolResult::Moved { from, to } => format!("Moved {from} → {to}"),
        ToolResult::Todo { tasks } => todo(tasks),
        ToolResult::GitStatus { branch, staged, unstaged, conflicted, truncated } => {
            git_status(branch.as_deref(), staged, unstaged, conflicted, *truncated)
        }
        ToolResult::GitDiff { path, label, is_binary: true, .. } => format!("{path} is a binary file — no text diff ({label})"),
        ToolResult::GitDiff { path, label, diff, .. } => render_for_model(&format!("Diff ({label}):"), path, diff, true),
        ToolResult::GitBlame { path, hunks, truncated } => blame(path, hunks, *truncated),
        ToolResult::CommandRan(output) => command(output),
        ToolResult::ProcessStarted(process) => {
            format!("Started {} in {}. Read what it writes with readOutput.", process.describe(), process.cwd)
        }
        ToolResult::ProcessOutput(output) => process_output(output),
        ToolResult::ProcessStopped(process) => process.describe(),
        ToolResult::SearchResults { matches, meta } => search(matches, meta),
        ToolResult::Skill { name, instructions, files } => skill(name, instructions, files),
        ToolResult::SkillFile { name, path, content } => format!("{name}/{path}:\n{content}"),
        ToolResult::PlanWritten { lines } => format!("Plan saved ({lines} lines). The user sees it in the Plan tab."),
        // Already the text the server meant for a model.
        ToolResult::Mcp { text } => text.clone(),
    }
}

/// The range comes first: whether this is the whole file is the one thing the
/// content alone does not say.
fn file(content: &str, start: u32, end: u32, total: u32) -> String {
    if total == 0 {
        return "The file is empty.".to_string();
    }
    let head = if start == 1 && end == total {
        format!("All {total} lines:")
    } else {
        format!("Lines {start}-{end} of {total}:")
    };
    format!("{head}\n{content}")
}

fn outline(path: &str, entries: &[OutlineEntry], total: u32) -> String {
    if entries.is_empty() {
        return format!("No declarations or headings found in {path} ({total} lines).");
    }
    let rows: Vec<String> = entries.iter().map(|e| format!("{}-{}  {}", e.start_line, e.end_line, e.name)).collect();
    format!("Outline of {path} ({total} lines), start-end:\n{}", rows.join("\n"))
}

/// As `rg -n` prints it: a file's name once, then `line:` for a hit and
/// `line-` for the context around it, `--` between groups that do not touch.
fn grep(matches: &[GrepMatch], truncated: bool) -> String {
    if matches.is_empty() {
        return "No matches.".to_string();
    }
    let mut out = String::new();
    let mut current: Option<&str> = None;
    let mut last_line = 0u32;
    for m in matches {
        let first = m.line.saturating_sub(m.before.len() as u32);
        if current != Some(m.path.as_str()) {
            if current.is_some() {
                out.push('\n');
            }
            out.push_str(&m.path);
            out.push('\n');
            current = Some(&m.path);
        } else if first > last_line + 1 {
            out.push_str("--\n");
        }
        for (k, text) in m.before.iter().enumerate() {
            let n = first + k as u32;
            if n > last_line {
                out.push_str(&format!("{n}-{text}\n"));
            }
        }
        out.push_str(&format!("{}:{}\n", m.line, m.text));
        for (k, text) in m.after.iter().enumerate() {
            out.push_str(&format!("{}-{text}\n", m.line + 1 + k as u32));
        }
        last_line = m.line + m.after.len() as u32;
    }
    if truncated {
        out.push_str("\n[more matches not shown — narrow the pattern or the glob]");
    }
    out.trim_end().to_string()
}

fn todo(tasks: &[Task]) -> String {
    if tasks.is_empty() {
        return "The checklist is empty.".to_string();
    }
    let rows: Vec<String> = tasks
        .iter()
        .map(|t| {
            let mark = match t.status {
                TodoStatus::Pending => "[ ]",
                TodoStatus::InProgress => "[>]",
                TodoStatus::Completed => "[x]",
                TodoStatus::Cancelled => "[-]",
            };
            let note = t.note.as_deref().map(|n| format!(" — {n}")).unwrap_or_default();
            format!("{mark} {} {}{note}", t.id, t.title)
        })
        .collect();
    format!("Checklist ([>] in progress, [x] done, [-] cancelled; ids first):\n{}", rows.join("\n"))
}

fn git_status(
    branch: Option<&str>,
    staged: &[GitFileStatus],
    unstaged: &[GitFileStatus],
    conflicted: &[GitFileStatus],
    truncated: bool,
) -> String {
    let mut out = match branch {
        Some(b) => format!("On branch {b}"),
        None => "No branch (detached HEAD or no commits yet)".to_string(),
    };
    if staged.is_empty() && unstaged.is_empty() && conflicted.is_empty() {
        out.push_str("\nNothing changed.");
    }
    for (title, files) in [("Conflicted", conflicted), ("Staged", staged), ("Not staged", unstaged)] {
        if files.is_empty() {
            continue;
        }
        out.push_str(&format!("\n{title}:"));
        for f in files {
            out.push_str(&format!("\n  {} {}", f.status, f.path));
        }
    }
    if truncated {
        out.push_str("\n[more paths not shown]");
    }
    out
}

fn blame(path: &str, hunks: &[BlameHunk], truncated: bool) -> String {
    let rows: Vec<String> = hunks
        .iter()
        .map(|h| {
            let last = h.start_line + h.line_count.saturating_sub(1);
            format!("{}-{last}  {}  {}  {}  {}", h.start_line, h.commit, h.date, h.author, h.summary)
        })
        .collect();
    let more = if truncated { "\n[more lines not shown — ask for a range]" } else { "" };
    format!("Blame of {path} (lines, commit, date, author, message):\n{}{more}", rows.join("\n"))
}

/// The exit comes first: it is the answer, and the output is the evidence.
fn command(output: &CommandOutput) -> String {
    let head = match (output.timed_out, output.exit_code) {
        (true, _) => "Timed out and was killed, with everything it started.".to_string(),
        (false, Some(code)) => format!("Exit code {code}"),
        (false, None) => "Ended by a signal, with no exit code.".to_string(),
    };
    let mut out = head;
    for (name, text) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
        if !text.trim().is_empty() {
            out.push_str(&format!("\n{name}:\n{}", text.trim_end()));
        }
    }
    if output.stdout.trim().is_empty() && output.stderr.trim().is_empty() {
        out.push_str("\n(no output)");
    }
    out
}

fn process_output(output: &ProcessOutput) -> String {
    let mut out = output.process.describe();
    if output.missed {
        out.push_str("\n[some output was lost before this read]");
    }
    if output.output.is_empty() {
        out.push_str("\n(no new output)");
    } else {
        out.push_str(&format!("\n{}", output.output.trim_end()));
    }
    out
}

/// Each hit as where it is, then what it says; the hint last, as the thing to
/// act on.
fn search(matches: &[CodeMatch], meta: &SearchMeta) -> String {
    let mut out = if matches.is_empty() {
        "No matches.".to_string()
    } else {
        let hits: Vec<String> = matches
            .iter()
            .map(|m| {
                let name = m.name.as_deref().map(|n| format!("  {n}")).unwrap_or_default();
                format!("{}:{}-{}{name}\n{}", m.path, m.start_line, m.end_line, m.text.trim_end())
            })
            .collect();
        hits.join("\n\n")
    };
    if meta.weak && !matches.is_empty() {
        out.push_str("\n\n[these matches are weak — none is a close fit]");
    }
    if let Some(hint) = &meta.hint {
        out.push_str(&format!("\n\n{hint}"));
    }
    out
}

fn skill(name: &str, instructions: &str, files: &[String]) -> String {
    let mut out = format!("Skill {name}:\n{}", instructions.trim_end());
    if !files.is_empty() {
        out.push_str(&format!("\n\nFiles in this skill (read one with skill and its path):\n- {}", files.join("\n- ")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::background::{ProcessInfo, ProcessState};

    fn hit(path: &str, line: u32, text: &str, before: &[&str], after: &[&str]) -> GrepMatch {
        GrepMatch {
            path: path.into(),
            line,
            text: text.into(),
            before: before.iter().map(|s| s.to_string()).collect(),
            after: after.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_file_says_whether_it_is_whole() {
        let whole = for_model(&ToolResult::File { content: "a\nb\n".into(), start_line: 1, end_line: 2, total_lines: 2 });
        assert_eq!(whole, "All 2 lines:\na\nb\n");
        let part = for_model(&ToolResult::File { content: "b\n".into(), start_line: 2, end_line: 2, total_lines: 9 });
        assert_eq!(part, "Lines 2-2 of 9:\nb\n");
        let empty = for_model(&ToolResult::File { content: String::new(), start_line: 0, end_line: 0, total_lines: 0 });
        assert_eq!(empty, "The file is empty.");
    }

    #[test]
    fn grep_reads_like_rg() {
        let shown = for_model(&ToolResult::GrepResults {
            matches: vec![
                hit("a.rs", 3, "fn one()", &["// one"], &["}"]),
                // Touches the first group: no separator, no repeated line.
                hit("a.rs", 5, "fn two()", &["}"], &[]),
                hit("a.rs", 20, "fn far()", &[], &[]),
                hit("b.rs", 1, "use a;", &[], &[]),
            ],
            truncated: true,
        });
        assert_eq!(
            shown,
            "a.rs\n2-// one\n3:fn one()\n4-}\n5:fn two()\n--\n20:fn far()\n\nb.rs\n1:use a;\n\n[more matches not shown — narrow the pattern or the glob]"
        );
        assert_eq!(for_model(&ToolResult::GrepResults { matches: vec![], truncated: false }), "No matches.");
    }

    #[test]
    fn a_command_leads_with_how_it_ended() {
        let out = |stdout: &str, stderr: &str, exit_code, timed_out| {
            for_model(&ToolResult::CommandRan(CommandOutput {
                stdout: stdout.into(),
                stderr: stderr.into(),
                exit_code,
                timed_out,
                truncated: false,
            }))
        };
        assert_eq!(out("ok\n", "", Some(0), false), "Exit code 0\nstdout:\nok");
        assert_eq!(out("", "boom\n", Some(1), false), "Exit code 1\nstderr:\nboom");
        assert_eq!(out("", "", None, true), "Timed out and was killed, with everything it started.\n(no output)");
        assert_eq!(out("", "", None, false), "Ended by a signal, with no exit code.\n(no output)");
    }

    #[test]
    fn a_checklist_shows_marks_ids_and_notes() {
        let task = |id: &str, title: &str, status, note: Option<&str>| Task {
            id: id.into(),
            title: title.into(),
            status,
            note: note.map(str::to_string),
        };
        let shown = for_model(&ToolResult::Todo {
            tasks: vec![
                task("1", "Read", TodoStatus::Completed, Some("found it")),
                task("2", "Fix", TodoStatus::InProgress, None),
                task("3", "Test", TodoStatus::Pending, None),
                task("4", "Docs", TodoStatus::Cancelled, Some("not asked")),
            ],
        });
        assert!(
            shown.ends_with("\n[x] 1 Read — found it\n[>] 2 Fix\n[ ] 3 Test\n[-] 4 Docs — not asked"),
            "{shown}"
        );
    }

    #[test]
    fn git_status_groups_what_changed() {
        let f = |status: &str, path: &str| GitFileStatus { status: status.into(), path: path.into() };
        let shown = for_model(&ToolResult::GitStatus {
            branch: Some("main".into()),
            staged: vec![f("R", "b.rs")],
            unstaged: vec![f("M", "a.rs"), f("?", "new.rs")],
            conflicted: vec![],
            truncated: false,
        });
        assert_eq!(shown, "On branch main\nStaged:\n  R b.rs\nNot staged:\n  M a.rs\n  ? new.rs");
        let clean = for_model(&ToolResult::GitStatus { branch: None, staged: vec![], unstaged: vec![], conflicted: vec![], truncated: false });
        assert!(clean.ends_with("\nNothing changed."), "{clean}");
    }

    #[test]
    fn blame_is_one_line_per_hunk() {
        let shown = for_model(&ToolResult::GitBlame {
            path: "a.rs".into(),
            hunks: vec![BlameHunk {
                start_line: 3,
                line_count: 2,
                commit: "abc1234".into(),
                author: "Ann".into(),
                date: "2026-09-01".into(),
                summary: "fix".into(),
            }],
            truncated: false,
        });
        assert!(shown.ends_with("\n3-4  abc1234  2026-09-01  Ann  fix"), "{shown}");
    }

    #[test]
    fn a_process_read_says_how_it_stands_and_what_was_lost() {
        let process = ProcessInfo { id: 3, command: "npm run dev".into(), cwd: ".".into(), state: ProcessState::Running };
        let shown = for_model(&ToolResult::ProcessOutput(ProcessOutput {
            process: process.clone(),
            output: "ready\n".into(),
            missed: true,
            truncated: false,
        }));
        assert_eq!(shown, "#3 `npm run dev` is running\n[some output was lost before this read]\nready");
        let quiet = for_model(&ToolResult::ProcessOutput(ProcessOutput { process, output: String::new(), missed: false, truncated: false }));
        assert!(quiet.ends_with("(no new output)"), "{quiet}");
    }

    #[test]
    fn a_search_lists_places_then_the_hint() {
        use crate::domain::code_search::MatchSource;
        let shown = for_model(&ToolResult::SearchResults {
            matches: vec![CodeMatch {
                path: "a.rs".into(),
                start_line: 1,
                end_line: 3,
                name: Some("sync".into()),
                text: "fn sync() {}\n".into(),
                source: MatchSource::Symbol,
            }],
            meta: SearchMeta { tiers_used: vec![], weak: true, hint: Some("try grep".into()) },
        });
        assert_eq!(shown, "a.rs:1-3  sync\nfn sync() {}\n\n[these matches are weak — none is a close fit]\n\ntry grep");
    }

    #[test]
    fn a_skill_lists_its_files_after_the_instructions() {
        let shown = for_model(&ToolResult::Skill { name: "tests".into(), instructions: "Do it.\n".into(), files: vec!["a.md".into()] });
        assert_eq!(shown, "Skill tests:\nDo it.\n\nFiles in this skill (read one with skill and its path):\n- a.md");
    }

    #[test]
    fn an_outline_lists_ranges_or_says_there_is_nothing() {
        let entry = OutlineEntry { name: "A.run".into(), start_line: 2, end_line: 9 };
        let shown = for_model(&ToolResult::FileOutline { path: "a.rs".into(), entries: vec![entry], total_lines: 10 });
        assert_eq!(shown, "Outline of a.rs (10 lines), start-end:\n2-9  A.run");
        let none = for_model(&ToolResult::FileOutline { path: "a.txt".into(), entries: vec![], total_lines: 4 });
        assert_eq!(none, "No declarations or headings found in a.txt (4 lines).");
    }
}
