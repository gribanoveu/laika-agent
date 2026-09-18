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
//! Upstream also matched name stems (`notifications` → `NotificationService`)
//! and path segments. Not ported: the full-text index already weighs chunk
//! names four to one and matches prefixes, which covers the stem case, and a
//! file name is `listFiles`' question. The quality measurement (F-5.13c)
//! decides whether either comes back.

use std::collections::{HashMap, HashSet};

use crate::domain::chunk_index::ChunkId;
use crate::domain::code_search::{CodeMatch, CodeSearchResult, MatchSource, SearchMeta};
use crate::domain::search_query::{
    SearchMetaInput, extract_search_tokens, fts5_query, looks_like_identifier, weak_search_hint,
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

/// Reciprocal-rank fusion's smoothing constant, the value it was published
/// with: large enough that rank 1 against rank 2 does not drown out a second
/// list's opinion, small enough that being ranked at all still counts.
const RRF_K: f32 = 60.0;

pub fn search(indexer: &RepoIndexer, query: &str, top_k: usize) -> Result<CodeSearchResult, IndexStoreError> {
    let top_k = top_k.clamp(1, MAX_TOP_K);
    let store = indexer.store();
    let tokens = extract_search_tokens(query);
    let mut ranked: Vec<(ChunkId, MatchSource)> = Vec::new();

    for name in names_in(query, &tokens) {
        for id in store.chunks_declaring(&name, top_k)? {
            ranked.push((id, MatchSource::Symbol));
        }
    }

    let candidates = top_k * CANDIDATES_PER_MATCH;
    let lexical = match fts5_query(query) {
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
    // Semantic first: a chunk both found is labelled by the more telling one.
    ranked.extend(fuse_rrf([(semantic, MatchSource::Semantic), (lexical, MatchSource::Lexical)]));

    let mut seen = HashSet::new();
    let mut matches = Vec::with_capacity(top_k);
    for (id, source) in ranked {
        if matches.len() == top_k {
            break;
        }
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(chunk) = store.load_chunk(&id)? else { continue };
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

/// Merges rankings by `Σ 1 / (K + rank)` over the lists that hold a chunk.
///
/// Rank, not score: cosine similarity and BM25 measure incomparable things,
/// and no fixed factor makes them commensurable across queries. A chunk keeps
/// the source of the first list it is in; ties go to the earlier list.
fn fuse_rrf<const N: usize>(lists: [(Vec<ChunkId>, MatchSource); N]) -> Vec<(ChunkId, MatchSource)> {
    let mut fused: HashMap<ChunkId, (f32, (usize, usize), MatchSource)> = HashMap::new();
    for (order, (list, source)) in lists.into_iter().enumerate() {
        for (rank, id) in list.into_iter().enumerate() {
            let contribution = 1.0 / (RRF_K + rank as f32 + 1.0);
            fused.entry(id).and_modify(|entry| entry.0 += contribution).or_insert((contribution, (order, rank), source));
        }
    }
    let mut out: Vec<_> = fused.into_iter().collect();
    out.sort_by(|(_, a), (_, b)| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    out.into_iter().map(|(id, (_, _, source))| (id, source)).collect()
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

        let result = search(&indexer, "where is parse_config", 5).unwrap();

        let first = &result.matches[0];
        assert_eq!((first.path.as_str(), first.source), ("config.rs", MatchSource::Symbol));
        assert_eq!((first.start_line, first.end_line), (1, 5));
        assert_eq!(result.matches.iter().filter(|m| m.path == "config.rs").count(), 1, "listed twice");
    }

    #[test]
    fn a_plain_word_is_not_taken_for_a_name() {
        let indexer = indexed("search-plain", &[("a.rs", "fn sync() {}\n")], Arc::default());
        let result = search(&indexer, "how does sync work", 5).unwrap();
        assert!(result.matches.iter().all(|m| m.source != MatchSource::Symbol), "{:?}", summary(&result));
    }

    #[test]
    fn a_one_word_query_is_taken_for_a_name() {
        let indexer = indexed("search-one-word", &[("a.rs", "fn first() {}\n\nfn tokenize() {}\n")], Arc::default());
        let result = search(&indexer, "tokenize", 5).unwrap();
        // The chunk the declaration is in, not the file's first.
        let symbols: Vec<_> = summary(&result).into_iter().filter(|(_, s)| *s == MatchSource::Symbol).collect();
        assert_eq!(symbols, [("tokenize".to_string(), MatchSource::Symbol)]);
        assert_eq!((result.matches[0].start_line, result.matches[0].end_line), (3, 3));
        // In whatever case it was typed.
        assert_eq!(search(&indexer, "TOKENIZE", 5).unwrap().matches[0].source, MatchSource::Symbol);
    }

    // --------------------------------------------------- words and meaning

    #[test]
    fn meaning_finds_what_shares_no_word_with_the_query() {
        let indexer = indexed(
            "search-meaning",
            &[("drive.rs", "fn start_car() {\n    // turn the key\n}\n"), ("other.rs", "fn unrelated() {}\n")],
            Arc::default(),
        );

        let result = search(&indexer, "automobile", 1).unwrap();

        assert_eq!(summary(&result), [("start_car".to_string(), MatchSource::Semantic)]);
        assert!(result.meta.tiers_used.contains(&MatchSource::Semantic));
        // Found by words as well, it is still labelled by meaning.
        let both = search(&indexer, "turn the key", 1).unwrap();
        assert_eq!(summary(&both), [("start_car".to_string(), MatchSource::Semantic)]);
    }

    #[test]
    fn fusion_ranks_agreement_first_and_keeps_the_first_lists_label() {
        let id = |s: &str| ChunkId(s.into());
        let fused = fuse_rrf([
            (vec![id("a"), id("b")], MatchSource::Semantic),
            (vec![id("b"), id("c")], MatchSource::Lexical),
        ]);
        assert_eq!(
            fused,
            [(id("b"), MatchSource::Semantic), (id("a"), MatchSource::Semantic), (id("c"), MatchSource::Lexical)]
        );
    }

    #[test]
    fn ties_go_to_the_earlier_list() {
        let id = |s: &str| ChunkId(s.into());
        for _ in 0..20 {
            let fused = fuse_rrf([(vec![id("z")], MatchSource::Semantic), (vec![id("a")], MatchSource::Lexical)]);
            assert_eq!(fused[0].0, id("z"));
        }
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

        let result = search(&indexer, "widget", 5).unwrap();

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

        let result = search(&indexer, "calls to widget", 1).unwrap();

        assert_eq!(result.matches.iter().map(|m| m.path.as_str()).collect::<Vec<_>>(), ["b.rs"]);
    }

    #[test]
    fn the_number_asked_for_is_held_between_one_and_the_maximum() {
        let body: String = (0..60).map(|i| format!("fn gadget_{i}() {{ gadget }}\n")).collect();
        let indexer = indexed("search-limits", &[("a.rs", &body)], Arc::default());
        assert_eq!(search(&indexer, "gadget", 0).unwrap().matches.len(), 1);
        assert_eq!(search(&indexer, "gadget", 500).unwrap().matches.len(), MAX_TOP_K);
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
        let result = search(&indexer, "what does gizmo do", 5).unwrap();

        assert_eq!(result.matches.len(), 1);
        assert!(!result.meta.tiers_used.contains(&MatchSource::Semantic));
        assert!(result.meta.hint.as_deref().is_some_and(|h| h.contains("model.safetensors")), "{:?}", result.meta);
    }

    /// Never embedded because the model would not load: nothing to search by
    /// meaning, and the hint must not say "wait for it".
    #[test]
    fn a_model_that_never_loaded_is_named_rather_than_waited_for() {
        let model = Arc::new(FakeModel { fail: AtomicBool::new(true) });
        let indexer = indexed("search-no-model", &[("a.rs", "fn gizmo() {}\n")], model);

        let result = search(&indexer, "what does gizmo do", 5).unwrap();

        let hint = result.meta.hint.unwrap();
        assert!(hint.contains("unavailable") && !hint.contains("finished building"), "{hint}");
    }
}
