//! `readFile` — one file's text, optionally narrowed to a line range.
//!
//! An out-of-range range is clamped rather than rejected: the model is
//! guessing line numbers from an earlier search hit, and a hard error would
//! cost a round trip just to learn the file got shorter.
//!
//! No extension filter. The boundary at the tool layer is containment under
//! the scope root and nothing else — a coding agent has to read source files,
//! build manifests, lockfiles and dotfiles alike.
//!
//! A size limit, though: one read sends at most [`MAX_READ_LINES`] lines and
//! [`MAX_READ_BYTES`], and says where it stopped. Without it a 6000-line CSV
//! read "to have a look" rode along in every later round of the turn — 550k
//! tokens for a task that costs 50k (`agent_bench`, `truncated-output`).
//!
//! A large log or data file read without a range stops much sooner, at
//! [`DATA_HEAD_LINES`]: enough to see its format, and it is grep that finds
//! what is in it. Three 350 KB logs read whole put 145k tokens into one round
//! of `log-forensics`, and the model went on to grep them anyway.

use crate::domain::llm::LlmToolDefinition;
use std::fs;

use crate::domain::chunk_index::qualified_name;
use crate::domain::repo_index::{detect_language, extension_of};
use crate::domain::tools::{OutlineEntry, ReadFileArgs, ReadFiles, ToolError, ToolResult, ToolScope};
use crate::infra::language_indexers::indexer_for;

use super::super::resolve::{relative_to_root, resolve_existing};

/// The most lines one read returns — about what an editor shows in a dozen
/// screens, and more than almost any source file has.
pub const MAX_READ_LINES: u32 = 2000;

/// The most bytes one read returns: ~25k tokens. Binds before the line limit
/// on long lines — data, generated code, a minified bundle.
pub const MAX_READ_BYTES: usize = 100_000;

/// Where a read of a large log or data file without a range stops.
pub const DATA_HEAD_LINES: u32 = 50;

/// Above this a log or data file is large: a small one — a fixture, a sample —
/// is still read whole, and so can still be rewritten.
const DATA_WHOLE_BYTES: usize = 20_000;

/// Log and data files, by extension: read by searching, not start to end.
// ponytail: extensions only; `app.log.1` and extensionless dumps read as text.
const DATA_EXTENSIONS: &[&str] = &[".log", ".csv", ".tsv", ".jsonl", ".ndjson"];

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
    // being conservative costs one re-read, being wrong costs the file. So does
    // a whole-file read the limit cut short: the model has not seen the rest.
    let relative = relative_to_root(scope, &path)?;
    let asked_whole = args.start_line.is_none() && args.end_line.is_none();
    let data = asked_whole
        && content.len() > DATA_WHOLE_BYTES
        && DATA_EXTENSIONS.contains(&extension_of(&args.path).as_str());
    let max_lines = if data { DATA_HEAD_LINES } else { MAX_READ_LINES };
    let result = slice_lines(&content, args.start_line, args.end_line, max_lines);
    let whole = asked_whole && !matches!(result, ToolResult::File { truncated: true, .. });
    reads.record(&relative, &content, whole);
    Ok(result)
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

/// Clamps the requested range into the file rather than erroring, and stops
/// at the read limit: `max_lines`, and [`MAX_READ_BYTES`].
///
/// A whole file within the limit is handed back byte-identical to what was
/// read — no split-and-rejoin round trip for the common case, which would
/// also silently rewrite the file's final newline.
///
/// An empty file reports `0`/`0`/`0`. When `end_line` clamps below
/// `start_line` after each is independently clamped into `[1, total_lines]`,
/// `end_line` rises to `start_line` and one line comes back — still an answer,
/// where an error would only have cost a round trip.
fn slice_lines(content: &str, start_line: Option<u32>, end_line: Option<u32>, max_lines: u32) -> ToolResult {
    let total_lines = content.lines().count() as u32;
    let whole = start_line.is_none() && end_line.is_none();
    if whole && total_lines <= max_lines && content.len() <= MAX_READ_BYTES {
        return ToolResult::File {
            content: content.to_string(),
            start_line: if total_lines == 0 { 0 } else { 1 },
            end_line: total_lines,
            total_lines,
            clamped: false,
            truncated: false,
        };
    }

    let lines: Vec<&str> = content.lines().collect();
    if total_lines == 0 {
        return ToolResult::File {
            content: String::new(),
            start_line: 0,
            end_line: 0,
            total_lines: 0,
            clamped: false,
            truncated: false,
        };
    }

    let start = start_line.unwrap_or(1).clamp(1, total_lines);
    let asked_end = end_line.unwrap_or(total_lines).clamp(start, total_lines);
    let clamped = start_line.is_some_and(|s| s != start) || end_line.is_some_and(|e| e != asked_end);

    // Whole lines while both limits hold.
    let mut taken: u32 = 0;
    let mut bytes = 0;
    for line in &lines[(start - 1) as usize..asked_end as usize] {
        if taken == max_lines || bytes + line.len() + 1 > MAX_READ_BYTES {
            break;
        }
        bytes += line.len() + 1;
        taken += 1;
    }
    let mut end = start + taken - 1;

    let line_cut = taken == 0;
    let mut sliced = if line_cut {
        // The first line alone is over the limit: as much of it as fits, cut
        // on a character boundary.
        end = start;
        let line = lines[(start - 1) as usize];
        let mut cut = MAX_READ_BYTES;
        while !line.is_char_boundary(cut) {
            cut -= 1;
        }
        line[..cut].to_string()
    } else {
        lines[(start - 1) as usize..end as usize].join("\n")
    };
    sliced.push('\n');

    ToolResult::File {
        content: sliced,
        start_line: start,
        end_line: end,
        total_lines,
        clamped,
        truncated: line_cut || end < asked_end,
    }
}

