//! Search of the open folder's code: names first, then words and meaning
//! fused into one ranking.
//!
//! A declaration named as the query spells it comes first — nothing a ranker
//! says outweighs a name the caller got right. Words (BM25) and meaning
//! (embeddings) are peers: `fuse_rrf` merges their rankings rather than
//! picking one, because "these words appear here" and "this passage means
//! that" are different evidence, and a chunk carrying both beats one carrying
//! either.
//!
//! The constants below were set by the quality bench (F-5.13c,
//! `services/search_bench.rs`, 67 questions over three repositories), not by
//! taste; `docs/06-port-plan.md` has the numbers. Upstream's stem tier is not
//! ported — the full-text index matches prefixes and weighs chunk names four
//! to one. Its path tier came back as [`PATH_BOOST`].

use std::collections::{HashMap, HashSet};

use crate::domain::chunk_index::{ChunkId, ChunkMetadata};
use crate::domain::code_search::{CodeMatch, CodeSearchResult, MatchSource, SearchMeta};
use crate::domain::repo_index::Language;
use crate::domain::search_query::{
    SearchMetaInput, extract_search_tokens, fts5_query, is_documentation, looks_like_identifier,
    path_segment_matches, weak_search_hint,
};
use crate::infra::index_store::IndexStoreError;
use crate::services::chunk_text::resolve_chunk;
use crate::services::index_sync::RepoIndexer;

pub const DEFAULT_TOP_K: usize = 10;
pub const MAX_TOP_K: usize = 50;

/// How many candidates each ranking contributes per match wanted. A chunk
/// whose file changed since it was indexed is dropped when its text is read;
/// without slack, ten asked for could come back as seven.
const CANDIDATES_PER_MATCH: usize = 2;

/// Reciprocal-rank fusion's smoothing constant. Small: being first by one
/// measure outweighs being sixth by both. The published 60 did the opposite —
/// a long Russian design document, middling on words *and* on meaning,
/// outranked the function that was first by meaning, and MRR fell below words
/// alone (0.437 against 0.494).
const RRF_K: f32 = 3.0;

/// How much more a rank by words counts than the same rank by meaning. A
/// static model is a blunt instrument next to an exact word; it earns its
/// place on questions whose words the code does not use.
const LEXICAL_WEIGHT: f32 = 1.5;

/// Prose — Markdown, AsciiDoc, plain text — is long, and long text shares a
/// few words and a general drift with most questions. Scaled down, not out:
/// harder than this and questions answered by documentation lose.
const PROSE_FACTOR: f32 = 0.75;

/// A query word in a file's path (`controller` → `DocumentController.java`)
/// says what the file is about better than any one passage of it.
const PATH_BOOST: f32 = 1.5;

/// A file changed by a commit whose message is near the query in meaning.
/// Small on purpose: history is a hint about where to look, it says nothing
/// about which passage answers — and in a repository with a few vague
/// commits it says nothing at all, so it must not outvote the text.
///
/// Bench, 2026-09-21 (with `includeDocs` already in): MRR 0.564 at 1.0 and at
/// 1.2, 0.560 at 1.5. History fired on 15 of 39 questions here and named the
/// answer's file in 8; on docflow 10 of 20 and 3; on a 9-commit service 1 and
/// 0. Neutral at this weight — kept for repositories whose commits describe
/// the work; raise it only on a bench that shows a gain.
const HISTORY_BOOST: f32 = 1.2;

/// With documentation left out, how many more candidates each ranking brings
/// in. In a repository whose docs outnumber its code the plain pool can be
/// mostly prose, and dropping it would leave fewer matches than asked for.
const WITHOUT_DOCS_SLACK: usize = 2;

