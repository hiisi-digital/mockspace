//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! One index per repository over its mock dir, and `mock ask` over it.
//!
//! A project opts in through `[corpus]` in `mockspace.toml`. From then on
//! the bare run builds the index under `<mock>/.muisti/`, incrementally, so a
//! round that closes is findable by the next question, and `mock ask` puts a
//! question in words to it and quotes the passages that clear the threshold
//! or says nothing did. The directory ignores itself, since an index is
//! generated and never committed, and the walk that fills it keeps itself
//! out of it.
//!
//! The crate under this is `muisti`, which hardwires no model; this engine
//! picks `potion-base-8M`, the static embedder with no runtime beneath it,
//! since every consumer builds the engine and pays for whatever it links.
//! The answer stays extractive for the same reason: the generator wants
//! llama.cpp and a C++ toolchain on every machine that runs `cargo mock`,
//! which is nothing a consumer asked for.

use std::fmt::{self, Write as _};
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use muisti::embed::Potion;
use muisti::{Corpus, Embedder, Header, Hit, Indexed, RERANK_DEPTH, Store, Threshold, search};

use crate::config::{Config, CorpusConfig};

/// The directory under the mock dir the index lives in, and what makes a
/// project that opted in tell from one that did not.
pub const DIR: &str = ".muisti";
/// The index file inside it.
pub const INDEX_FILE: &str = "index.sqlite";
/// The ignore file the directory carries, so nothing generated here reaches
/// a commit whatever the repository's own ignore rules say.
pub const IGNORE: &str = "# Written by mockspace. The corpus index is generated on every run and never \
                          committed.\n*\n";
/// The threshold a project that names none gets, the crate's own.
pub const DEFAULT_THRESHOLD: f32 = 0.75;
/// How many near misses a refusal shows.
pub const NEAR_MISSES: usize = 3;
/// The model cache, shared with every other tool built on the same crate so
/// a model downloads once per machine.
pub const CACHE: &str = ".cache/muisti";

/// `<mock>/.muisti`.
#[must_use]
pub fn dir(mock_dir: &Path) -> PathBuf {
    mock_dir.join(DIR)
}

/// `<mock>/.muisti/index.sqlite`.
#[must_use]
pub fn index_path(mock_dir: &Path) -> PathBuf {
    dir(mock_dir).join(INDEX_FILE)
}

/// Make the index directory and its ignore file. Idempotent, and the ignore
/// file is rewritten only when it does not already say what it should.
///
/// # Errors
///
/// The filesystem's, with the path.
pub fn ensure_dir(mock_dir: &Path) -> Result<PathBuf, String> {
    let d = dir(mock_dir);
    fs::create_dir_all(&d).map_err(|e| format!("could not make {}: {e}", d.display()))?;
    let ignore = d.join(".gitignore");
    if fs::read_to_string(&ignore).ok().as_deref() != Some(IGNORE) {
        fs::write(&ignore, IGNORE).map_err(|e| format!("could not write {}: {e}", ignore.display()))?;
    }
    Ok(d)
}

/// The threshold the config names, or the default.
///
/// # Errors
///
/// A number outside the unit interval.
pub fn threshold(c: &CorpusConfig) -> Result<Threshold, String> {
    let t = c.threshold.unwrap_or(DEFAULT_THRESHOLD);
    Threshold::new(t).ok_or_else(|| format!("[corpus] threshold is {t}; it is a number from 0 to 1"))
}

/// The walk over the mock dir, with the index directory kept out of it and
/// the project's own globs applied.
fn corpus(mock_dir: &Path, c: &CorpusConfig) -> Corpus {
    let mut w = Corpus::new(mock_dir).exclude(format!("{DIR}/**"));
    for g in &c.include {
        w = w.include(g.as_str());
    }
    for g in &c.exclude {
        w = w.exclude(g.as_str());
    }
    w
}

