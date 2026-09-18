//! `semanticSearch` — the model's way into code it has not seen yet. The
//! ranking is `services::code_search`; this is the wrapper: the default size,
//! how much text each match carries, and the schema.

use crate::domain::llm::LlmToolDefinition;
use crate::domain::tools::{SemanticSearchArgs, ToolDeps, ToolError, ToolResult};
use crate::services::code_search::{DEFAULT_TOP_K, MAX_TOP_K};

/// Text per match by default. The result lands in the model's context, and
/// ten whole chunks are thousands of tokens spent on text it re-reads with
/// `readFile` anyway. Path, lines and name decide which hit to open; the text
/// only has to be enough to recognise a wrong turn — and to be readable in the
/// transcript, where it is all a person sees.
const TEXT_CHARS: usize = 240;
/// With `preview`: most of a typical chunk (median ~470 bytes).
const PREVIEW_CHARS: usize = 1_200;

pub fn semantic_search(args: &SemanticSearchArgs, deps: &ToolDeps) -> Result<ToolResult, ToolError> {
    let search = deps
        .search
        .as_ref()
        .ok_or_else(|| ToolError::SearchUnavailable("this folder has no index".to_string()))?;
    let result = search(&args.query, args.fts.as_deref(), args.top_k.unwrap_or(DEFAULT_TOP_K))
        .map_err(ToolError::SearchUnavailable)?;
    let limit = if args.preview == Some(true) { PREVIEW_CHARS } else { TEXT_CHARS };
    let matches = result
        .matches
        .into_iter()
        .map(|mut m| {
            m.text = shorten(&m.text, limit);
            m
        })
        .collect();
    Ok(ToolResult::SearchResults { matches, meta: result.meta })
}

fn shorten(text: &str, limit: usize) -> String {
    let Some((cut, _)) = text.char_indices().nth(limit) else {
        return text.to_string();
    };
    format!("{}…", &text[..cut])
}

pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "semanticSearch".to_string(),
        description: format!(
            "Find code by what it does or what it is called — the first tool to reach for when you do not already know the file. \
Declarations named in the query come first; the rest is ranked by meaning and by shared words together. \
Each match gives the path, the line range readFile takes, the enclosing declaration's name, and the start of its text. \
Write the query as a sentence about the behaviour, and include any function, type or file names you know or can justify from the user's words. \
Put the exact words that must appear in the code in fts. \
Read the most promising one or two matches before searching again; a second search should use a name the first one taught you. \
If meta.hint is present, follow it. Use grep instead when you need every occurrence of an exact string. \
Returns at most {MAX_TOP_K} matches."
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What you are looking for, as a sentence, with any identifiers you know (parseConfig, RepoIndexer, read_source)."
                },
                "fts": {
                    "type": ["array", "null"],
                    "items": { "type": "string" },
                    "description": "Words that must match as text, when they are not just the query's own — identifiers, error messages, config keys. Leave out filler and guesses: a wrong word here costs ranking."
                },
                "topK": {
                    "type": ["integer", "null"],
                    "minimum": 1,
                    "description": format!("How many matches, default {DEFAULT_TOP_K}, at most {MAX_TOP_K}.")
                },
                "preview": {
                    "type": ["boolean", "null"],
                    "description": "Longer text for every match. Leave unset unless you must compare several candidates verbatim; readFile reads one precisely."
                }
            },
            "required": ["query"]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::code_search::{CodeMatch, CodeSearchResult, MatchSource, SearchMeta};
    use std::sync::{Arc, Mutex};

    type Asked = Arc<Mutex<Vec<(String, Option<Vec<String>>, usize)>>>;

    /// Answers every search with one match carrying `text`, and records what
    /// it was asked.
    fn deps_answering(text: &str) -> (ToolDeps, Asked) {
        let asked: Asked = Arc::default();
        let record = Arc::clone(&asked);
        let text = text.to_string();
        let deps = ToolDeps {
            search: Some(Arc::new(move |query: &str, fts: Option<&[String]>, top_k: usize| {
                record.lock().unwrap().push((query.to_string(), fts.map(<[String]>::to_vec), top_k));
                Ok(CodeSearchResult {
                    matches: vec![CodeMatch {
                        path: "a.rs".into(),
                        start_line: 3,
                        end_line: 9,
                        name: Some("a".into()),
                        text: text.clone(),
                        source: MatchSource::Symbol,
                    }],
                    meta: SearchMeta { tiers_used: vec![MatchSource::Symbol], weak: false, hint: None },
                })
            })),
            ..ToolDeps::default()
        };
        (deps, asked)
    }

    fn args(query: &str) -> SemanticSearchArgs {
        SemanticSearchArgs { query: query.into(), ..SemanticSearchArgs::default() }
    }

    fn text_of(result: ToolResult) -> String {
        let ToolResult::SearchResults { matches, .. } = result else { panic!("{result:?}") };
        matches[0].text.clone()
    }

    #[test]
    fn without_an_index_the_model_is_pointed_at_grep() {
        let error = semantic_search(&args("x"), &ToolDeps::default()).unwrap_err();
        assert!(matches!(error, ToolError::SearchUnavailable(_)));
        assert!(error.to_string().contains("grep"), "{error}");
    }

    #[test]
    fn a_failed_search_is_reported_as_unavailable() {
        let deps = ToolDeps { search: Some(Arc::new(|_: &str, _: Option<&[String]>, _: usize| Err("disk".into()))), ..ToolDeps::default() };
        let error = semantic_search(&args("x"), &deps).unwrap_err();
        assert!(matches!(&error, ToolError::SearchUnavailable(reason) if reason == "disk"), "{error}");
    }

    #[test]
    fn the_query_fts_and_default_size_reach_the_search() {
        let (deps, asked) = deps_answering("fn a() {}");
        let call = SemanticSearchArgs { fts: Some(vec!["a".into()]), ..args("find a") };
        semantic_search(&call, &deps).unwrap();
        semantic_search(&SemanticSearchArgs { top_k: Some(3), ..args("b") }, &deps).unwrap();
        assert_eq!(
            *asked.lock().unwrap(),
            [("find a".to_string(), Some(vec!["a".to_string()]), DEFAULT_TOP_K), ("b".to_string(), None, 3)]
        );
    }

    #[test]
    fn text_is_short_unless_a_preview_is_asked_for() {
        let long = "й".repeat(PREVIEW_CHARS * 2);
        let (deps, _) = deps_answering(&long);

        let short = text_of(semantic_search(&args("x"), &deps).unwrap());
        let preview = text_of(semantic_search(&SemanticSearchArgs { preview: Some(true), ..args("x") }, &deps).unwrap());

        assert_eq!(short.chars().count(), TEXT_CHARS + 1, "cut on a character, with an ellipsis");
        assert!(short.ends_with('…'));
        assert_eq!(preview.chars().count(), PREVIEW_CHARS + 1);
        let (deps, _) = deps_answering("fn a() {}");
        assert_eq!(text_of(semantic_search(&args("x"), &deps).unwrap()), "fn a() {}", "short text is left alone");
    }

    /// What the model reads back.
    #[test]
    fn the_result_is_flat_on_the_wire() {
        let (deps, _) = deps_answering("fn a() {}");
        let wire = serde_json::to_value(semantic_search(&args("x"), &deps).unwrap()).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({
                "result": "searchResults",
                "matches": [{ "path": "a.rs", "startLine": 3, "endLine": 9, "name": "a", "text": "fn a() {}", "source": "symbol" }],
                "meta": { "tiersUsed": ["symbol"], "weak": false, "hint": null }
            })
        );
    }
}
