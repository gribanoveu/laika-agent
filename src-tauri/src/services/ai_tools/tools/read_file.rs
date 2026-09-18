//! `readFile` — one file's text, optionally narrowed to a line range.
//!
//! An out-of-range range is clamped rather than rejected: the model is
//! guessing line numbers from an earlier search hit, and a hard error would
//! cost a round trip just to learn the file got shorter.
//!
//! No extension filter. The boundary at the tool layer is containment under
//! the scope root and nothing else — a coding agent has to read source files,
//! build manifests, lockfiles and dotfiles alike.

use crate::domain::llm::LlmToolDefinition;
use std::fs;

use crate::domain::chunk_index::qualified_name;
use crate::domain::repo_index::detect_language;
use crate::domain::tools::{OutlineEntry, ReadFileArgs, ReadFiles, ToolError, ToolResult, ToolScope};
use crate::infra::language_indexers::indexer_for;

use super::super::resolve::{relative_to_root, resolve_existing};

pub fn read_file(
    scope: &ToolScope,
    args: &ReadFileArgs,
    reads: &mut ReadFiles,
) -> Result<ToolResult, ToolError> {
    let path = resolve_existing(scope, &args.path)?;
    if !path.is_file() {
        return Err(ToolError::NotAFile(args.path.clone()));
    }
    let content = fs::read_to_string(&path).map_err(ToolError::Io)?;
    if args.outline == Some(true) {
        return Ok(outline(&args.path, &content));
    }

    // Recorded against the whole file even when a slice is returned, and under
    // the canonical spelling rather than whatever the model typed — otherwise
    // `./src/a.rs` and `src/a.rs` would be two different files to the registry.
    // A range that happens to cover the whole file still counts as partial:
    // being conservative costs one re-read, being wrong costs the file.
    let whole = args.start_line.is_none() && args.end_line.is_none();
    reads.record(&relative_to_root(scope, &path)?, &content, whole);

    Ok(slice_lines(content, args.start_line, args.end_line))
}

