//! Writing a commit message from what is staged: what the model is shown, and
//! what is kept of its reply. The request itself is `services::commit_message`.
//!
//! The convention is not configured anywhere. The repository's own recent
//! subjects are the example — `feat(KEY-123): …` in one project, `area: …` in
//! another — and the model copies them. The one thing it is not left to guess
//! is the ticket key: that comes from the branch name, or there is none.

use regex::Regex;
use thiserror::Error;

use crate::domain::git_changes::GitChangesError;
use crate::domain::llm::LlmError;
use crate::domain::project_rules::RuleFile;

/// How much of the staged patch the model sees. Past it a file is named with
/// its counts only: the subject is one line, and a diff the size of the
/// window buys nothing a list of files does not.
pub const MAX_PATCH_CHARS: usize = 12_000;

/// How many recent subjects are shown as the convention to follow.
pub const RECENT_SUBJECTS: usize = 15;

/// Files whose diff says nothing a person wrote: named, never shown.
const GENERATED: &[&str] = &[
    "Cargo.lock",
    "bun.lock",
    "bun.lockb",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "poetry.lock",
    "Gemfile.lock",
    "go.sum",
];

pub const INSTRUCTIONS: &str = "\
You write git commit messages. Reply with the message only: no code fence, no quotes, no commentary.

Format: a subject line, a blank line, then a short body.
- Subject: at most 72 characters, imperative mood, no trailing period. Follow the convention of the repository's recent subjects exactly: prefix style (`type(scope):`, `area:`, a ticket key), casing and language. With no recent subjects, use Conventional Commits, `type(scope): subject`, with the ticket key as the scope when one is given.
- Ticket: when a ticket key is given, put it where the convention puts it. When the convention has no place for one, leave it out. Never invent a ticket key or copy one from an old subject.
- Body: one to three short lines on what changed and why, wrapped at 72 characters. Leave it out when the subject already says everything.
- Describe only the staged changes shown.";

/// One staged file. `patch` is `None` for a binary file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedPatch {
    pub path: String,
    pub add: usize,
    pub del: usize,
    pub patch: Option<String>,
}

/// Everything the request is written from.
pub struct CommitContext<'a> {
    pub staged: &'a [StagedPatch],
    /// Newest first.
    pub recent: &'a [String],
    pub ticket: Option<&'a str>,
    /// What the user already typed in the message box.
    pub draft: &'a str,
}

