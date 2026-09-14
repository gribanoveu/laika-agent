//! `editFile` — anchored replacements inside a file that already exists.
//!
//! Exact and all-or-nothing. Every anchor is looked up in the file's original
//! content, never in the output of an earlier edit in the same call, which is
//! what makes a batch independent of its own ordering. An anchor that matches
//! nothing, matches more than once, or overlaps another edit's region rejects
//! the entire call before anything reaches the disk.
//!
//! Alfa Atlas has a second path this does not port: when an exact match fails,
//! `fast_apply` hands the whole file and the edit's intent to the model and
//! asks it to make the change, then checks that everything outside the edited
//! region is byte-identical. It is ingenious and it turns a deterministic tool
//! into another model call, with that cost and that variance. A failed anchor
//! is already a precise, actionable error; the model can widen it and retry.

use std::fs;

use crate::domain::tools::{
    EditFileArgs, FileEdit, ReadFiles, ToolError, ToolResult, ToolScope, WriteBlocked,
};
use crate::services::text_diff;

use super::super::resolve::{relative_to_root, resolve_existing};

pub fn edit_file(
    scope: &ToolScope,
    args: &EditFileArgs,
    reads: &mut ReadFiles,
) -> Result<ToolResult, ToolError> {
    let path = resolve_existing(scope, &args.path)?;
    if !path.is_file() {
        return Err(ToolError::NotAFile(args.path.clone()));
    }
    let relative = relative_to_root(scope, &path)?;
    let content = fs::read_to_string(&path).map_err(ToolError::Io)?;

    // `false`: an anchored edit is content-addressed, so having read part of
    // the file is enough. The anchor has to be unique in the whole file, and
    // the hash proves nothing moved underneath — between them the parts the
    // agent never read are as safe as the parts it did.
    reads
        .check(&relative, &content, false)
        .map_err(|blocked| match blocked {
            WriteBlocked::NeverRead => ToolError::FileNotRead(relative.clone()),
            WriteBlocked::ReadInPart => ToolError::FileReadInPart(relative.clone()),
            WriteBlocked::ChangedSinceRead => ToolError::FileChangedSinceRead(relative.clone()),
        })?;

    let edited = apply_edits(&content, &args.edits)?;
    fs::write(&path, &edited).map_err(ToolError::Io)?;

    // `false`: editing taught the agent nothing about the parts it never read,
    // so this must not become a licence to replace the file wholesale.
    reads.record_write(&relative, &edited, false);

    Ok(ToolResult::FileEdited {
        path: relative,
        diff: text_diff::diff_stats(&content, &edited),
    })
}

/// Splices every edit into `content` at once. Nothing is applied unless all of
/// them resolve.
fn apply_edits(content: &str, edits: &[FileEdit]) -> Result<String, ToolError> {
    let ranges = exact_match_ranges(content, edits)?;

    let mut result = String::with_capacity(content.len());
    let mut cursor = 0;
    for (start, end, new) in ranges {
        result.push_str(&content[cursor..start]);
        result.push_str(new);
        cursor = end;
    }
    result.push_str(&content[cursor..]);
    Ok(result)
}

/// Resolves every anchor to a unique byte range in `content`, sorted by
/// position, having checked that none of them overlap.
fn exact_match_ranges<'a>(
    content: &str,
    edits: &'a [FileEdit],
) -> Result<Vec<(usize, usize, &'a str)>, ToolError> {
    let mut ranges: Vec<(usize, usize, &str)> = Vec::with_capacity(edits.len());
    for edit in edits {
        ranges.push(find_unique(content, &edit.old).map(|(s, e)| (s, e, edit.new.as_str()))?);
    }

    ranges.sort_by_key(|&(start, _, _)| start);
    for pair in ranges.windows(2) {
        if pair[1].0 < pair[0].1 {
            return Err(ToolError::EditsOverlap);
        }
    }
    Ok(ranges)
}