/// `fts`, when given, is what the word ranking searches instead of `query` —
/// unless it has no searchable word in it, in which case `query` is: a model
/// that asked for more precision must not lose the ranking for it.
///
/// `include_docs` false leaves documentation out (`is_documentation`); the
/// hint then says how many matches that cost, so the model can ask again.
pub fn search(
    indexer: &RepoIndexer,
    query: &str,
    fts: Option<&[String]>,
    top_k: usize,
    include_docs: bool,
) -> Result<CodeSearchResult, IndexStoreError> {
    let top_k = top_k.clamp(1, MAX_TOP_K);
    let store = indexer.store();
    let tokens = extract_search_tokens(query);
    let mut ranked: Vec<(ChunkMetadata, MatchSource)> = Vec::new();

    for name in names_in(query, &tokens) {
        for id in store.chunks_declaring(&name, top_k)? {
            ranked.extend(store.load_chunk(&id)?.map(|chunk| (chunk, MatchSource::Symbol)));
        }
    }

    let candidates = top_k * CANDIDATES_PER_MATCH * if include_docs { 1 } else { WITHOUT_DOCS_SLACK };
    let lexical = match fts.and_then(|terms| fts5_query(&terms.join(" "))).or_else(|| fts5_query(query)) {
        Some(fts) => store.search_bm25(&fts, candidates)?.into_iter().map(|(id, _)| id).collect(),
        None => Vec::new(),
    };
    let mut tiers_used = vec![MatchSource::Symbol, MatchSource::Lexical];
    let mut unavailable = None;
    let semantic = match indexer.search_meaning(query, candidates) {
        Ok(hits) => hits.into_iter().map(|(id, _)| id).collect(),
        Err(error) => {
            unavailable = Some(error.to_string());
            Vec::new()
        }
    };
    if !semantic.is_empty() {
        tiers_used.push(MatchSource::Semantic);
    } else if unavailable.is_none() {
        // Nothing embedded, and not for want of time: the last sync could not
        // load the model. Worth the same warning as a failed search.
        unavailable = indexer.status().embedding_error;
    }
    let by_history = indexer.files_by_history(query);
    // Semantic first: a chunk both found is labelled by the more telling one.
    let mut fused = Vec::new();
    for (id, score, source) in
        fuse_rrf([(semantic, MatchSource::Semantic, 1.0), (lexical, MatchSource::Lexical, LEXICAL_WEIGHT)])
    {
        if let Some(chunk) = store.load_chunk(&id)? {
            let score = score * weight(&chunk, &tokens, &by_history);
            fused.push((chunk, score, source));
        }
    }
    // Stable: equal scores keep the order fusion gave them.
    fused.sort_by(|a, b| b.1.total_cmp(&a.1));
    ranked.extend(fused.into_iter().map(|(chunk, _, source)| (chunk, source)));

    let mut seen = HashSet::new();
    let mut matches = Vec::with_capacity(top_k);
    let mut docs_left_out = 0;
    for (chunk, source) in ranked {
        if matches.len() == top_k {
            break;
        }
        if !seen.insert(chunk.id.clone()) {
            continue;
        }
        if !include_docs && is_documentation(&chunk.file_id.0) {
            docs_left_out += 1;
            continue;
        }
        // Changed on disk since it was indexed: the watcher is on its way.
        let Ok(resolved) = resolve_chunk(indexer.root(), &chunk) else { continue };
        matches.push(CodeMatch {
            path: chunk.file_id.0,
            start_line: resolved.start_line,
            end_line: resolved.end_line,
            name: chunk.qualified_name,
            text: resolved.text,
            source,
        });
    }

    let (weak, hint) = weak_search_hint(SearchMetaInput {
        match_count: matches.len(),
        symbol_hits: matches.iter().filter(|m| m.source == MatchSource::Symbol).count() as u32,
        has_semantic: matches.iter().any(|m| m.source == MatchSource::Semantic),
        only_lexical: matches.iter().all(|m| m.source == MatchSource::Lexical),
        extracted_tokens: &tokens,
    });
    // Outranks the ordinary advice, which would say to wait for the semantic
    // index — wrong when waiting will not bring it back.
    let hint = match unavailable {
        Some(reason) => Some(format!(
            "Search by meaning is unavailable ({reason}); these results matched on names and words only."
        )),
        None => hint.map(str::to_string),
    };
    // Said even beside another hint: it is the one thing a second search can
    // change without rewording anything.
    let hint = match (docs_left_out, hint) {
        (0, hint) => hint,
        (n, hint) => {
            let docs = format!(
                "{n} documentation {} {} left out; search again with includeDocs if the answer may be in the docs.",
                if n == 1 { "match" } else { "matches" },
                if n == 1 { "was" } else { "were" },
            );
            Some(hint.map_or(docs.clone(), |hint| format!("{hint} {docs}")))
        }
    };
    Ok(CodeSearchResult { matches, meta: SearchMeta { tiers_used, weak, hint } })
}