#[derive(Debug, Error)]
pub enum CommitMessageError {
    #[error(transparent)]
    Git(#[from] GitChangesError),
    #[error(transparent)]
    Llm(#[from] LlmError),
    #[error("the model returned an empty message")]
    EmptyReply,
}

/// A Jira-style key in a branch name: `feature/KEY-123-login` → `KEY-123`.
/// Upper case only, so `release-2` is not read as a ticket.
pub fn ticket_from_branch(branch: &str) -> Option<String> {
    let re = Regex::new(r"(?:^|[^A-Za-z0-9])([A-Z][A-Z0-9]+-[0-9]+)").ok()?;
    Some(re.captures(branch)?.get(1)?.as_str().to_string())
}

/// The system message: the instructions, then the project's own rules — they
/// may say how commits are written there.
pub fn system_prompt(rules: &[RuleFile]) -> String {
    let mut out = INSTRUCTIONS.to_string();
    if !rules.is_empty() {
        out.push_str("\n\nProject instructions follow. Obey any rule they give about commit messages and ignore the rest.");
        for rule in rules {
            out.push_str(&format!("\n\n## {}\n\n{}", rule.name, rule.content));
        }
    }
    out
}

/// The user message: ticket, convention, draft, then the staged changes.
pub fn render_request(ctx: &CommitContext) -> String {
    let mut out = match ctx.ticket {
        Some(key) => format!("Ticket key: {key}\n"),
        None => "Ticket key: none, so do not write one.\n".to_string(),
    };

    if ctx.recent.is_empty() {
        out.push_str("\nNo commits yet.\n");
    } else {
        out.push_str("\nRecent commit subjects, newest first:\n");
        for subject in ctx.recent {
            out.push_str(&format!("- {subject}\n"));
        }
    }

    let draft = ctx.draft.trim();
    if !draft.is_empty() {
        out.push_str(&format!("\nThe user's draft; keep its intent:\n{draft}\n"));
    }

    out.push_str("\nStaged files:\n");
    for file in ctx.staged {
        out.push_str(&format!("{} +{} -{}\n", file.path, file.add, file.del));
    }

    let mut budget = MAX_PATCH_CHARS;
    let mut omitted = Vec::new();
    out.push_str("\nStaged diff:\n");
    for file in ctx.staged {
        match &file.patch {
            Some(patch) if !is_generated(&file.path) && patch.len() <= budget => {
                budget -= patch.len();
                out.push_str(&format!("\n--- {}\n{}", file.path, patch));
            }
            _ => omitted.push(file.path.as_str()),
        }
    }
    if !omitted.is_empty() {
        out.push_str(&format!("\nDiff not shown (generated, binary or too large): {}\n", omitted.join(", ")));
    }
    out
}

fn is_generated(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    GENERATED.contains(&name)
}

/// The message as it goes into the box: the fence or quotes some models wrap
/// it in taken off, and a blank line after the subject when the body follows
/// it directly. `None` when nothing is left.
pub fn clean_reply(reply: &str) -> Option<String> {
    let mut text = reply.trim();
    if let Some(rest) = text.strip_prefix("```") {
        // The fence's own line may name a language.
        text = rest.split_once('\n').map_or("", |(_, body)| body);
        text = text.trim_end().strip_suffix("```").unwrap_or(text).trim();
    }
    // Only a quote that wraps the whole text: "`Dropdown`: close on `Escape`"
    // starts and ends with one and is not quoted.
    for quote in ['"', '`'] {
        if let Some(inner) = text.strip_prefix(quote).and_then(|t| t.strip_suffix(quote)) {
            if !inner.contains(quote) {
                text = inner.trim();
            }
        }
    }
    if text.is_empty() {
        return None;
    }

    let mut lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    if lines.len() > 1 && !lines[1].is_empty() {
        lines.insert(1, "");
    }
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged(path: &str, patch: Option<&str>) -> StagedPatch {
        StagedPatch { path: path.into(), add: 1, del: 0, patch: patch.map(String::from) }
    }

    fn ctx<'a>(staged: &'a [StagedPatch], recent: &'a [String], ticket: Option<&'a str>) -> CommitContext<'a> {
        CommitContext { staged, recent, ticket, draft: "" }
    }

    #[test]
    fn a_ticket_is_read_from_the_branch() {
        assert_eq!(ticket_from_branch("feature/KEY-123-login").as_deref(), Some("KEY-123"));
        assert_eq!(ticket_from_branch("KEY-7").as_deref(), Some("KEY-7"));
        assert_eq!(ticket_from_branch("bugfix/AB2-40_crash").as_deref(), Some("AB2-40"));
    }

    #[test]
    fn a_branch_without_a_key_has_none() {
        assert_eq!(ticket_from_branch("main"), None);
        assert_eq!(ticket_from_branch("release-2"), None);
        assert_eq!(ticket_from_branch("feature/K-1"), None);
        assert_eq!(ticket_from_branch("fooKEY-1"), None);
    }

    #[test]
    fn the_ticket_or_its_absence_is_stated() {
        let files = [staged("a.rs", Some("+x\n"))];
        assert!(render_request(&ctx(&files, &[], Some("KEY-9"))).starts_with("Ticket key: KEY-9\n"));
        assert!(render_request(&ctx(&files, &[], None)).contains("none, so do not write one"));
    }