/// What the model is told `readFile` is for.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "readFile".to_string(),
        description: "Read one file by its path relative to the workspace root, optionally restricted to a line range, or ask for its outline instead. Paths returned by grep and listFiles are already rooted correctly — pass them back unchanged. A range outside the file is cut to fit, and the result says so. Reading is also what unlocks writing: writeFile and deleteFile refuse a file this turn has not read in full, and an outline does not count. To read several files, call readFile for each in the same response — they run together, in one round. One read returns at most 2000 lines or 100 KB; a longer file or range stops there, and the result says so. A log or data file (.log, .csv, .tsv, .jsonl, .ndjson) over 20 KB read without a range stops after 50 lines — enough to see its format; grep it for the rest."
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

    fn clamped(result: ToolResult) -> bool {
        matches!(result, ToolResult::File { clamped: true, .. })
    }

    fn unwrap_file(result: ToolResult) -> (String, u32, u32, u32) {
        match result {
            ToolResult::File {
                content,
                start_line,
                end_line,
                total_lines,
                ..
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

        assert!(clamped(read(&scope, &args("file.txt", Some(1), Some(900))).unwrap()));
        assert!(clamped(read(&scope, &args("file.txt", Some(900), None)).unwrap()));
        assert!(clamped(read(&scope, &args("file.txt", Some(0), Some(1))).unwrap()), "no line 0");
        assert!(!clamped(read(&scope, &args("file.txt", Some(1), Some(2))).unwrap()), "the exact range is not cut");
        assert!(!clamped(read(&scope, &args("file.txt", None, Some(1))).unwrap()), "an open start is not cut");
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

    // ------------------------------------------------------------ limit

    fn truncated(result: &ToolResult) -> bool {
        matches!(result, ToolResult::File { truncated: true, .. })
    }

    fn numbered(n: u32) -> String {
        (1..=n).map(|i| format!("{i}\n")).collect()
    }

    /// A long file comes back as its first screens, says so, and does not
    /// count as read in full: the model has not seen the rest.
    #[test]
    fn a_whole_file_over_the_line_limit_stops_there_and_does_not_unlock_a_write() {
        let body = numbered(MAX_READ_LINES + 500);
        let (scope, _) = fixture("read-limit-lines", &body);
        let mut reads = ReadFiles::default();

        let result = read_file(&scope, &args("file.txt", None, None), &mut reads).unwrap();
        assert!(truncated(&result));
        let (content, start, end, total) = unwrap_file(result);
        assert_eq!((start, end, total), (1, MAX_READ_LINES, MAX_READ_LINES + 500));
        assert_eq!(content, numbered(MAX_READ_LINES));
        assert!(reads.check("file.txt", &body, true).is_err(), "a cut read is a partial one");
    }

    /// Exactly at the limit is the whole file — byte for byte, the missing
    /// final newline included — unlocks a write, and says nothing about a limit.
    #[test]
    fn a_file_at_the_line_limit_is_whole() {
        let body = numbered(MAX_READ_LINES).trim_end().to_string();
        let (scope, _) = fixture("read-limit-exact", &body);
        let mut reads = ReadFiles::default();
        let result = read_file(&scope, &args("file.txt", None, None), &mut reads).unwrap();
        assert!(!truncated(&result));
        assert_eq!(unwrap_file(result).0, body);
        assert_eq!(reads.check("file.txt", &body, true), Ok(()));
    }

    /// Long lines hit the byte limit first; only whole lines come back.
    #[test]
    fn long_lines_stop_at_the_byte_limit_on_a_line_boundary() {
        let line = "x".repeat(999);
        let body: String = (0..150).map(|_| format!("{line}\n")).collect();
        let (scope, _) = fixture("read-limit-bytes", &body);
        let result = read(&scope, &args("file.txt", None, None)).unwrap();
        assert!(truncated(&result));
        let (content, _, end, _) = unwrap_file(result);
        let fits = (MAX_READ_BYTES / 1000) as u32;
        assert_eq!(end, fits);
        assert_eq!(content.len(), fits as usize * 1000);

        // A line that fits only without its newline does not fit.
        let body = format!("{}{}\n", format!("{line}\n").repeat(fits as usize - 1), "y".repeat(1000));
        let (scope, _) = fixture("read-limit-bytes-edge", &body);
        let (_, _, end, _) = unwrap_file(read(&scope, &args("file.txt", None, None)).unwrap());
        assert_eq!(end, fits - 1);
    }

    /// Asking for a range does not get round the limit; it stops where a
    /// whole-file read would, counted from the range's start.
    #[test]
    fn a_range_longer_than_the_limit_is_cut_from_its_start() {
        let (scope, _) = fixture("read-limit-range", &numbered(5000));
        let result = read(&scope, &args("file.txt", Some(3000), None)).unwrap();
        assert!(truncated(&result));
        assert!(!clamped(result.clone()), "the range fit the file; the limit is a different cut");
        let (content, start, end, _) = unwrap_file(result);
        assert_eq!((start, end), (3000, 3000 + MAX_READ_LINES - 1));
        assert!(content.starts_with("3000\n"));

        let within = read(&scope, &args("file.txt", Some(4000), Some(4010))).unwrap();
        assert!(!truncated(&within), "a short range is not cut");
    }

    /// One line longer than the limit — a minified bundle — comes back cut
    /// short rather than whole, on a character boundary.
    #[test]
    fn a_single_line_over_the_limit_is_cut_on_a_character_boundary() {
        // Two-byte characters with the limit falling mid-character.
        let body = format!("a{}\n", "é".repeat(MAX_READ_BYTES));
        let (scope, _) = fixture("read-limit-one-line", &body);
        let result = read(&scope, &args("file.txt", None, None)).unwrap();
        assert!(truncated(&result));
        let (content, start, end, total) = unwrap_file(result);
        assert_eq!((start, end, total), (1, 1, 1));
        assert!(content.len() <= MAX_READ_BYTES + 1, "{}", content.len());
        assert!(content.len() >= MAX_READ_BYTES - 1, "{}", content.len());
        assert!(content.starts_with("aé") && content.ends_with("é\n"));
    }

    /// A large log read without a range is its first lines only, and a
    /// partial read. The same text under another extension, a range asked
    /// for, or a small data file are not cut.
    #[test]
    fn a_large_data_file_read_whole_stops_at_its_head() {
        let body: String = (1..=1500).map(|i| format!("{i} request ok\n")).collect();
        assert!(body.len() > DATA_WHOLE_BYTES);
        let (scope, root) = fixture("read-data-head", &body);
        std::fs::write(root.join("app.LOG"), &body).unwrap();
        let small: String = (1..=100).map(|i| format!("{i},2\n")).collect();
        std::fs::write(root.join("small.csv"), &small).unwrap();
        let mut reads = ReadFiles::default();

        let result = read_file(&scope, &args("app.LOG", None, None), &mut reads).unwrap();
        assert!(truncated(&result));
        let (content, start, end, total) = unwrap_file(result);
        assert_eq!((start, end, total), (1, DATA_HEAD_LINES, 1500));
        assert!(content.ends_with("50 request ok\n"), "{content}");
        assert!(reads.check("app.LOG", &body, true).is_err(), "a head is a partial read");

        let text = read(&scope, &args("file.txt", None, None)).unwrap();
        assert_eq!(unwrap_file(text).2, 1500, "not a data file: read whole");

        let range = read(&scope, &args("app.LOG", Some(100), Some(400))).unwrap();
        assert!(!truncated(&range), "a range asked for is not cut to the head");
        assert_eq!(unwrap_file(range).2, 400);

        let result = read_file(&scope, &args("small.csv", None, None), &mut reads).unwrap();
        assert!(!truncated(&result), "a small data file past the head is still read whole");
        assert_eq!(reads.check("small.csv", &small, true), Ok(()));
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
