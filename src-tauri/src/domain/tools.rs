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

use serde::{Deserialize, Serialize};

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
}