/// The store under the embedder's header, made if absent.
fn open<E: Embedder>(mock_dir: &Path, embedder: &E) -> Result<Store, String> {
    ensure_dir(mock_dir)?;
    let path = index_path(mock_dir);
    let dimension = NonZeroUsize::new(embedder.dimension())
        .ok_or_else(|| "the embedder has no dimension".to_owned())?;
    Store::open(&path, Header::new(embedder.model(), dimension)).map_err(|e| {
        format!(
            "{}: {e}\n  an index another model wrote is not reused; delete {} and run again",
            path.display(),
            dir(mock_dir).display()
        )
    })
}

/// The model cache under the home directory.
fn cache() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(CACHE))
        .ok_or_else(|| "HOME is not set, so there is nowhere to cache the model".to_owned())
}

fn potion() -> Result<Potion, String> {
    let cache = cache()?;
    Potion::load(&cache).map_err(|e| format!("could not load potion-base-8M into {}: {e}", cache.display()))
}

/// What one build did, in counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Built {
    pub written:   usize,
    pub unchanged: usize,
    pub removed:   usize,
    pub skipped:   usize,
    /// Chunks in the index afterwards.
    pub chunks:    usize,
}

impl Built {
    fn count(&mut self, e: &Indexed) {
        match e {
            Indexed::Written {
                ..
            } => self.written += 1,
            Indexed::Unchanged(_) => self.unchanged += 1,
            Indexed::Removed(_) => self.removed += 1,
            Indexed::Skipped {
                ..
            } => self.skipped += 1,
        }
    }
}

impl fmt::Display for Built {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} written, {} unchanged, {} removed, {} skipped; {} chunks",
            self.written, self.unchanged, self.removed, self.skipped, self.chunks
        )
    }
}

/// Build the index over `mock_dir` with `embedder`, reporting each source
/// to `sink`. Incremental: a source whose bytes the store already holds is
/// unchanged, one the walk no longer reaches is removed.
///
/// # Errors
///
/// The store's, the walk's, and a glob that does not parse.
pub fn build_with<E: Embedder>(
    mock_dir: &Path,
    c: &CorpusConfig,
    embedder: &mut E,
    mut sink: impl FnMut(&Indexed),
) -> Result<Built, String> {
    let mut store = open(mock_dir, embedder)?;
    let mut built = Built::default();
    muisti::index(&mut store, &corpus(mock_dir, c), embedder, |e| {
        built.count(&e);
        sink(&e);
    })
    .map_err(|e| e.to_string())?;
    built.chunks = store.chunk_count().map_err(|e| e.to_string())?;
    Ok(built)
}

/// The bare run's step: build the index for a project that opted in. Skips
/// are printed as they happen, since one binary among the writing is the
/// ordinary state of a repository and a reader wants to know which.
///
/// # Errors
///
/// No `[corpus]`, no model, or what `build_with` refuses.
pub fn build(cfg: &Config, out: &mut String) -> Result<Built, String> {
    let c = cfg.corpus.as_ref().ok_or_else(|| "this project has no [corpus]".to_owned())?;
    let mut embedder = potion()?;
    build_with(&cfg.mock_dir, c, &mut embedder, |e| {
        if let Indexed::Skipped {
            path,
            why,
        } = e
        {
            let _ = writeln!(out, "  skipped {path}: {why}");
        }
    })
}

/// The arguments after `ask`: the question, in as many words as it takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskArgs {
    pub question: String,
}

impl AskArgs {
    /// Every argument is a word of the question, so a question needs no
    /// quoting. `--answer` is named so a reader who expects it is told why
    /// it is not here rather than having it read as a word.
    ///
    /// # Errors
    ///
    /// No question, or a flag this engine does not carry.
    pub fn parse(args: &[&str]) -> Result<Self, String> {
        const USAGE: &str = "mock ask <question...>";
        if let Some(flag) = args.iter().find(|a| a.starts_with("--")) {
            return Err(match *flag {
                "--answer" => {
                    "--answer: this engine carries no generator, so the answer is the passages \
                     that clear, quoted; a generated form wants a build the launcher cannot \
                     carry yet"
                        .to_owned()
                },
                other => format!("{other} is not an option of `ask`\n  usage: {USAGE}"),
            });
        }
        let question = args.join(" ");
        if question.trim().is_empty() {
            return Err(format!("ask what?\n  usage: {USAGE}"));
        }
        Ok(Self {
            question,
        })
    }
}