/// The words of the query that are names as code spells them — and the whole
/// query when it is one word, since a one-word search is a name more often
/// than not. Plain words stay out: "how does sync work" must not put every
/// function called `sync` above the answer.
fn names_in(query: &str, tokens: &[String]) -> Vec<String> {
    let mut names: Vec<String> = tokens.iter().filter(|t| looks_like_identifier(t)).cloned().collect();
    let whole = query.trim();
    let one_word = !whole.is_empty() && whole.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if one_word && !names.iter().any(|n| n.eq_ignore_ascii_case(whole)) {
        names.push(whole.to_string());
    }
    names
}

/// What a chunk's fused score is multiplied by, for what it is rather than
/// what it says.
fn weight(chunk: &ChunkMetadata, tokens: &[String], by_history: &HashSet<String>) -> f32 {
    let prose = matches!(chunk.language, Language::Markdown | Language::PlainText);
    let named = tokens.iter().any(|token| path_segment_matches(&chunk.file_id.0, token));
    let changed = by_history.contains(&chunk.file_id.0);
    (if prose { PROSE_FACTOR } else { 1.0 })
        * (if named { PATH_BOOST } else { 1.0 })
        * (if changed { HISTORY_BOOST } else { 1.0 })
}

/// Merges rankings by `Σ weight / (K + rank)` over the lists that hold a
/// chunk, best first.
///
/// Rank, not score: cosine similarity and BM25 measure incomparable things,
/// and no fixed factor makes them commensurable across queries. A chunk keeps
/// the source of the first list it is in; ties go to the earlier list.
fn fuse_rrf<const N: usize>(lists: [(Vec<ChunkId>, MatchSource, f32); N]) -> Vec<(ChunkId, f32, MatchSource)> {
    let mut fused: HashMap<ChunkId, (f32, (usize, usize), MatchSource)> = HashMap::new();
    for (order, (list, source, weight)) in lists.into_iter().enumerate() {
        for (rank, id) in list.into_iter().enumerate() {
            let contribution = weight / (RRF_K + rank as f32 + 1.0);
            fused.entry(id).and_modify(|entry| entry.0 += contribution).or_insert((contribution, (order, rank), source));
        }
    }
    let mut out: Vec<_> = fused.into_iter().collect();
    out.sort_by(|(_, a), (_, b)| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    out.into_iter().map(|(id, (score, _, source))| (id, score, source)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chunk_index::ChunkBuildOptions;
    use crate::domain::embeddings::{Embedding, EmbeddingError, EmbeddingProvider};
    use crate::domain::workspace_index::IndexEventSink;
    use crate::testing::temp_dir;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    const DIMS: usize = 64;

    /// "Meaning" is shared words, with one synonym so that meaning and words
    /// can disagree: `automobile` means `car`, and no text says `automobile`.
    #[derive(Default)]
    struct FakeModel {
        fail: AtomicBool,
    }

    impl EmbeddingProvider for FakeModel {
        fn embed(&self, texts: &[&str]) -> Result<Vec<Embedding>, EmbeddingError> {
            if self.fail.load(Ordering::SeqCst) {
                return Err(EmbeddingError::NotFound(PathBuf::from("model.safetensors")));
            }
            Ok(texts
                .iter()
                .map(|text| {
                    let mut v = vec![0.0_f32; DIMS];
                    for word in text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
                        let word = if word == "automobile" { "car" } else { word };
                        v[blake3::hash(word.to_lowercase().as_bytes()).as_bytes()[0] as usize % DIMS] += 1.0;
                    }
                    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
                    Embedding(v.iter().map(|x| x / norm).collect())
                })
                .collect())
        }
        fn dimensions(&self) -> usize {
            DIMS
        }
    }

    fn indexed(label: &str, files: &[(&str, &str)], model: Arc<FakeModel>) -> RepoIndexer {
        let root = temp_dir(&format!("{label}-repo"));
        for (path, body) in files {
            fs::write(root.join(path), body).unwrap();
        }
        let indexer =
            RepoIndexer::open(&root, &temp_dir(&format!("{label}-store")), model, ChunkBuildOptions::default()).unwrap();
        let sink: IndexEventSink = Arc::new(|_| {});
        indexer.sync(&sink).unwrap();
        indexer
    }

    fn summary(result: &CodeSearchResult) -> Vec<(String, MatchSource)> {
        result.matches.iter().map(|m| (m.name.clone().unwrap_or_else(|| m.path.clone()), m.source)).collect()
    }

    // ------------------------------------------------------------ names

    /// Prose that repeats a name all over outranks its declaration on words;
    /// the declaration still comes first.
    #[test]
    fn a_name_the_query_spells_comes_first_with_its_lines() {
        let indexer = indexed(
            "search-name",
            &[
                ("notes.md", "# Notes\n\nparse_config parse_config parse_config is called on start.\n"),
                ("config.rs", "use std::fs;\n\nfn parse_config() {\n    todo!()\n}\n"),
            ],
            Arc::default(),
        );

        let result = search(&indexer, "where is parse_config", None, 5, true).unwrap();

        let first = &result.matches[0];
        assert_eq!((first.path.as_str(), first.source), ("config.rs", MatchSource::Symbol));
        assert_eq!((first.start_line, first.end_line), (1, 5));
        assert_eq!(result.matches.iter().filter(|m| m.path == "config.rs").count(), 1, "listed twice");
    }

    /// Documentation is out unless asked for, and the hint says what that
    /// cost — the model's way to know a second search with it is worth making.
    #[test]
    fn documentation_is_left_out_unless_asked_for_and_the_hint_says_so() {
        let indexer = indexed(
            "search-docs",
            &[
                ("notes.md", "# Notes\n\nparse_config parse_config parse_config is called on start.\n"),
                ("config.rs", "use std::fs;\n\nfn parse_config() {\n    todo!()\n}\n"),
            ],
            Arc::default(),
        );

        let code = search(&indexer, "where is parse_config called", None, 5, false).unwrap();
        assert!(code.matches.iter().all(|m| m.path == "config.rs"), "{:?}", summary(&code));
        let hint = code.meta.hint.unwrap_or_default();
        assert!(hint.contains("1 documentation match was left out") && hint.contains("includeDocs"), "{hint}");

        let all = search(&indexer, "where is parse_config called", None, 5, true).unwrap();
        assert!(all.matches.iter().any(|m| m.path == "notes.md"), "{:?}", summary(&all));
        assert!(!all.meta.hint.unwrap_or_default().contains("includeDocs"));
    }

    /// Prose that outranks the code on every measure fills the plain candidate
    /// pool; left out, it must not leave the search empty-handed.
    #[test]
    fn documentation_crowding_the_candidates_does_not_crowd_out_the_code() {
        let indexer = indexed(
            "search-docs-crowd",
            &[
                ("a.md", "gizmo gizmo gizmo stuff\n"),
                ("b.md", "gizmo gizmo gizmo stuff\n"),
                ("c.md", "gizmo gizmo gizmo stuff\n"),
                ("run.rs", "fn run() { gizmo }\n"),
            ],
            Arc::default(),
        );

        let result = search(&indexer, "gizmo stuff", None, 1, false).unwrap();

        assert_eq!(summary(&result).first().map(|(name, _)| name.as_str()), Some("run"), "{:?}", summary(&result));
    }

    /// The nearest commits count, and no more than a few of them: a query
    /// near everything is evidence about nothing.
    #[test]
    fn only_the_nearest_few_commits_count() {
        let files = [("f1.rs", "gear"), ("f2.rs", "gear gear alpha"), ("f3.rs", "gear alpha"), ("f4.rs", "gear alpha beta")];
        let indexer = indexed("search-history-near", &files.map(|(path, _)| (path, "fn x() {}\n")), Arc::default());
        let repo = git2::Repository::init(indexer.root()).unwrap();
        // Oldest first, so the newest commit is the least near.
        for (path, message) in files {
            commit_file(&repo, path, message);
        }

        let near = indexer.files_by_history("gear");

        assert_eq!(near, HashSet::from(["f1.rs".to_string(), "f2.rs".to_string(), "f3.rs".to_string()]));
    }

    /// Commits one file at a time, so each commit touches exactly `path`.
    fn commit_file(repo: &git2::Repository, path: &str, message: &str) {
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new(path)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let parent: Vec<git2::Commit> = repo.head().ok().and_then(|h| h.peel_to_commit().ok()).into_iter().collect();
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents).unwrap();
    }

    /// A commit whose message is near the query points at the files it
    /// touched, and those weigh a little more — only those, and only when
    /// some commit is near.
    #[test]
    fn a_file_a_near_commit_touched_weighs_a_little_more() {
        let indexer = indexed(
            "search-history",
            &[("a.rs", "fn a() { gear }\n"), ("b.rs", "fn b() { gear }\n")],
            Arc::default(),
        );
        let repo = git2::Repository::init(indexer.root()).unwrap();
        commit_file(&repo, "a.rs", "initial setup");
        commit_file(&repo, "b.rs", "gear handling");

        let near = indexer.files_by_history("gear");
        assert_eq!(near, HashSet::from(["b.rs".to_string()]));
        assert!(indexer.files_by_history("nothing like it").is_empty());

        let ids = indexer.store().search_bm25(&fts5_query("gear").unwrap(), 5).unwrap();
        let chunk_of = |path: &str| {
            ids.iter().find_map(|(id, _)| indexer.store().load_chunk(id).unwrap().filter(|c| c.file_id.0 == path)).unwrap()
        };
        assert_eq!(weight(&chunk_of("b.rs"), &[], &near), HISTORY_BOOST);
        assert_eq!(weight(&chunk_of("a.rs"), &[], &near), 1.0);

        // A new commit is seen by the next search: history is read per HEAD.
        fs::write(indexer.root().join("c.rs"), "fn c() {}\n").unwrap();
        commit_file(&repo, "c.rs", "gear");
        assert!(indexer.files_by_history("gear").contains("c.rs"));
    }

    #[test]
    fn a_plain_word_is_not_taken_for_a_name() {
        let indexer = indexed("search-plain", &[("a.rs", "fn sync() {}\n")], Arc::default());
        let result = search(&indexer, "how does sync work", None, 5, true).unwrap();
        assert!(result.matches.iter().all(|m| m.source != MatchSource::Symbol), "{:?}", summary(&result));
    }

    #[test]
    fn a_one_word_query_is_taken_for_a_name() {
        let indexer = indexed("search-one-word", &[("a.rs", "fn first() {}\n\nfn tokenize() {}\n")], Arc::default());
        let result = search(&indexer, "tokenize", None, 5, true).unwrap();
        // The chunk the declaration is in, not the file's first.
        let symbols: Vec<_> = summary(&result).into_iter().filter(|(_, s)| *s == MatchSource::Symbol).collect();
        assert_eq!(symbols, [("tokenize".to_string(), MatchSource::Symbol)]);
        assert_eq!((result.matches[0].start_line, result.matches[0].end_line), (3, 3));
        // In whatever case it was typed.
        assert_eq!(search(&indexer, "TOKENIZE", None, 5, true).unwrap().matches[0].source, MatchSource::Symbol);
    }

    // --------------------------------------------------- words and meaning

    #[test]
    fn meaning_finds_what_shares_no_word_with_the_query() {
        let indexer = indexed(
            "search-meaning",
            &[("drive.rs", "fn start_car() {\n    // turn the key\n}\n"), ("other.rs", "fn unrelated() {}\n")],
            Arc::default(),
        );

        let result = search(&indexer, "automobile", None, 1, true).unwrap();

        assert_eq!(summary(&result), [("start_car".to_string(), MatchSource::Semantic)]);
        assert!(result.meta.tiers_used.contains(&MatchSource::Semantic));
        // Found by words as well, it is still labelled by meaning.
        let both = search(&indexer, "turn the key", None, 1, true).unwrap();
        assert_eq!(summary(&both), [("start_car".to_string(), MatchSource::Semantic)]);
    }

    #[test]
    fn fusion_ranks_agreement_first_and_keeps_the_first_lists_label() {
        let id = |s: &str| ChunkId(s.into());
        let fused = fuse_rrf([
            (vec![id("a"), id("b")], MatchSource::Semantic, 1.0),
            (vec![id("b"), id("c")], MatchSource::Lexical, 1.0),
        ]);
        let order: Vec<_> = fused.into_iter().map(|(id, _, source)| (id, source)).collect();
        assert_eq!(
            order,
            [(id("b"), MatchSource::Semantic), (id("a"), MatchSource::Semantic), (id("c"), MatchSource::Lexical)]
        );
    }

    #[test]
    fn ties_go_to_the_earlier_list() {
        let id = |s: &str| ChunkId(s.into());
        for _ in 0..20 {
            let fused =
                fuse_rrf([(vec![id("z")], MatchSource::Semantic, 1.0), (vec![id("a")], MatchSource::Lexical, 1.0)]);
            assert_eq!(fused[0].0, id("z"));
        }
    }

    /// First by one measure beats sixth by both — the case the published
    /// K of 60 got wrong.
    #[test]
    fn a_first_place_outweighs_two_middling_ones() {
        let id = |s: &str| ChunkId(s.into());
        let filler = |p: &str| (0..5).map(|i| id(&format!("{p}{i}"))).collect::<Vec<_>>();
        let mut meaning = vec![id("top")];
        meaning.extend(filler("m"));
        meaning.push(id("both"));
        let mut words = filler("w");
        words.push(id("both"));

        let fused = fuse_rrf([(meaning, MatchSource::Semantic, 1.0), (words, MatchSource::Lexical, 1.0)]);

        assert_eq!(fused[0].0, id("top"));
    }

    /// Each is first by one measure and second by the other: words win.
    #[test]
    fn a_first_place_by_words_outweighs_one_by_meaning() {
        let indexer = indexed(
            "search-words-weigh",
            &[("x.rs", "fn x() {\n    wheel(wheel, wheel);\n}\n"), ("y.rs", "fn y() {\n    car(wheel);\n}\n")],
            Arc::default(),
        );
        let meaning = indexer.search_meaning("automobile wheel", 2).unwrap();
        assert_eq!(indexer.store().load_chunk(&meaning[0].0).unwrap().unwrap().file_id.0, "y.rs", "set-up");

        let result = search(&indexer, "automobile wheel", None, 2, true).unwrap();

        assert_eq!(result.matches[0].path, "x.rs", "{:?}", summary(&result));
    }

    /// Prose — Markdown or anything read as plain text — that matches as well
    /// as the code, or a little better, is listed after it.
    #[test]
    fn prose_gives_way_to_code_that_matches_as_well() {
        for notes in ["notes.md", "notes.adoc"] {
            let indexer = indexed(
                "search-prose",
                &[(notes, "# Notes\n\nwidget gear widget gear\n"), ("turn.rs", "fn turn() {\n    widget(gear);\n}\n")],
                Arc::default(),
            );
            let result = search(&indexer, "widget gear", None, 2, true).unwrap();
            assert_eq!(result.matches[0].path, "turn.rs", "{notes}: {:?}", summary(&result));
        }
    }

    /// Scaled down, not out: prose that is the answer by every measure stays
    /// above code found only by a vague likeness.
    #[test]
    fn prose_that_matches_far_better_still_comes_first() {
        let indexer = indexed(
            "search-prose-wins",
            &[("notes.md", "# Notes\n\nwidget gear widget gear\n"), ("turn.rs", "fn turn() {\n    spin();\n}\n")],
            Arc::default(),
        );
        let result = search(&indexer, "widget gear", None, 2, true).unwrap();
        assert_eq!(result.matches[0].path, "notes.md", "{:?}", summary(&result));
    }

    /// A query word naming the file lifts it over a closer match elsewhere.
    #[test]
    fn a_query_word_in_the_path_lifts_the_file() {
        let indexer = indexed(
            "search-path",
            &[("other.rs", "fn run() {\n    gear(gear);\n}\n"), ("widget.rs", "fn run() {\n    gear();\n}\n")],
            Arc::default(),
        );
        let result = search(&indexer, "widget gear", None, 2, true).unwrap();
        assert_eq!(result.matches[0].path, "widget.rs", "{:?}", summary(&result));
    }

    // ------------------------------------------------------------ limits

    #[test]
    fn a_file_changed_since_it_was_indexed_is_left_out() {
        let indexer = indexed(
            "search-stale",
            &[("a.rs", "fn widget_one() {}\n"), ("b.rs", "fn widget_two() {}\n")],
            Arc::default(),
        );
        fs::write(indexer.root().join("a.rs"), "fn widget_ONE() {}\n").unwrap();

        let result = search(&indexer, "widget", None, 5, true).unwrap();

        assert_eq!(result.matches.iter().map(|m| m.path.as_str()).collect::<Vec<_>>(), ["b.rs"]);
    }

    /// The best candidate is stale; the next one fills its place, even when
    /// only one match was asked for.
    #[test]
    fn a_stale_best_match_is_replaced_by_the_next() {
        let indexer = indexed(
            "search-stale-slack",
            &[("a.rs", "fn widget() { widget(); widget(); }
"), ("b.rs", "fn other() { widget(); }
")],
            Arc::default(),
        );
        fs::write(indexer.root().join("a.rs"), "fn changed() {}
").unwrap();

        let result = search(&indexer, "calls to widget", None, 1, true).unwrap();

        assert_eq!(result.matches.iter().map(|m| m.path.as_str()).collect::<Vec<_>>(), ["b.rs"]);
    }

    #[test]
    fn the_number_asked_for_is_held_between_one_and_the_maximum() {
        let body: String = (0..60).map(|i| format!("fn gadget_{i}() {{ gadget }}\n")).collect();
        let indexer = indexed("search-limits", &[("a.rs", &body)], Arc::default());
        assert_eq!(search(&indexer, "gadget", None, 0, true).unwrap().matches.len(), 1);
        assert_eq!(search(&indexer, "gadget", None, 500, true).unwrap().matches.len(), MAX_TOP_K);
    }

    // ------------------------------------------------ meaning unavailable

    /// The model loaded for the sync and fails for the query: search still
    /// answers, on words, and says so.
    #[test]
    fn a_failing_model_leaves_words_and_says_why() {
        let model = Arc::new(FakeModel::default());
        let indexer = indexed("search-fails", &[("a.rs", "fn gizmo() {}\n")], Arc::clone(&model));
        model.fail.store(true, Ordering::SeqCst);

        // A plain word, so no name lookup: only words can find it.
        let result = search(&indexer, "what does gizmo do", None, 5, true).unwrap();

        assert_eq!(result.matches.len(), 1);
        assert!(!result.meta.tiers_used.contains(&MatchSource::Semantic));
        assert!(result.meta.hint.as_deref().is_some_and(|h| h.contains("model.safetensors")), "{:?}", result.meta);
    }

    /// With the model down, only the word ranking answers — which makes what
    /// it searched for visible.
    #[test]
    fn fts_is_what_the_words_are_matched_against() {
        let model = Arc::new(FakeModel::default());
        let indexer = indexed("search-fts", &[("a.rs", "fn gizmo() {}\n")], Arc::clone(&model));
        model.fail.store(true, Ordering::SeqCst);

        let terms = ["gizmo".to_string()];
        assert_eq!(search(&indexer, "the thing that starts up", Some(&terms), 5, true).unwrap().matches.len(), 1);
        // Nothing searchable in `fts`: the query's own words are used.
        let noise = ["!!".to_string()];
        assert_eq!(search(&indexer, "what does gizmo do", Some(&noise), 5, true).unwrap().matches.len(), 1);
    }

    /// Never embedded because the model would not load: nothing to search by
    /// meaning, and the hint must not say "wait for it".
    #[test]
    fn a_model_that_never_loaded_is_named_rather_than_waited_for() {
        let model = Arc::new(FakeModel { fail: AtomicBool::new(true) });
        let indexer = indexed("search-no-model", &[("a.rs", "fn gizmo() {}\n")], model);

        let result = search(&indexer, "what does gizmo do", None, 5, true).unwrap();

        let hint = result.meta.hint.unwrap();
        assert!(hint.contains("unavailable") && !hint.contains("finished building"), "{hint}");
    }
}
