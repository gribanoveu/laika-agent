//! Tool identity, loop cost, and the approval gate.
//!
//! Ported from Alfa Atlas `domain/ai_access.rs`, which was 646 lines. Most of
//! it did not survive: `AiAccessMode` (`DocsOnly`/`FullRepo`) was specific to a
//! documentation product — here the access root is always the repository, and
//! it is enforced where paths are resolved, not by a mode enum. The per-project
//! tool allowlist went with it: its own fallback returned every variant that
//! existed, and there was no UI to narrow it, so it guarded nothing. It belongs
//! back only once something can actually configure it.
//!
//! What is left is the part that was load-bearing: which tools may not run
//! unattended, what a call costs the loop, and the wire names the model uses.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A tool's identity, known from the wire name alone — before its arguments
/// are parsed, and without borrowing them. That is what lets it key the
/// "always allow" set, the call log and the loop budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolName {
    ReadFile,
    Grep,
    ListFiles,
    WriteFile,
    EditFile,
    DeleteFile,
    CreateDirectory,
    DeleteDirectory,
    Move,
    Todo,
    GitDiff,
    GitBlame,
    GitStatus,
}

impl ToolName {
    /// Every variant. Kept by hand, and guarded by `all_is_complete` below —
    /// the round-trip and weight tests are only worth what this list covers.
    pub const ALL: &'static [ToolName] = &[
        ToolName::ReadFile,
        ToolName::Grep,
        ToolName::ListFiles,
        ToolName::WriteFile,
        ToolName::EditFile,
        ToolName::DeleteFile,
        ToolName::CreateDirectory,
        ToolName::DeleteDirectory,
        ToolName::Move,
        ToolName::Todo,
        ToolName::GitDiff,
        ToolName::GitBlame,
        ToolName::GitStatus,
    ];

    /// The name the model calls this tool by. Must match what `Serialize`
    /// produces — `serde_form_matches_wire_name` holds the two together.
    pub fn wire_name(self) -> &'static str {
        match self {
            ToolName::ReadFile => "readFile",
            ToolName::Grep => "grep",
            ToolName::ListFiles => "listFiles",
            ToolName::WriteFile => "writeFile",
            ToolName::EditFile => "editFile",
            ToolName::DeleteFile => "deleteFile",
            ToolName::CreateDirectory => "createDirectory",
            ToolName::DeleteDirectory => "deleteDirectory",
            ToolName::Move => "move",
            ToolName::Todo => "todo",
            ToolName::GitDiff => "gitDiff",
            ToolName::GitBlame => "gitBlame",
            ToolName::GitStatus => "gitStatus",
        }
    }

    /// Classifies a call before its arguments are known to parse — the loop
    /// has to decide "risky or not" on a call whose JSON may still be broken.
    pub fn from_wire_name(name: &str) -> Option<ToolName> {
        ToolName::ALL.iter().copied().find(|t| t.wire_name() == name)
    }

    /// Whether this tool changes the working tree. The default verdict fed to
    /// [`ApprovalPolicy::requires_approval`].
    ///
    /// True by identity alone, which holds for every tool here: a write is a
    /// write whatever its arguments. It stops holding at `runCommand`, where
    /// `ls` and `rm -rf /` are the same tool — which is why the gate takes the
    /// verdict as an argument instead of calling this itself.
    pub fn is_mutating(self) -> bool {
        matches!(
            self,
            ToolName::WriteFile
                | ToolName::EditFile
                | ToolName::DeleteFile
                | ToolName::CreateDirectory
                | ToolName::DeleteDirectory
                | ToolName::Move
        )
    }

    /// What one call charges against the turn's tool budget — an estimate of
    /// cost, not of risk. A round costs the sum of its calls' weights, so a
    /// cheap run of reads lives long and an expensive one is cut off early.
    pub fn loop_weight(self) -> u32 {
        match self {
            // Bare filesystem or in-memory work.
            ToolName::ReadFile
            | ToolName::ListFiles
            | ToolName::DeleteFile
            | ToolName::CreateDirectory
            | ToolName::DeleteDirectory
            | ToolName::Todo => 1,
            // A gitignore-aware walk plus a regex over many files.
            ToolName::Grep => 3,
            // Local git2 I/O plus diff/blame compaction.
            ToolName::GitDiff | ToolName::GitBlame => 2,
            // One walk of the working tree, no per-file blob reads.
            ToolName::GitStatus => 1,
            // Read, diff against the new content, write.
            ToolName::WriteFile | ToolName::EditFile => 2,
            // A rename. Atlas charged 2 because its `move` also rewrote
            // `include::`/`xref:` references in other documents; that part is
            // not ported, so the cost went with it.
            ToolName::Move => 1,
        }
    }
}