/// The file's declarations and headings, found by the same parser the index
/// uses — on this one file, so it works before any sync and in a folder that
/// is not indexed at all.
///
/// Not a read: nothing is recorded, so an outline does not unlock a write.
/// The model has seen names and line numbers, not the text it would replace.
fn outline(path: &str, content: &str) -> ToolResult {
    let symbols = indexer_for(detect_language(path)).index(content);
    let entries = symbols
        .iter()
        .map(|symbol| OutlineEntry {
            name: qualified_name(symbol, &symbols),
            start_line: symbol.start_line,
            end_line: symbol.end_line,
        })
        .collect();
    ToolResult::FileOutline { path: path.to_string(), entries, total_lines: content.lines().count() as u32 }
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

/// What the model is told `readFile` is for.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "readFile".to_string(),
        description: "Read one file by its path relative to the workspace root, optionally restricted to a line range, or ask for its outline instead. Paths returned by grep and listFiles are already rooted correctly — pass them back unchanged. A range outside the file is clamped, not rejected. Reading is also what unlocks writing: writeFile and deleteFile refuse a file this turn has not read."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to the workspace root."
                },
                "startLine": {
                    "type": [
                        "integer",
                        "null"
                    ],
                    "minimum": 1,
                    "description": "1-indexed first line to return, inclusive. Omit to start at the beginning."
                },
                "endLine": {
                    "type": [
                        "integer",
                        "null"
                    ],
                    "minimum": 1,
                    "description": "1-indexed last line to return, inclusive. Omit to read to the end. Prefer a range over the whole file when only part of it matters — but read the whole file before writing it, since a partial read does not unlock a write."
                },
                "outline": {
                    "type": [
                        "boolean",
                        "null"
                    ],
                    "description": "When true, return the file's declarations and headings with the lines each spans, plus its total line count, instead of its text. Use it on a large file you need only part of: read the outline, then read the one range that matters. Ignores startLine/endLine, and does not unlock a write."
                }
            },
            "required": [
                "path"
            ]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::{ReadFiles, ToolCall, ToolDeps};
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

    /// Every test reads with a throwaway registry unless it is testing the
    /// registry itself, so a signature change lands here and nowhere else.
    fn read(scope: &ToolScope, args: &ReadFileArgs) -> Result<ToolResult, ToolError> {
        read_file(scope, args, &mut ReadFiles::default())
    }

    fn args(path: &str, start: Option<u32>, end: Option<u32>) -> ReadFileArgs {
        ReadFileArgs {
            path: path.to_string(),
            start_line: start,
            end_line: end,
            outline: None,
        }
    }

    fn unwrap_file(result: ToolResult) -> (String, u32, u32, u32) {
        match result {
            ToolResult::File {
                content,
                start_line,
                end_line,
                total_lines,
            } => (content, start_line, end_line, total_lines),
            other => panic!("expected a file read, got {other:?}"),
        }
    }

    /// A whole-file read hands back exactly what is on disk. Rebuilding it from
    /// `lines()` would append a final newline the file never had, and the model
    /// would then write that difference back on its next edit.
    #[test]
    fn a_whole_file_read_is_byte_identical() {
        for body in ["one\ntwo\nthree\n", "no trailing newline", "", "\n\n"] {
            let (scope, _) = fixture("read-identical", body);
            let (content, ..) = unwrap_file(read(&scope, &args("file.txt", None, None)).unwrap());
            assert_eq!(content, body, "body {body:?} came back changed");
        }
    }

    #[test]
    fn a_whole_file_read_reports_the_full_range() {
        let (scope, _) = fixture("read-range-full", "one\ntwo\nthree\n");
        let (_, start, end, total) =
            unwrap_file(read(&scope, &args("file.txt", None, None)).unwrap());
        assert_eq!((start, end, total), (1, 3, 3));
    }

    #[test]
    fn a_requested_range_returns_only_those_lines() {
        let (scope, _) = fixture("read-range", "one\ntwo\nthree\nfour\n");
        let (content, start, end, total) =
            unwrap_file(read(&scope, &args("file.txt", Some(2), Some(3))).unwrap());
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
            unwrap_file(read(&scope, &args("file.txt", Some(1), Some(900))).unwrap());
        assert_eq!(content, "one\ntwo\n");
        assert_eq!((start, end, total), (1, 2, 2));

        let (_, start, end, _) =
            unwrap_file(read(&scope, &args("file.txt", Some(900), None)).unwrap());
        assert_eq!((start, end), (2, 2), "a start past EOF lands on the last line");
    }

    /// An inverted range still answers with the line the model most likely
    /// meant, rather than spending a round trip on an error.
    #[test]
    fn an_inverted_range_returns_the_start_line() {
        let (scope, _) = fixture("read-inverted", "one\ntwo\nthree\n");
        let (content, start, end, _) =
            unwrap_file(read(&scope, &args("file.txt", Some(3), Some(1))).unwrap());
        assert_eq!(content, "three\n");
        assert_eq!((start, end), (3, 3));
    }

    /// There is no line 1 in an empty file, and claiming one would send the
    /// model looking for content that cannot exist.
    #[test]
    fn an_empty_file_claims_no_lines() {
        let (scope, _) = fixture("read-empty", "");
        let (content, start, end, total) =
            unwrap_file(read(&scope, &args("file.txt", Some(1), Some(5))).unwrap());
        assert_eq!(content, "");
        assert_eq!((start, end, total), (0, 0, 0));
    }

    #[test]
    fn a_directory_is_not_a_file() {
        let (scope, root) = fixture("read-dir", "body");
        std::fs::create_dir(root.join("sub")).expect("dir is creatable");
        assert!(matches!(
            read(&scope, &args("sub", None, None)),
            Err(ToolError::NotAFile(_))
        ));
    }

    #[test]
    fn a_missing_file_reports_not_found() {
        let (scope, _) = fixture("read-404", "body");
        assert!(matches!(
            read(&scope, &args("nope.txt", None, None)),
            Err(ToolError::NotFound(_))
        ));
    }

    /// Containment is not re-implemented here — it is inherited from `resolve`,
    /// and this pins that the tool actually goes through it.
    #[test]
    fn a_path_leaving_the_root_is_refused() {
        let (scope, _) = fixture("read-escape", "body");
        assert!(matches!(
            read(&scope, &args("../file.txt", None, None)),
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

        let (content, start, end, _) = unwrap_file(execute_tool(&scope, &call, &mut ReadFiles::default(), &mut Vec::new(), &ToolDeps::default()).unwrap());

        assert_eq!(content, "two\nthree\n");
        assert_eq!((start, end), (2, 3));
    }

    // ------------------------------------------------------------ outline

    fn outline_of(scope: &ToolScope, path: &str, reads: &mut ReadFiles) -> (Vec<(String, u32, u32)>, u32) {
        let args = ReadFileArgs { path: path.into(), start_line: Some(2), end_line: Some(2), outline: Some(true) };
        match read_file(scope, &args, reads).unwrap() {
            ToolResult::FileOutline { entries, total_lines, .. } => {
                (entries.into_iter().map(|e| (e.name, e.start_line, e.end_line)).collect(), total_lines)
            }
            other => panic!("expected an outline, got {other:?}"),
        }
    }

    /// Names as the index qualifies them, lines as `readFile` takes them —
    /// and the range asked for alongside is ignored.
    #[test]
    fn an_outline_lists_declarations_with_their_lines() {
        let dir = temp_dir("read-outline");
        std::fs::write(
            dir.join("lib.rs"),
            "use std::fs;\n\npub struct Store;\n\nimpl Store {\n    fn open() {}\n\n    fn close() {}\n}\n",
        )
        .unwrap();
        let scope = ToolScope::new(&dir).unwrap();

        let (entries, total) = outline_of(&scope, "lib.rs", &mut ReadFiles::default());

        assert_eq!(total, 9);
        assert!(entries.contains(&("Store.open".to_string(), 6, 6)), "{entries:?}");
        assert!(entries.contains(&("Store.close".to_string(), 8, 8)), "{entries:?}");
        assert!(entries.iter().any(|(name, start, end)| name == "Store" && (*start, *end) == (5, 9)), "{entries:?}");
    }

    /// A file with no parser still answers: nothing declared, and how long a
    /// plain read would be.
    #[test]
    fn a_file_without_a_parser_has_an_empty_outline_and_a_size() {
        let (scope, _) = fixture("read-outline-plain", "one\ntwo\nthree\n");
        assert_eq!(outline_of(&scope, "file.txt", &mut ReadFiles::default()), (vec![], 3));
    }

    /// Seeing names and line numbers is not seeing the text a write would
    /// replace.
    #[test]
    fn an_outline_does_not_count_as_a_read() {
        let (scope, _) = fixture("read-outline-no-unlock", "fn a() {}\n");
        let mut reads = ReadFiles::default();
        outline_of(&scope, "file.txt", &mut reads);
        assert!(reads.check("file.txt", "fn a() {}\n", true).is_err());
    }
}