fn location(h: &Hit) -> String {
    let lines = h.chunk.chunk.lines();
    format!(
        "{}:{}-{}",
        h.chunk.path,
        lines.start,
        lines.end.saturating_sub(1).max(lines.start)
    )
}

fn print_hit(h: &Hit, out: &mut String) {
    let arms = match (h.lexical, h.dense) {
        (Some(l), Some(d)) => format!("lexical {l}, dense {d}"),
        (Some(l), None) => format!("lexical {l}"),
        (None, Some(d)) => format!("dense {d}"),
        (None, None) => String::new(),
    };
    let _ = writeln!(out, "{}  {:.2}  ({arms})", location(h), h.score);
    for line in h.chunk.chunk.text().lines() {
        let _ = writeln!(out, "    {line}");
    }
    out.push('\n');
}

/// Ask `question` of the index under `mock_dir` with `embedder`, writing the
/// passages that clear the threshold, quoted under their location, or the
/// refusal with the near misses under it.
///
/// # Errors
///
/// No index yet, a store another model wrote, or the search's own.
pub fn ask_with<E: Embedder>(
    mock_dir: &Path,
    c: &CorpusConfig,
    embedder: &mut E,
    question: &str,
    out: &mut String,
) -> Result<(), String> {
    let path = index_path(mock_dir);
    if !path.is_file() {
        return Err(format!(
            "no index at {}; run `cargo mock` once to build it",
            path.display()
        ));
    }
    let t = threshold(c)?;
    let store = open(mock_dir, embedder)?;
    let mut found = Vec::new();
    search(&store, question, embedder, RERANK_DEPTH, |h| found.push(h)).map_err(|e| e.to_string())?;
    if found.first().is_some_and(|h| h.clears(t)) {
        for h in found.iter().filter(|h| h.clears(t)) {
            let _ = writeln!(out, "{}", location(h));
            for line in h.chunk.chunk.text().lines() {
                let _ = writeln!(out, "> {line}");
            }
            out.push('\n');
        }
    } else {
        let _ = writeln!(
            out,
            "nothing clears the threshold of {:.2} for {question:?}; the near misses:",
            t.get()
        );
        for h in found.iter().take(NEAR_MISSES) {
            print_hit(h, out);
        }
    }
    Ok(())
}