/// The anchor's single occurrence.
///
/// Reporting the count on an ambiguous match is the actionable half: it tells
/// the model the anchor was too short, instead of leaving it to guess why an
/// exact match was refused.
fn find_unique(content: &str, old: &str) -> Result<(usize, usize), ToolError> {
    let mut occurrences = content.match_indices(old);
    let Some((start, _)) = occurrences.next() else {
        return Err(ToolError::EditTextNotFound(old.to_string()));
    };
    let count = 1 + occurrences.count();
    if count > 1 {
        return Err(ToolError::EditTextAmbiguous(old.to_string(), count));
    }
    Ok((start, start + old.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::{ReadFileArgs, ToolCall};
    use crate::services::ai_tools::tools::{execute_tool, read_file::read_file};
    use crate::testing::temp_dir;
    use std::path::{Path, PathBuf};

    fn fixture(label: &str, body: &str) -> (ToolScope, PathBuf, ReadFiles) {
        let dir = temp_dir(label);
        std::fs::write(dir.join("a.txt"), body).expect("file is writable");
        let scope = ToolScope::new(&dir).expect("root resolves");
        let root = scope.root().to_path_buf();
        let mut reads = ReadFiles::default();
        agent_reads(&scope, &mut reads, "a.txt", None);
        (scope, root, reads)
    }

    fn agent_reads(scope: &ToolScope, reads: &mut ReadFiles, path: &str, range: Option<(u32, u32)>) {
        let args = ReadFileArgs {
            path: path.to_string(),
            start_line: range.map(|(s, _)| s),
            end_line: range.map(|(_, e)| e),
        };
        read_file(scope, &args, reads).expect("read succeeds");
    }

    fn edits(pairs: &[(&str, &str)]) -> EditFileArgs {
        EditFileArgs {
            path: "a.txt".to_string(),
            edits: pairs
                .iter()
                .map(|(old, new)| FileEdit {
                    old: (*old).to_string(),
                    new: (*new).to_string(),
                })
                .collect(),
        }
    }

    fn on_disk(root: &Path) -> String {
        std::fs::read_to_string(root.join("a.txt")).expect("readable")
    }

    #[test]
    fn replaces_the_anchored_text() {
        let (scope, root, mut reads) = fixture("edit-one", "let x = 1;\n");

        let result = edit_file(&scope, &edits(&[("= 1", "= 2")]), &mut reads).expect("edits");

        assert_eq!(on_disk(&root), "let x = 2;\n");
        let ToolResult::FileEdited { path, diff } = result else {
            panic!("wrong result")
        };
        assert_eq!(path, "a.txt");
        assert_eq!((diff.lines_added, diff.lines_removed), (1, 1));
    }

    /// Anchors resolve against the original content, never against an earlier
    /// edit's output — so a batch means the same thing whatever order it
    /// arrives in. Without that, two edits could silently fight.
    #[test]
    fn edits_are_independent_of_their_order() {
        let body = "alpha\nbeta\ngamma\n";
        let forwards = {
            let (scope, root, mut reads) = fixture("edit-order-fwd", body);
            edit_file(&scope, &edits(&[("alpha", "A"), ("gamma", "G")]), &mut reads).unwrap();
            on_disk(&root)
        };
        let backwards = {
            let (scope, root, mut reads) = fixture("edit-order-back", body);
            edit_file(&scope, &edits(&[("gamma", "G"), ("alpha", "A")]), &mut reads).unwrap();
            on_disk(&root)
        };

        assert_eq!(forwards, "A\nbeta\nG\n");
        assert_eq!(forwards, backwards);
    }

    /// The sharp case: one edit's replacement text *is* another edit's anchor.
    /// Applied against the original content this is unambiguous. Applied one
    /// after another it would not be — the first edit would manufacture a
    /// second occurrence of `B` and the next anchor would become ambiguous.
    /// The order-independence test above cannot tell the two apart, because on
    /// non-interfering edits they agree.
    #[test]
    fn an_anchor_never_matches_what_an_earlier_edit_produced() {
        let (scope, root, mut reads) = fixture("edit-no-cascade", "A\nB\n");

        edit_file(&scope, &edits(&[("A", "B"), ("B", "C")]), &mut reads)
            .expect("both anchors are unique in the original");

        assert_eq!(on_disk(&root), "B\nC\n");
    }

    #[test]
    fn a_missing_anchor_rejects_the_call() {
        let (scope, root, mut reads) = fixture("edit-missing", "let x = 1;\n");

        let err = edit_file(&scope, &edits(&[("= 9", "= 2")]), &mut reads).expect_err("no match");

        assert!(matches!(err, ToolError::EditTextNotFound(_)));
        assert_eq!(on_disk(&root), "let x = 1;\n", "left untouched");
    }

    /// The count is what tells the model the anchor was too short.
    #[test]
    fn an_ambiguous_anchor_reports_how_many_times_it_matched() {
        let (scope, root, mut reads) = fixture("edit-ambiguous", "x = 1;\nx = 1;\nx = 1;\n");

        let err = edit_file(&scope, &edits(&[("x = 1;", "x = 2;")]), &mut reads)
            .expect_err("three candidates");

        assert!(matches!(err, ToolError::EditTextAmbiguous(_, 3)));
        assert!(err.to_string().contains("matched 3 times"), "{err}");
        assert_eq!(on_disk(&root), "x = 1;\nx = 1;\nx = 1;\n", "left untouched");
    }

    /// Two anchors covering the same bytes would make the result depend on
    /// which was applied first, or corrupt one of them outright.
    #[test]
    fn overlapping_edits_reject_the_call() {
        let (scope, root, mut reads) = fixture("edit-overlap", "abcdef\n");

        let err = edit_file(&scope, &edits(&[("abcd", "X"), ("cdef", "Y")]), &mut reads)
            .expect_err("regions overlap");

        assert!(matches!(err, ToolError::EditsOverlap));
        assert_eq!(on_disk(&root), "abcdef\n", "left untouched");
    }

    /// All or nothing: one unusable edit in a batch must not leave the file
    /// half-changed, which is worse than either outcome.
    #[test]
    fn one_bad_edit_in_a_batch_writes_nothing() {
        let (scope, root, mut reads) = fixture("edit-atomic", "alpha\nbeta\n");

        edit_file(&scope, &edits(&[("alpha", "A"), ("nope", "N")]), &mut reads)
            .expect_err("second anchor is missing");

        assert_eq!(on_disk(&root), "alpha\nbeta\n", "the good edit did not land either");
    }

    #[test]
    fn editing_an_unread_file_is_refused() {
        let dir = temp_dir("edit-unread");
        std::fs::write(dir.join("a.txt"), "precious\n").expect("writable");
        let scope = ToolScope::new(&dir).expect("root resolves");

        let err = edit_file(&scope, &edits(&[("precious", "gone")]), &mut ReadFiles::default())
            .expect_err("never read");

        assert!(matches!(err, ToolError::FileNotRead(_)));
    }

    /// The difference from `writeFile`. An anchored edit is content-addressed:
    /// the anchor must be unique across the *whole* file and the hash proves
    /// nothing moved, so the unread parts are as safe as the read ones.
    #[test]
    fn a_partial_read_is_enough_to_edit() {
        let dir = temp_dir("edit-partial");
        let body: String = (1..=100).map(|i| format!("line {i}\n")).collect();
        std::fs::write(dir.join("a.txt"), &body).expect("writable");
        let scope = ToolScope::new(&dir).expect("root resolves");
        let root = scope.root().to_path_buf();
        let mut reads = ReadFiles::default();
        agent_reads(&scope, &mut reads, "a.txt", Some((1, 5)));

        edit_file(&scope, &edits(&[("line 3\n", "LINE 3\n")]), &mut reads)
            .expect("a partial read is enough for an anchored edit");

        assert!(on_disk(&root).contains("LINE 3\n"));
    }

    /// Editing does not teach the agent what the rest of the file says, so it
    /// must not unlock a wholesale replacement afterwards.
    #[test]
    fn editing_after_a_partial_read_does_not_unlock_replacing() {
        let dir = temp_dir("edit-no-upgrade");
        let body: String = (1..=100).map(|i| format!("line {i}\n")).collect();
        std::fs::write(dir.join("a.txt"), &body).expect("writable");
        let scope = ToolScope::new(&dir).expect("root resolves");
        let mut reads = ReadFiles::default();
        agent_reads(&scope, &mut reads, "a.txt", Some((1, 5)));
        edit_file(&scope, &edits(&[("line 3\n", "LINE 3\n")]), &mut reads).expect("edits");

        let err = super::super::write_file::write_file(
            &scope,
            &crate::domain::tools::WriteFileArgs {
                path: "a.txt".into(),
                content: "short\n".into(),
            },
            &mut reads,
        )
        .expect_err("still only ever saw five lines");

        assert!(matches!(err, ToolError::FileReadInPart(_)));
    }

    #[test]
    fn editing_over_someone_elses_change_is_refused() {
        let (scope, root, mut reads) = fixture("edit-stale", "original\n");
        std::fs::write(root.join("a.txt"), "edited by a human\n").expect("writable");

        let err = edit_file(&scope, &edits(&[("human", "robot")]), &mut reads)
            .expect_err("changed under it");

        assert!(matches!(err, ToolError::FileChangedSinceRead(_)));
        assert_eq!(on_disk(&root), "edited by a human\n", "left untouched");
    }

    /// The registry is refreshed by the write, so a follow-up edit is not
    /// mistaken for one made against a stale read.
    #[test]
    fn consecutive_edits_are_allowed() {
        let (scope, root, mut reads) = fixture("edit-twice", "one\ntwo\n");

        edit_file(&scope, &edits(&[("one", "ONE")]), &mut reads).expect("first");
        edit_file(&scope, &edits(&[("two", "TWO")]), &mut reads).expect("second");

        assert_eq!(on_disk(&root), "ONE\nTWO\n");
    }

    #[test]
    fn editing_a_file_that_does_not_exist_is_not_a_way_to_create_one() {
        let dir = temp_dir("edit-absent");
        let scope = ToolScope::new(&dir).expect("root resolves");
        assert!(matches!(
            edit_file(&scope, &edits(&[("a", "b")]), &mut ReadFiles::default()),
            Err(ToolError::NotFound(_))
        ));
    }

    #[test]
    fn a_path_leaving_the_root_is_refused() {
        let (scope, _root, mut reads) = fixture("edit-escape", "body\n");
        let outside = EditFileArgs {
            path: "../a.txt".to_string(),
            ..edits(&[("a", "b")])
        };
        assert!(matches!(
            edit_file(&scope, &outside, &mut reads),
            Err(ToolError::PathEscape(_))
        ));
    }

    /// Byte ranges come from `match_indices`, so they always land on character
    /// boundaries — splicing around a multi-byte anchor must not panic.
    #[test]
    fn a_multibyte_anchor_splices_cleanly() {
        let (scope, root, mut reads) = fixture("edit-multibyte", "привет мир\n");

        edit_file(&scope, &edits(&[("мир", "世界")]), &mut reads).expect("edits");

        assert_eq!(on_disk(&root), "привет 世界\n");
    }

    #[test]
    fn the_call_wire_shape_is_stable_and_counts_as_risky() {
        let json =
            r#"{"tool": "editFile", "args": {"path": "a.txt", "edits": [{"old": "x", "new": "y"}]}}"#;
        let call: ToolCall = serde_json::from_str(json).expect("parses");
        assert!(call.is_risky(), "an edit needs a human");

        let (scope, root, mut reads) = fixture("edit-dispatch", "x\n");
        assert!(matches!(
            execute_tool(&scope, &call, &mut reads),
            Ok(ToolResult::FileEdited { .. })
        ));
        assert_eq!(on_disk(&root), "y\n");
    }
}