/// When the agent may act without asking.
///
/// Two independent relaxations, both narrowing to "ask about everything that
/// changes the tree" when left at their defaults.
#[derive(Debug, Clone, Default)]
pub struct ApprovalPolicy {
    /// Tools the user answered "always allow" for. Per tool, not per call.
    pub always_allowed: HashSet<ToolName>,
    /// Run the whole turn unattended. Deliberately not part of persisted
    /// settings: a saved "never ask me" is a brake released a month ago and
    /// forgotten. The turn that ran under it has to say so in its transcript —
    /// that is the loop's job, not this struct's.
    pub skip_all: bool,
}

impl ApprovalPolicy {
    /// `risky` is the call's own verdict. Every tool here answers it with
    /// [`ToolName::is_mutating`]; `runCommand` will answer it by looking at the
    /// command line. Passing it in rather than deriving it keeps this gate the
    /// single place the three relaxations compose, and spares the call sites a
    /// rewrite when the first argument-dependent tool arrives.
    pub fn requires_approval(&self, name: ToolName, risky: bool) -> bool {
        risky && !self.skip_all && !self.always_allowed.contains(&name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    /// Adding a variant without listing it in `ALL` would silently shrink every
    /// other test in this file to a subset of the enum.
    #[test]
    fn all_is_complete() {
        assert_eq!(
            ToolName::ALL.len(),
            13,
            "a variant was added or removed — update ALL and this count together"
        );
        let unique: HashSet<_> = ToolName::ALL.iter().collect();
        assert_eq!(unique.len(), ToolName::ALL.len(), "duplicate entry in ALL");
    }

    #[test]
    fn wire_names_round_trip() {
        for &tool in ToolName::ALL {
            assert_eq!(ToolName::from_wire_name(tool.wire_name()), Some(tool));
        }
    }

    #[test]
    fn wire_names_are_unique() {
        let unique: HashSet<_> = ToolName::ALL.iter().map(|t| t.wire_name()).collect();
        assert_eq!(unique.len(), ToolName::ALL.len());
    }

    /// The model is given `wire_name`, but a serialized `ToolName` is what
    /// lands in settings and the call log. If the two drifted, an "always
    /// allow" saved under one spelling would never match a call under the
    /// other, and the tool would keep asking forever.
    #[test]
    fn serde_form_matches_wire_name() {
        for &tool in ToolName::ALL {
            let json = serde_json::to_string(&tool).expect("ToolName serializes");
            assert_eq!(json, format!("\"{}\"", tool.wire_name()));
        }
    }

    #[test]
    fn unknown_wire_name_is_rejected() {
        assert_eq!(ToolName::from_wire_name("runCommand"), None);
        assert_eq!(ToolName::from_wire_name(""), None);
        assert_eq!(ToolName::from_wire_name("ReadFile"), None, "case matters");
    }

    /// A zero weight would make a round free and let the budget backstop be the
    /// only thing ending the loop.
    #[test]
    fn every_weight_is_positive() {
        for &tool in ToolName::ALL {
            assert!(tool.loop_weight() > 0, "{tool:?} costs nothing");
        }
    }

    #[test]
    fn exactly_the_writing_tools_are_mutating() {
        let mutating: HashSet<_> = ToolName::ALL
            .iter()
            .copied()
            .filter(|t| t.is_mutating())
            .collect();
        assert_eq!(
            mutating,
            HashSet::from([
                ToolName::WriteFile,
                ToolName::EditFile,
                ToolName::DeleteFile,
                ToolName::CreateDirectory,
                ToolName::DeleteDirectory,
                ToolName::Move,
            ])
        );
    }

    #[test]
    fn default_policy_asks_before_writing_and_not_before_reading() {
        let policy = ApprovalPolicy::default();
        for &tool in ToolName::ALL {
            assert_eq!(
                policy.requires_approval(tool, tool.is_mutating()),
                tool.is_mutating(),
                "{tool:?}"
            );
        }
    }

    #[test]
    fn always_allowed_covers_only_its_own_tool() {
        let policy = ApprovalPolicy {
            always_allowed: HashSet::from([ToolName::WriteFile]),
            ..ApprovalPolicy::default()
        };
        assert!(!policy.requires_approval(ToolName::WriteFile, true));
        assert!(policy.requires_approval(ToolName::DeleteFile, true));
    }

    #[test]
    fn skip_all_suppresses_every_prompt() {
        let policy = ApprovalPolicy {
            skip_all: true,
            ..ApprovalPolicy::default()
        };
        for &tool in ToolName::ALL {
            assert!(!policy.requires_approval(tool, true), "{tool:?}");
        }
    }

    /// The gate never invents risk: a tool the call itself cleared is not
    /// promoted back by the policy. This is what `runCommand` will rely on to
    /// let `ls` through while `rm -rf /` still stops the turn.
    #[test]
    fn a_call_cleared_by_its_own_verdict_never_prompts() {
        let policy = ApprovalPolicy::default();
        for &tool in ToolName::ALL {
            assert!(!policy.requires_approval(tool, false), "{tool:?}");
        }
    }

    /// The whole point of resolving the root once: a boundary still holding a
    /// `..`, or an unresolved symlink, is not a boundary. `/var` really is a
    /// symlink to `/private/var` on macOS, so this bites on a plain temp path.
    #[test]
    fn new_canonicalizes_the_root() {
        let dir = temp_dir("canonical");
        std::fs::create_dir_all(dir.join("nested")).expect("subdirectory is creatable");

        let scope = ToolScope::new(&dir.join("nested").join("..")).expect("root resolves");

        assert_eq!(scope.root(), dir.canonicalize().expect("dir resolves"));
        assert!(!scope.root().to_string_lossy().contains(".."));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A root that cannot be resolved must fail loudly rather than degrade into
    /// an unconstrained one.
    #[test]
    fn new_rejects_a_missing_root() {
        let missing = temp_dir("missing").join("no-such-directory");
        assert!(matches!(
            ToolScope::new(&missing),
            Err(ToolError::NotFound(_))
        ));
    }

    /// An empty checklist is the case that actually happened: the model tried
    /// to complete a task on a list it never created. "no task with id: x" is a
    /// dead end; the message has to name the way out.
    #[test]
    fn task_not_found_names_the_way_out_when_the_list_is_empty() {
        let err = ToolError::TaskNotFound {
            id: "t1".into(),
            available: Some(Vec::new()),
        };
        let message = err.to_string();
        assert!(message.contains("checklist is empty"), "{message}");
        assert!(message.contains(r#"op "write""#), "{message}");
    }

    #[test]
    fn task_not_found_lists_the_real_ids_when_they_are_known() {
        let err = ToolError::TaskNotFound {
            id: "t9".into(),
            available: Some(vec!["t1".into(), "t2".into()]),
        };
        assert_eq!(err.to_string(), "no task with id: t9 — current ids: t1, t2");
    }

    /// `None` means "the caller does not know the list", which must not be
    /// reported as "the list is empty" — opposite advice from the same message.
    #[test]
    fn task_not_found_claims_nothing_when_the_list_is_unknown() {
        let err = ToolError::TaskNotFound {
            id: "t9".into(),
            available: None,
        };
        assert_eq!(err.to_string(), "no task with id: t9");
    }

    /// The count is the actionable half: it tells the model the anchor was too
    /// short, rather than leaving it to guess why an exact match was refused.
    #[test]
    fn ambiguous_edit_reports_how_many_times_it_matched() {
        let err = ToolError::EditTextAmbiguous("}\n".into(), 14);
        assert!(err.to_string().contains("matched 14 times"), "{err}");
    }
}

/// The filesystem boundary a tool call executes against.
///
/// In Alfa Atlas this carried seven fields: a mode, three roots, a search
/// filter prefix, the tool allowlist, and external `@deps` roots. Six of them
/// existed to express "the model may read the whole repository but may only
/// write inside the documentation subtree" — a distinction a coding agent does
/// not have. What is left is the one thing that was always doing the work.
///
/// Constructed once per session so every tool resolves against an already
/// canonical root: a boundary compared against a path with `..` or a symlink
/// still in it is not a boundary.
#[derive(Debug, Clone)]
pub struct ToolScope {
    root: PathBuf,
}

impl ToolScope {
    /// Canonicalizes `root`. Fails if it does not exist — an access boundary
    /// that cannot be resolved must not silently become "anywhere".
    pub fn new(root: &Path) -> Result<Self, ToolError> {
        let root = root.canonicalize().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ToolError::NotFound(root.display().to_string()),
            _ => ToolError::Io(e),
        })?;
        Ok(Self { root })
    }

    /// The canonical root. Every path a tool touches has to resolve under it —
    /// enforced in one place, `services::ai_tools::resolve`, rather than by
    /// each tool remembering to check.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Why a tool call could not be carried out.
///
/// Data all the way to the boundary, per `AGENTS.md`: the string form exists
/// only because the model reads it. Every message is addressed to the model and
/// says what to do differently, not merely what went wrong.
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("path escapes tool root: {0}")]
    PathEscape(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("not a file: {0}")]
    NotAFile(String),
    #[error("io error: {0}")]
    Io(#[source] std::io::Error),
    /// A `listFiles` `pattern` that does not compile as a glob.
    #[error("invalid glob pattern: {0}")]
    InvalidPattern(String),
    /// An `editFile` edit's `old` text appears nowhere in the file.
    #[error("edit text not found: {0}")]
    EditTextNotFound(String),
    /// An `editFile` edit's `old` text appears more than once — which
    /// occurrence was meant is unknowable, so nothing is written. `.1` is the
    /// match count, so the model learns how much more context to include.
    #[error("edit text is not unique — matched {1} times: {0}")]
    EditTextAmbiguous(String, usize),
    /// Two edits in one call matched overlapping regions of the original
    /// content: applying both would be order-dependent or would corrupt one of
    /// them, so the whole call is rejected.
    #[error("edits overlap in the same region of the file")]
    EditsOverlap,
    /// `deleteDirectory` without `recursive` against a directory with contents.
    #[error("directory is not empty: {0}")]
    DirectoryNotEmpty(String),
    /// A `move` whose destination exists. Nothing is overwritten — the check
    /// happens before the rename, not after.
    #[error("already exists: {0}")]
    AlreadyExists(String),
    /// A wire name matching no known tool.
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    /// Arguments that did not deserialize into the struct their tool expects.
    ///
    /// `reason` is JSON-path-annotated (`edits[1]: missing field \`old\``)
    /// rather than a byte offset into the raw JSON. The failure this exists for
    /// is a model getting one field name wrong deep inside an array argument:
    /// "line 1 column 7275" does not tell it which element to fix, a path does.
    #[error("invalid arguments for {tool}: {reason}")]
    InvalidArguments { tool: String, reason: String },
    /// A `todo write` whose new titles would push the list past its maximum —
    /// rejected outright rather than silently truncated, so the model decides
    /// what to drop or split instead of discovering later that it was cut.
    #[error("todo list already has {current} task(s); adding {adding} more would exceed the {max} maximum")]
    TooManyTasks {
        current: usize,
        adding: usize,
        max: usize,
    },
    /// A `todo update` naming an id that is not in the list — usually a stale
    /// id from earlier in the conversation, or a checklist the model never
    /// created before trying to complete a task on it.
    ///
    /// `available` is `Some` when the caller knows the whole list, so the
    /// message can name the way out instead of leaving a dead end.
    #[error("{}", task_not_found_message(id, available))]
    TaskNotFound {
        id: String,
        available: Option<Vec<String>>,
    },
    /// A git read failed. Carried as a string so git types stay out of the
    /// tool boundary.
    #[error("git error: {0}")]
    Git(String),
}

fn task_not_found_message(id: &str, available: &Option<Vec<String>>) -> String {
    match available {
        Some(ids) if ids.is_empty() => format!(
            "no task with id: {id} — the checklist is empty, create tasks with op \"write\" before updating one"
        ),
        Some(ids) => format!("no task with id: {id} — current ids: {}", ids.join(", ")),
        None => format!("no task with id: {id}"),
    }
}

/// A parsed, validated call the model asked for.
///
/// Adjacently tagged (`{"tool": "readFile", "args": {…}}`) rather than a set
/// of functions, which buys one place to check permissions, one place to
/// serialize at the model boundary, one place to log and redact, and a wire
/// shape a test can pin. Adding a tool is a variant here, a variant in
/// [`ToolResult`], a module under `services::ai_tools::tools`, and an arm in
/// the dispatcher — nowhere else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "tool", content = "args", rename_all = "camelCase")]
pub enum ToolCall {
    ReadFile(ReadFileArgs),
    Grep(GrepArgs),
    ListFiles(ListFilesArgs),
}

impl ToolCall {
    pub fn name(&self) -> ToolName {
        match self {
            ToolCall::ReadFile(_) => ToolName::ReadFile,
            ToolCall::Grep(_) => ToolName::Grep,
            ToolCall::ListFiles(_) => ToolName::ListFiles,
        }
    }

    /// Whether *this* call needs a human before it runs — the verdict
    /// [`ApprovalPolicy::requires_approval`] takes.
    ///
    /// Every tool so far answers from its identity alone. `runCommand` will be
    /// the first to answer from its arguments, which is why this is a method on
    /// the call rather than a lookup on the name.
    pub fn is_risky(&self) -> bool {
        self.name().is_mutating()
    }
}

/// What a settled call gives back to the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "camelCase")]
pub enum ToolResult {
    #[serde(rename_all = "camelCase")]
    File {
        content: String,
        /// 1-indexed and inclusive — the range actually returned after
        /// clamping, not necessarily the one asked for. Both are `0` for an
        /// empty file: there is no line 1 to claim.
        start_line: u32,
        end_line: u32,
        total_lines: u32,
    },
    /// `truncated` means "there are more hits than these" — the cap was
    /// reached with matching still to do. A search that quietly stopped at a
    /// limit reads to the model as an exhaustive answer, which is the one
    /// thing `grep` is for.
    #[serde(rename_all = "camelCase")]
    GrepResults {
        matches: Vec<GrepMatch>,
        truncated: bool,
    },
    /// `truncated` carries the same "there is more here than you are seeing"
    /// contract as `GrepResults`. It matters most on a vendored tree: a real
    /// `node_modules` is tens of thousands of entries, and a listing that
    /// quietly stopped at a cap reads to the model as the whole picture.
    #[serde(rename_all = "camelCase")]
    FileList {
        entries: Vec<ToolFileEntry>,
        truncated: bool,
    },
}

/// `readFile` arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReadFileArgs {
    /// Path relative to the scope root.
    pub path: String,
    /// 1-indexed, inclusive. `None` reads from the start. Out-of-range values
    /// are clamped rather than rejected.
    #[serde(default, deserialize_with = "crate::domain::flexible_args::opt_u32")]
    pub start_line: Option<u32>,
    /// 1-indexed, inclusive. `None` reads through the end.
    #[serde(default, deserialize_with = "crate::domain::flexible_args::opt_u32")]
    pub end_line: Option<u32>,
}

/// `grep` arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GrepArgs {
    /// Rust `regex` syntax — no backreferences, no lookaround.
    pub pattern: String,
    /// A file or subdirectory to search under, relative to the scope root.
    /// `None`, `""` or `"."` searches everything.
    pub path: Option<String>,
    /// Glob over the file *name* only, not the path.
    pub glob: Option<String>,
    /// Default false — an exact, case-sensitive match unless asked otherwise.
    #[serde(default, deserialize_with = "crate::domain::flexible_args::opt_bool")]
    pub case_insensitive: Option<bool>,
    #[serde(default, deserialize_with = "crate::domain::flexible_args::opt_usize")]
    pub max_results: Option<usize>,
    /// Lines of context around each hit. `None`/`0` returns the matching line
    /// alone. Exists for the model: without it every "what does this line
    /// actually do" costs a follow-up `readFile` round trip.
    #[serde(default, deserialize_with = "crate::domain::flexible_args::opt_usize")]
    pub context_lines: Option<usize>,
}

/// One line hit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrepMatch {
    /// Relative to the scope root, `/`-separated — the same spelling
    /// `readFile` takes, so a hit round-trips without editing.
    pub path: String,
    /// 1-indexed.
    pub line: u32,
    pub text: String,
    /// Lines before the hit, oldest first. Omitted from the wire form when
    /// empty, so a plain search's shape is unchanged by the feature existing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
}

/// `listFiles` arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListFilesArgs {
    /// Subdirectory relative to the scope root; `None`/`"."` lists the root.
    pub path: Option<String>,
    /// Levels below `path`, following the walker's convention: `path` itself
    /// is depth 0, its direct children depth 1. `Some(0)` is valid and means
    /// no descendants — not an error. `None` is unlimited.
    #[serde(default, deserialize_with = "crate::domain::flexible_args::opt_u32")]
    pub depth: Option<u32>,
    /// Glob over each entry's file *name*, never its full path, so `"*.rs"`
    /// matches at any depth. Directories are kept regardless: this scopes
    /// which files come back, not the navigable structure.
    pub pattern: Option<String>,
}

/// One listing entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolFileEntry {
    /// Relative to the scope root, `/`-separated — the spelling `readFile`
    /// takes, so an entry round-trips without editing.
    pub path: String,
    pub is_dir: bool,
}
