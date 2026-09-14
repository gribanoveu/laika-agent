//! Line-level diffing for the file-mutating tools.
//!
//! The decision that matters is not the algorithm but where the result goes:
//! the diff is returned **to the UI and back to the model**. Without it the
//! model sees `{"path": "…"}` and has no idea what actually landed on disk.

use similar::{ChangeTag, TextDiff};

use crate::domain::tools::FileDiffStats;

/// Caps the rendered diff so neither the model's context nor a detail view has
/// to carry an unbounded one. The counts stay exact regardless.
pub const MAX_UNIFIED_DIFF_CHARS: usize = 6000;

/// Diffs `old` against `new`. A brand-new file or a deleted one needs no
/// special case — pass `""` for the side that does not exist and the result
/// comes out as all-added or all-removed.
pub fn diff_stats(old: &str, new: &str) -> FileDiffStats {
    let diff = TextDiff::from_lines(old, new);

    let (mut lines_added, mut lines_removed) = (0u32, 0u32);
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => lines_added += 1,
            ChangeTag::Delete => lines_removed += 1,
            ChangeTag::Equal => {}
        }
    }

    let full = diff.unified_diff().context_radius(2).to_string();
    let (unified_diff, truncated) = truncate_on_line_boundary(&full, MAX_UNIFIED_DIFF_CHARS);

    FileDiffStats {
        lines_added,
        lines_removed,
        unified_diff,
        truncated,
    }
}

/// Cuts to at most `max_chars`, never mid-line.
///
/// A diff cut mid-line reads as corrupted rather than merely incomplete. And
/// since this is arbitrary UTF-8 file content, a raw byte-index slice would
/// panic on a multi-byte character boundary.
fn truncate_on_line_boundary(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }
    let mut kept = String::new();
    let mut kept_chars = 0usize;
    for line in text.split_inclusive('\n') {
        let line_chars = line.chars().count();
        if kept_chars + line_chars > max_chars {
            break;
        }
        kept.push_str(line);
        kept_chars += line_chars;
    }
    (kept, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_content_has_no_changes() {
        let stats = diff_stats("foo\nbar\n", "foo\nbar\n");
        assert_eq!((stats.lines_added, stats.lines_removed), (0, 0));
        assert_eq!(stats.unified_diff, "");
        assert!(!stats.truncated);
    }

    #[test]
    fn a_new_file_is_all_additions() {
        let stats = diff_stats("", "foo\nbar\nbaz\n");
        assert_eq!((stats.lines_added, stats.lines_removed), (3, 0));
        assert!(stats.unified_diff.contains("+foo"));
    }

    #[test]
    fn a_deleted_file_is_all_removals() {
        let stats = diff_stats("foo\nbar\n", "");
        assert_eq!((stats.lines_added, stats.lines_removed), (0, 2));
        assert!(stats.unified_diff.contains("-foo"));
    }

    #[test]
    fn a_replacement_counts_both_sides() {
        let stats = diff_stats("foo\nbar\nbaz\n", "foo\nqux\nbaz\n");
        assert_eq!((stats.lines_added, stats.lines_removed), (1, 1));
    }

    /// The counts must stay true even when the rendered diff was cut — they
    /// are what the `+N −M` badge shows and what the model reads as "how big
    /// was this change".
    #[test]
    fn a_large_diff_is_cut_on_a_line_boundary_without_losing_the_counts() {
        let new: String = (0..2000).map(|i| format!("line {i}\n")).collect();
        let stats = diff_stats("", &new);

        assert!(stats.truncated);
        assert!(stats.unified_diff.chars().count() <= MAX_UNIFIED_DIFF_CHARS);
        assert!(stats.unified_diff.ends_with('\n'), "never cut mid-line");
        assert_eq!(stats.lines_added, 2000, "the count is the true total");
    }

    /// Byte-slicing this would panic partway through a character.
    #[test]
    fn a_multibyte_diff_is_cut_safely() {
        let new: String = (0..2000).map(|i| format!("строка {i}\n")).collect();
        let stats = diff_stats("", &new);
        assert!(stats.truncated);
        assert!(stats.unified_diff.ends_with('\n'));
    }
}
