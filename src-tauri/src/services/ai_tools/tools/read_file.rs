//! `readFile` — one file's text, optionally narrowed to a line range.
//!
//! An out-of-range range is clamped rather than rejected: the model is
//! guessing line numbers from an earlier search hit, and a hard error would
//! cost a round trip just to learn the file got shorter.
//!
//! No extension filter. The boundary at the tool layer is containment under
//! the scope root and nothing else — a coding agent has to read source files,
//! build manifests, lockfiles and dotfiles alike.

use std::fs;

use crate::domain::tools::{ReadFileArgs, ToolError, ToolResult, ToolScope};

use super::super::resolve::resolve_existing;

pub fn read_file(scope: &ToolScope, args: &ReadFileArgs) -> Result<ToolResult, ToolError> {
    let path = resolve_existing(scope, &args.path)?;
    if !path.is_file() {
        return Err(ToolError::NotAFile(args.path.clone()));
    }
    let content = fs::read_to_string(&path).map_err(ToolError::Io)?;
    Ok(slice_lines(content, args.start_line, args.end_line))
}

/// Clamps the requested range into the file rather than erroring.
///
/// With neither bound requested, `content` is handed back byte-identical to
/// what was read — no split-and-rejoin round trip for the common whole-file
/// case, which would also silently rewrite the file's final newline.
///
/// An empty file reports `0`/`0`/`0`. When `end_line` clamps below
/// `start_line` after each is independently clamped into `[1, total_lines]`,
/// `end_line` rises to `start_line` and one line comes back — still an answer,
/// where an error would only have cost a round trip.
fn slice_lines(content: String, start_line: Option<u32>, end_line: Option<u32>) -> ToolResult {
    if start_line.is_none() && end_line.is_none() {
        let total_lines = content.lines().count() as u32;
        return ToolResult::File {
            content,
            start_line: if total_lines == 0 { 0 } else { 1 },
            end_line: total_lines,
            total_lines,
        };
    }

    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len() as u32;
    if total_lines == 0 {
        return ToolResult::File {
            content: String::new(),
            start_line: 0,
            end_line: 0,
            total_lines: 0,
        };
    }

    let start = start_line.unwrap_or(1).clamp(1, total_lines);
    let end = end_line.unwrap_or(total_lines).clamp(start, total_lines);
    let mut sliced = lines[(start - 1) as usize..end as usize].join("\n");
    sliced.push('\n');

    ToolResult::File {
        content: sliced,
        start_line: start,
        end_line: end,
        total_lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::ToolCall;
    use crate::services::ai_tools::tools::execute_tool;
    use crate::testing::temp_dir;
    use std::path::PathBuf;

    fn fixture(label: &str, body: &str) -> (ToolScope, PathBuf) {
        let dir = temp_dir(label);
        std::fs::write(dir.join("file.txt"), body).expect("file is writable");
        let scope = ToolScope::new(&dir).expect("root resolves");
        let root = scope.root().to_path_buf();
        (scope, root)
    }

    fn args(path: &str, start: Option<u32>, end: Option<u32>) -> ReadFileArgs {
        ReadFileArgs {
            path: path.to_string(),
            start_line: start,
            end_line: end,
        }
    }

    fn unwrap_file(result: ToolResult) -> (String, u32, u32, u32) {
        let ToolResult::File {
            content,
            start_line,
            end_line,
            total_lines,
        } = result;
        (content, start_line, end_line, total_lines)
    }

    /// A whole-file read hands back exactly what is on disk. Rebuilding it from
    /// `lines()` would append a final newline the file never had, and the model
    /// would then write that difference back on its next edit.
    #[test]
    fn a_whole_file_read_is_byte_identical() {
        for body in ["one\ntwo\nthree\n", "no trailing newline", "", "\n\n"] {
            let (scope, _) = fixture("read-identical", body);
            let (content, ..) = unwrap_file(read_file(&scope, &args("file.txt", None, None)).unwrap());
            assert_eq!(content, body, "body {body:?} came back changed");
        }
    }

    #[test]
    fn a_whole_file_read_reports_the_full_range() {
        let (scope, _) = fixture("read-range-full", "one\ntwo\nthree\n");
        let (_, start, end, total) =
            unwrap_file(read_file(&scope, &args("file.txt", None, None)).unwrap());
        assert_eq!((start, end, total), (1, 3, 3));
    }

    #[test]
    fn a_requested_range_returns_only_those_lines() {
        let (scope, _) = fixture("read-range", "one\ntwo\nthree\nfour\n");
        let (content, start, end, total) =
            unwrap_file(read_file(&scope, &args("file.txt", Some(2), Some(3))).unwrap());
        assert_eq!(content, "two\nthree\n");
        assert_eq!((start, end, total), (2, 3, 4));
    }

    /// The point of clamping: the model guessed line numbers from an older
    /// search hit and the file has since shrunk. Erroring would cost a round
    /// trip to learn something the answer can simply carry.
    #[test]
    fn an_out_of_range_request_is_clamped_not_refused() {
        let (scope, _) = fixture("read-clamp", "one\ntwo\n");
        let (content, start, end, total) =
            unwrap_file(read_file(&scope, &args("file.txt", Some(1), Some(900))).unwrap());
        assert_eq!(content, "one\ntwo\n");
        assert_eq!((start, end, total), (1, 2, 2));

        let (_, start, end, _) =
            unwrap_file(read_file(&scope, &args("file.txt", Some(900), None)).unwrap());
        assert_eq!((start, end), (2, 2), "a start past EOF lands on the last line");
    }

    /// An inverted range still answers with the line the model most likely
    /// meant, rather than spending a round trip on an error.
    #[test]
    fn an_inverted_range_returns_the_start_line() {
        let (scope, _) = fixture("read-inverted", "one\ntwo\nthree\n");
        let (content, start, end, _) =
            unwrap_file(read_file(&scope, &args("file.txt", Some(3), Some(1))).unwrap());
        assert_eq!(content, "three\n");
        assert_eq!((start, end), (3, 3));
    }

    /// There is no line 1 in an empty file, and claiming one would send the
    /// model looking for content that cannot exist.
    #[test]
    fn an_empty_file_claims_no_lines() {
        let (scope, _) = fixture("read-empty", "");
        let (content, start, end, total) =
            unwrap_file(read_file(&scope, &args("file.txt", Some(1), Some(5))).unwrap());
        assert_eq!(content, "");
        assert_eq!((start, end, total), (0, 0, 0));
    }

    #[test]
    fn a_directory_is_not_a_file() {
        let (scope, root) = fixture("read-dir", "body");
        std::fs::create_dir(root.join("sub")).expect("dir is creatable");
        assert!(matches!(
            read_file(&scope, &args("sub", None, None)),
            Err(ToolError::NotAFile(_))
        ));
    }

    #[test]
    fn a_missing_file_reports_not_found() {
        let (scope, _) = fixture("read-404", "body");
        assert!(matches!(
            read_file(&scope, &args("nope.txt", None, None)),
            Err(ToolError::NotFound(_))
        ));
    }

    /// Containment is not re-implemented here — it is inherited from `resolve`,
    /// and this pins that the tool actually goes through it.
    #[test]
    fn a_path_leaving_the_root_is_refused() {
        let (scope, _) = fixture("read-escape", "body");
        assert!(matches!(
            read_file(&scope, &args("../file.txt", None, None)),
            Err(ToolError::PathEscape(_))
        ));
    }

    /// The wire contract the model is held to. Pinned as a literal because
    /// renaming a field or losing the adjacent tagging breaks every call
    /// silently — the enum would simply stop matching.
    #[test]
    fn the_call_wire_shape_is_stable() {
        let json = r#"{"tool": "readFile", "args": {"path": "src/main.rs", "startLine": 10}}"#;
        let call: ToolCall = serde_json::from_str(json).expect("parses");
        assert_eq!(
            call,
            ToolCall::ReadFile(args("src/main.rs", Some(10), None))
        );
        assert_eq!(call.name(), crate::domain::tools::ToolName::ReadFile);
        assert!(!call.is_risky(), "reading changes nothing");
    }

    /// End to end: the quoted-scalar call that motivated `flexible_args` has to
    /// survive all the way through the dispatcher, not just through serde.
    #[test]
    fn a_call_with_quoted_line_numbers_executes() {
        let (scope, _) = fixture("read-quoted", "one\ntwo\nthree\n");
        let json = r#"{"tool": "readFile", "args": {"path": "file.txt", "startLine": "2", "endLine": "3"}}"#;
        let call: ToolCall = serde_json::from_str(json).expect("quoted numbers parse");

        let (content, start, end, _) = unwrap_file(execute_tool(&scope, &call).unwrap());

        assert_eq!(content, "two\nthree\n");
        assert_eq!((start, end), (2, 3));
    }
}