/// `mock ask`, for a project that opted in.
///
/// # Errors
///
/// No `[corpus]`, no model, or what `ask_with` refuses.
pub fn ask(cfg: &Config, question: &str, out: &mut String) -> Result<(), String> {
    let c = cfg.corpus.as_ref().ok_or_else(|| {
        "this project has no [corpus] in mockspace.toml, so there is no index to ask".to_owned()
    })?;
    let mut embedder = potion()?;
    ask_with(&cfg.mock_dir, c, &mut embedder, question, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A planted embedder: the vector is the byte histogram of the text,
    /// which is deterministic and makes two texts sharing words near.
    struct Planted;

    impl Embedder for Planted {
        fn model(&self) -> &str {
            "planted"
        }

        fn dimension(&self) -> usize {
            8
        }

        fn embed(&mut self, text: &str, out: &mut [f32]) -> Result<(), muisti::Error> {
            out.fill(0.0);
            for b in text.bytes() {
                out[(b as usize) % 8] += 1.0;
            }
            Ok(())
        }
    }

    struct Other;

    impl Embedder for Other {
        fn model(&self) -> &str {
            "other"
        }

        fn dimension(&self) -> usize {
            8
        }

        fn embed(&mut self, _: &str, out: &mut [f32]) -> Result<(), muisti::Error> {
            out.fill(1.0);
            Ok(())
        }
    }

    fn cfg() -> CorpusConfig {
        CorpusConfig {
            threshold: None,
            include:   Vec::new(),
            exclude:   Vec::new(),
        }
    }

    fn planted_mock_dir() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let rounds = tmp.path().join("design_rounds");
        fs::create_dir_all(&rounds).unwrap();
        fs::write(
            rounds.join("202609050954_topic.the-document-store.md"),
            "# The document store\n\nA fetched paper is kept under the hash of its bytes with a \
             record beside it naming the url.\n",
        )
        .unwrap();
        fs::write(
            rounds.join("202609050954_changelist.doc.md.meta"),
            "locked = true\n",
        )
        .unwrap();
        fs::write(tmp.path().join("research.md"), "Reranking with a cross encoder costs a model of two gigabytes.\n")
            .unwrap();
        tmp
    }

    #[test]
    fn the_index_directory_ignores_itself_and_the_second_ensure_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let d = ensure_dir(tmp.path()).unwrap();
        assert_eq!(d, tmp.path().join(".muisti"));
        let ignore = d.join(".gitignore");
        assert_eq!(fs::read_to_string(&ignore).unwrap(), IGNORE);
        let before = fs::metadata(&ignore).unwrap().modified().unwrap();
        ensure_dir(tmp.path()).unwrap();
        assert_eq!(fs::metadata(&ignore).unwrap().modified().unwrap(), before);
        // A wrong ignore file is put right rather than left.
        fs::write(&ignore, "index.sqlite\n").unwrap();
        ensure_dir(tmp.path()).unwrap();
        assert_eq!(fs::read_to_string(&ignore).unwrap(), IGNORE);
    }

    #[test]
    fn the_threshold_defaults_reads_and_refuses_outside_the_unit_interval() {
        assert_eq!(threshold(&cfg()).unwrap().get(), DEFAULT_THRESHOLD);
        let c = CorpusConfig {
            threshold: Some(0.5),
            ..cfg()
        };
        assert_eq!(threshold(&c).unwrap().get(), 0.5);
        for bad in [-0.1, 1.5] {
            let c = CorpusConfig {
                threshold: Some(bad),
                ..cfg()
            };
            let e = threshold(&c).unwrap_err();
            assert!(e.contains("[corpus] threshold"), "{e}");
        }
    }

    #[test]
    fn a_build_indexes_the_writing_and_not_the_machinery_and_a_second_is_unchanged() {
        let tmp = planted_mock_dir();
        let mut seen = Vec::new();
        let built = build_with(tmp.path(), &cfg(), &mut Planted, |e| seen.push(e.clone())).unwrap();
        assert_eq!(built.written, 2, "{seen:?}");
        assert_eq!(built.skipped, 0, "{seen:?}");
        assert!(built.chunks >= 2);
        assert!(index_path(tmp.path()).is_file());
        assert!(
            !seen.iter().any(|e| matches!(e, Indexed::Written { path, .. } if path.ends_with(".meta"))),
            "{seen:?}"
        );
        let again = build_with(tmp.path(), &cfg(), &mut Planted, |_| {}).unwrap();
        assert_eq!(again.written, 0);
        assert_eq!(again.unchanged, 2);
        assert_eq!(built.to_string(), format!("2 written, 0 unchanged, 0 removed, 0 skipped; {} chunks", built.chunks));
    }

    #[test]
    fn the_index_never_indexes_itself_and_a_removed_source_leaves() {
        let tmp = planted_mock_dir();
        build_with(tmp.path(), &cfg(), &mut Planted, |_| {}).unwrap();
        // The index file and its ignore file exist now and are not sources.
        let mut seen = Vec::new();
        build_with(tmp.path(), &cfg(), &mut Planted, |e| seen.push(e.clone())).unwrap();
        assert!(
            !seen.iter().any(|e| {
                matches!(e, Indexed::Written { path, .. } | Indexed::Unchanged(path) if path.starts_with(".muisti"))
            }),
            "{seen:?}"
        );
        fs::remove_file(tmp.path().join("research.md")).unwrap();
        let built = build_with(tmp.path(), &cfg(), &mut Planted, |_| {}).unwrap();
        assert_eq!(built.removed, 1);
        assert_eq!(built.unchanged, 1);
    }

    #[test]
    fn the_globs_bound_the_walk() {
        let tmp = planted_mock_dir();
        let c = CorpusConfig {
            threshold: None,
            include:   vec!["design_rounds/**".to_owned()],
            exclude:   Vec::new(),
        };
        let built = build_with(tmp.path(), &c, &mut Planted, |_| {}).unwrap();
        assert_eq!(built.written, 1);
        let c = CorpusConfig {
            threshold: None,
            include:   Vec::new(),
            exclude:   vec!["design_rounds/**".to_owned()],
        };
        let tmp = planted_mock_dir();
        let built = build_with(tmp.path(), &c, &mut Planted, |_| {}).unwrap();
        assert_eq!(built.written, 1);
        let c = CorpusConfig {
            threshold: None,
            include:   vec!["[".to_owned()],
            exclude:   Vec::new(),
        };
        assert!(build_with(tmp.path(), &c, &mut Planted, |_| {}).is_err());
    }

    #[test]
    fn an_index_another_model_wrote_is_refused_and_says_what_to_do() {
        let tmp = planted_mock_dir();
        build_with(tmp.path(), &cfg(), &mut Planted, |_| {}).unwrap();
        let e = build_with(tmp.path(), &cfg(), &mut Other, |_| {}).unwrap_err();
        assert!(e.contains("delete"), "{e}");
        let mut out = String::new();
        let e = ask_with(tmp.path(), &cfg(), &mut Other, "anything", &mut out).unwrap_err();
        assert!(e.contains("delete"), "{e}");
    }

    #[test]
    fn ask_quotes_what_clears_and_refuses_with_near_misses_otherwise() {
        let tmp = planted_mock_dir();
        build_with(tmp.path(), &cfg(), &mut Planted, |_| {}).unwrap();
        let mut out = String::new();
        ask_with(tmp.path(), &cfg(), &mut Planted, "record beside a fetched paper naming the url", &mut out).unwrap();
        assert!(out.contains("> A fetched paper is kept"), "{out}");
        assert!(out.contains("design_rounds/202609050954_topic.the-document-store.md:1-"), "{out}");
        assert!(!out.contains("near misses"), "{out}");
        // A chunk first in both arms measures one, so a threshold of one is
        // cleared by the one chunk holding a word; the refusal wants a
        // question no chunk holds a word of, where the dense arm alone puts
        // every hit at a half.
        let mut out = String::new();
        ask_with(tmp.path(), &cfg(), &mut Planted, "xylophone quartz", &mut out).unwrap();
        assert!(out.starts_with("nothing clears the threshold of 0.75"), "{out}");
        assert!(out.contains("research.md:1-1  0."), "{out}");
        assert!(out.contains("  (dense 1)\n") && out.contains("  (dense 2)\n"), "{out}");
        assert!(!out.contains("lexical"), "{out}");
        assert!(out.contains("    Reranking with"), "{out}");
        assert!(!out.contains("> "), "{out}");
        // And a chunk holding the one word the question has is quoted alone.
        let mut out = String::new();
        ask_with(tmp.path(), &cfg(), &mut Planted, "gigabytes", &mut out).unwrap();
        assert!(out.starts_with("research.md:1-1\n> Reranking"), "{out}");
        assert!(!out.contains("design_rounds"), "{out}");
    }

    #[test]
    fn ask_before_any_build_says_to_run_the_tool() {
        let tmp = tempfile::tempdir().unwrap();
        let mut out = String::new();
        let e = ask_with(tmp.path(), &cfg(), &mut Planted, "anything", &mut out).unwrap_err();
        assert!(e.contains("run `cargo mock`"), "{e}");
        assert!(!dir(tmp.path()).exists(), "asking made nothing");
    }

    #[test]
    fn the_question_is_the_words_and_the_flags_are_refused_by_name() {
        assert_eq!(
            AskArgs::parse(&["where", "is", "the", "store"]).unwrap().question,
            "where is the store"
        );
        assert_eq!(AskArgs::parse(&["one question"]).unwrap().question, "one question");
        let e = AskArgs::parse(&[]).unwrap_err();
        assert!(e.contains("usage"), "{e}");
        let e = AskArgs::parse(&["  "]).unwrap_err();
        assert!(e.contains("usage"), "{e}");
        let e = AskArgs::parse(&["what", "--answer"]).unwrap_err();
        assert!(e.contains("no generator"), "{e}");
        let e = AskArgs::parse(&["--limit", "3", "what"]).unwrap_err();
        assert!(e.contains("--limit is not an option"), "{e}");
    }
}
