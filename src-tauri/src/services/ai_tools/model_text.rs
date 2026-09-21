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

use std::collections::BTreeMap;

use crate::domain::background::ProcessOutput;
use crate::domain::code_search::{CodeMatch, SearchMeta};
use crate::domain::command_exec::CommandOutput;
use crate::domain::tools::{BlameHunk, GitFileDiff, GitFileStatus, GrepMatch, OutlineEntry, Task, TodoStatus, ToolResult};
use crate::services::ai_tools::tools::list_files::render_file_tree;
use crate::services::text_diff::render_for_model;

pub fn for_model(result: &ToolResult) -> String {
    match result {
        ToolResult::File { content, start_line, end_line, total_lines } => file(content, *start_line, *end_line, *total_lines),
        ToolResult::FileOutline { path, entries, total_lines } => outline(path, entries, *total_lines),
        ToolResult::GrepResults { matches, truncated } => grep(matches, *truncated),
        ToolResult::FileList { entries, .. } if entries.is_empty() => "No files found.".to_string(),
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
        ToolResult::GitDiffFiles { path, label, files, truncated } => diff_files(path, label, files, *truncated),
        ToolResult::GitBlame { path, hunks, truncated } => blame(path, hunks, *truncated),
        ToolResult::CommandRan(output) => command(output),
        // The folder before the command: last, a `.` folder ran into the
        // sentence's own full stop.
        ToolResult::ProcessStarted(process) => format!(
            "Started background process #{} in `{}`: `{}`. Read what it writes with readOutput.",
            process.id, process.cwd, process.command
        ),
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
///
/// Each line is printed once. Two hits whose context windows overlap share
/// their lines, and a line that is itself a hit is shown as one even when it
/// also falls in another hit's context.
fn grep(matches: &[GrepMatch], truncated: bool) -> String {
    if matches.is_empty() {
        return "No matches. Git-ignored paths (build output, dependencies) are not searched — use runCommand to search them.".to_string();
    }
    // Files in the order they came, each with its lines by number.
    let mut files: Vec<(&str, BTreeMap<u32, (bool, &str)>)> = Vec::new();
    for m in matches {
        if files.last().is_none_or(|(path, _)| *path != m.path) {
            files.push((&m.path, BTreeMap::new()));
        }
        let Some((_, lines)) = files.last_mut() else { continue };
        let first = m.line.saturating_sub(m.before.len() as u32);
        for (k, text) in m.before.iter().enumerate() {
            lines.entry(first + k as u32).or_insert((false, text));
        }
        lines.insert(m.line, (true, &m.text));
        for (k, text) in m.after.iter().enumerate() {
            lines.entry(m.line + 1 + k as u32).or_insert((false, text));
        }
    }
    let mut blocks = Vec::new();
    for (path, lines) in files {
        let mut out = path.to_string();
        let mut previous: Option<u32> = None;
        for (n, (hit, text)) in lines {
            if previous.is_some_and(|p| n > p + 1) {
                out.push_str("\n--");
            }
            out.push_str(&format!("\n{n}{}{text}", if hit { ':' } else { '-' }));
            previous = Some(n);
        }
        blocks.push(out);
    }
    let hits = matches.len();
    let files_count = blocks.len();
    let head = format!(
        "{}{hits} {} in {files_count} {}:",
        if truncated { "At least " } else { "" },
        if hits == 1 { "match" } else { "matches" },
        if files_count == 1 { "file" } else { "files" },
    );
    let mut out = format!("{head}\n{}", blocks.join("\n\n"));
    if truncated {
        out.push_str("\n\n[more matches not shown — narrow the pattern or the glob]");
    }
    out
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

/// A directory's change: the totals, then each file the way a single-file
/// diff reads. A file left without its text says how to get it.
fn diff_files(path: &str, label: &str, files: &[GitFileDiff], truncated: bool) -> String {
    if files.is_empty() {
        return format!("No changes under {path} ({label}).");
    }
    let (added, removed) = files.iter().fold((0, 0), |(a, r), f| (a + f.diff.lines_added, r + f.diff.lines_removed));
    let count = files.len();
    let mut out = format!(
        "Diff ({label}) under {path}: {count} {} changed (+{added} -{removed} lines)",
        if count == 1 { "file" } else { "files" }
    );
    for file in files {
        out.push_str("\n\n");
        if file.is_binary {
            out.push_str(&format!("{} is a binary file — no text diff", file.path));
        } else if file.diff.truncated && file.diff.unified_diff.is_empty() {
            out.push_str(&render_for_model("File", &file.path, &file.diff, false));
            out.push_str(" — diff not shown here, ask gitDiff for this file");
        } else {
            out.push_str(&render_for_model("File", &file.path, &file.diff, true));
        }
    }
    if truncated {
        out.push_str("\n\n[more changed files not shown — ask for a narrower path]");
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
    let took = took(output.duration_ms);
    let head = match (output.timed_out, output.exit_code) {
        (true, _) => format!("Timed out{took} and was killed, with everything it started."),
        (false, Some(code)) => format!("Exit code {code}{took}"),
        (false, None) => format!("Ended by a signal{took}, with no exit code."),
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

/// ` after 2.3 s`; nothing when the time is not known.
fn took(ms: u64) -> String {
    match ms {
        0 => String::new(),
        1..=999 => format!(" after {ms} ms"),
        _ => format!(" after {:.1} s", ms as f64 / 1000.0),
    }
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
    use crate::domain::tools::FileDiffStats;

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
    fn an_empty_listing_says_so_instead_of_drawing_a_bare_root() {
        assert_eq!(for_model(&ToolResult::FileList { entries: vec![], truncated: false }), "No files found.");
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
            "At least 4 matches in 2 files:\na.rs\n2-// one\n3:fn one()\n4-}\n5:fn two()\n--\n20:fn far()\n\nb.rs\n1:use a;\n\n[more matches not shown — narrow the pattern or the glob]"
        );
        let none = for_model(&ToolResult::GrepResults { matches: vec![], truncated: false });
        assert!(none.starts_with("No matches. Git-ignored paths"), "{none}");
    }

    /// Hits on 118 and 120 with two lines of context: 120 is both the first
    /// hit's context and the second hit. Each line once, 120 as the hit.
    #[test]
    fn overlapping_context_prints_each_line_once() {
        let shown = for_model(&ToolResult::GrepResults {
            matches: vec![
                hit("t.java", 118, "retry()", &["a", "b"], &["c", "retry()"]),
                hit("t.java", 120, "retry()", &["retry()", "c"], &["d", "e"]),
            ],
            truncated: false,
        });
        assert_eq!(shown, "2 matches in 1 file:\nt.java\n116-a\n117-b\n118:retry()\n119-c\n120:retry()\n121-d\n122-e");
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
                duration_ms: 0,
            }))
        };
        assert_eq!(out("ok\n", "", Some(0), false), "Exit code 0\nstdout:\nok");
        assert_eq!(out("", "boom\n", Some(1), false), "Exit code 1\nstderr:\nboom");
        assert_eq!(out("", "", None, true), "Timed out and was killed, with everything it started.\n(no output)");
        assert_eq!(out("", "", None, false), "Ended by a signal, with no exit code.\n(no output)");
    }

    /// The model has no clock: how long a run took is worth a few words.
    #[test]
    fn a_command_says_how_long_it_took() {
        let ran = |duration_ms, timed_out, exit_code| {
            for_model(&ToolResult::CommandRan(CommandOutput { duration_ms, timed_out, exit_code, ..CommandOutput::default() }))
        };
        assert!(ran(2345, false, Some(0)).starts_with("Exit code 0 after 2.3 s\n"));
        assert!(ran(40, false, Some(1)).starts_with("Exit code 1 after 40 ms\n"));
        assert!(ran(600_000, true, None).starts_with("Timed out after 600.0 s and was killed"));
    }

    /// `is running in ..` read as a typo: the folder is not last any more.
    #[test]
    fn a_background_start_reads_as_a_sentence() {
        let process = ProcessInfo { id: 1, command: "npm run dev".into(), cwd: ".".into(), state: ProcessState::Running };
        assert_eq!(
            for_model(&ToolResult::ProcessStarted(process)),
            "Started background process #1 in `.`: `npm run dev`. Read what it writes with readOutput."
        );
    }

    /// Totals first, then each file as a single-file diff reads; a file whose
    /// text did not fit says how to get it, and a binary one says what it is.
    #[test]
    fn a_directory_diff_reads_file_by_file() {
        use crate::services::text_diff::diff_stats;
        let file = |path: &str, diff, is_binary| GitFileDiff { path: path.into(), diff, is_binary };
        let mut cut = diff_stats("", "x\n");
        cut.unified_diff.clear();
        cut.truncated = true;
        let shown = for_model(&ToolResult::GitDiffFiles {
            path: "src".into(),
            label: "index → working tree".into(),
            files: vec![
                file("src/a.rs", diff_stats("one\n", "two\n"), false),
                file("src/big.rs", cut, false),
                file("src/logo.png", FileDiffStats::default(), true),
            ],
            truncated: true,
        });
        assert_eq!(
            shown,
            "Diff (index → working tree) under src: 3 files changed (+2 -1 lines)\n\n\
             File src/a.rs (+1 -1 lines)\n```diff\n@@ -1 +1 @@\n-one\n+two\n```\n\n\
             File src/big.rs (+1 -0 lines) — diff not shown here, ask gitDiff for this file\n\n\
             src/logo.png is a binary file — no text diff\n\n\
             [more changed files not shown — ask for a narrower path]"
        );
        let none = for_model(&ToolResult::GitDiffFiles { path: "src".into(), label: "x".into(), files: vec![], truncated: false });
        assert_eq!(none, "No changes under src (x).");
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