    #[test]
    fn recent_subjects_are_the_convention() {
        let files = [staged("a.rs", Some("+x\n"))];
        let recent = ["feat(KEY-1): add login".to_string(), "fix(auth): expire tokens".to_string()];
        let text = render_request(&ctx(&files, &recent, None));
        assert!(text.contains("- feat(KEY-1): add login\n- fix(auth): expire tokens\n"));
        assert!(render_request(&ctx(&files, &[], None)).contains("No commits yet."));
    }

    #[test]
    fn the_draft_is_passed_on_and_an_empty_one_is_not() {
        let files = [staged("a.rs", Some("+x\n"))];
        let with = CommitContext { draft: "  fix login \n", ..ctx(&files, &[], None) };
        assert!(render_request(&with).contains("keep its intent:\nfix login\n"));
        assert!(!render_request(&ctx(&files, &[], None)).contains("draft"));
    }

    #[test]
    fn lockfiles_binaries_and_what_is_past_the_budget_are_named_not_shown() {
        let big = "+".repeat(MAX_PATCH_CHARS - 10);
        let files = [
            staged("src/a.rs", Some(&big)),
            staged("src/b.rs", Some("+second file\n")),
            staged("src/c.rs", Some("+ok\n")),
            staged("web/bun.lock", Some("+lock\n")),
            staged("logo.png", None),
        ];
        let text = render_request(&ctx(&files, &[], None));
        assert!(text.contains(&format!("--- src/a.rs\n{big}")));
        // Every file is listed with its counts, shown or not.
        assert!(text.contains("src/b.rs +1 -0\n") && text.contains("logo.png +1 -0\n"));
        assert!(!text.contains("+second file"));
        // A later, smaller file still fits in what is left.
        assert!(text.contains("--- src/c.rs\n+ok\n"));
        assert!(!text.contains("+lock"));
        assert!(text.contains("Diff not shown (generated, binary or too large): src/b.rs, web/bun.lock, logo.png\n"));
    }

    #[test]
    fn a_patch_exactly_at_the_budget_is_shown() {
        let exact = "+".repeat(MAX_PATCH_CHARS);
        let files = [staged("a.rs", Some(&exact))];
        assert!(!render_request(&ctx(&files, &[], None)).contains("Diff not shown"));
    }

    #[test]
    fn rules_follow_the_instructions_only_when_there_are_any() {
        assert_eq!(system_prompt(&[]), INSTRUCTIONS);
        let rules = [RuleFile::new("AGENTS.md", "Commits: type(KEY): text")];
        let prompt = system_prompt(&rules);
        assert!(prompt.starts_with(INSTRUCTIONS));
        assert!(prompt.ends_with("## AGENTS.md\n\nCommits: type(KEY): text"));
    }

    #[test]
    fn a_fence_and_quotes_are_taken_off() {
        assert_eq!(clean_reply("```text\nfeat: x\n\nbody\n```\n").as_deref(), Some("feat: x\n\nbody"));
        assert_eq!(clean_reply("```\nfix: y\n```").as_deref(), Some("fix: y"));
        assert_eq!(clean_reply("\"fix: y\"").as_deref(), Some("fix: y"));
        assert_eq!(clean_reply("`Dropdown`: close on `Escape`").as_deref(), Some("`Dropdown`: close on `Escape`"));
        assert_eq!(clean_reply("  fix: y  \n").as_deref(), Some("fix: y"));
    }

    #[test]
    fn the_body_is_set_off_from_the_subject() {
        assert_eq!(clean_reply("feat: x\nwhy it matters").as_deref(), Some("feat: x\n\nwhy it matters"));
        assert_eq!(clean_reply("feat: x\n\nwhy").as_deref(), Some("feat: x\n\nwhy"));
    }

    #[test]
    fn nothing_left_is_no_message() {
        assert_eq!(clean_reply("   "), None);
        assert_eq!(clean_reply("```\n```"), None);
        assert_eq!(clean_reply("\"\""), None);
    }
}
