//! What a tool asks the repository's index, and what it is told back.
//!
//! The index itself lives in the engine, in the user cache, and a tool is a dynamic library built
//! apart from it. So the two meet here, at plain data and one trait, and nothing of the store's
//! own crates crosses: a tool that links this links no database.

/// Who wrote a document, which decides how much a hit in it is worth.
///
/// Read off the root a document sits under, never off its name: a name is not provenance.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum DocClass {
    /// A human's own words, captured verbatim. The highest tier there is.
    Human,
    /// A ratified registry file: what the project has settled.
    Canon,
    /// A design round's topic or changelist, written by an agent in a session.
    Design,
    /// A research note, or the data kept beside it.
    Research,
    /// Code written to try one thing. It says the thing at length, and the length wins a count.
    Sketch,
}

impl DocClass {
    /// The word a report prints.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Canon => "canon",
            Self::Design => "design",
            Self::Research => "research",
            Self::Sketch => "sketch",
        }
    }

    /// The multiplier a hit in this class carries: where a settled answer lives, not how good the
    /// writing is.
    #[must_use]
    pub fn weight(self) -> f64 {
        match self {
            Self::Human => 1.9,
            Self::Canon => 1.5,
            Self::Design | Self::Research => 1.0,
            Self::Sketch => 0.6,
        }
    }

    /// The inverse of [`Self::label`].
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        [Self::Human, Self::Canon, Self::Design, Self::Research, Self::Sketch]
            .into_iter()
            .find(|c| c.label() == label)
    }
}

/// One question.
#[derive(Clone, Debug)]
pub struct IndexQuery<'a> {
    /// The question in the asker's own words.
    pub text:   &'a str,
    /// Words to ask in as well, such as the keywords of the rows that carry the question.
    pub expand: &'a [String],
    /// Match the words only as one phrase, in the order written and side by side, with no expansion
    /// and no match on some of them. For asking whether a passage was ever written.
    pub phrase: bool,
    /// How many distinct hits to return. Copies of one passage fold into a hit and do not count.
    pub limit:  usize,
    /// Today, as days since 1970-01-01, so an age is computed the same way in a test and a run.
    pub today:  i64,
}

/// One document that answers, with its copies folded in.
#[derive(Clone, Debug)]
pub struct IndexHit {
    /// Relative to the repository root, or the captures root's shown name for a human's words.
    pub path:      String,
    /// The line most worth opening, one-indexed.
    pub line:      usize,
    pub class:     DocClass,
    /// The document's own date as days since 1970-01-01: its name's or front matter's stamp, else
    /// the day it was first committed. `None` where neither exists.
    pub day:       Option<i64>,
    /// `today` less `day`.
    pub age_days:  Option<i64>,
    /// Higher is better, comparable within one report only.
    pub score:     f64,
    /// The line, trimmed to a short length.
    pub excerpt:   String,
    /// Other documents carrying the same text, which this hit stands for.
    pub copies:    Vec<String>,
}

/// What a search found, and what it could not have found.
#[derive(Clone, Debug, Default)]
pub struct IndexReport {
    pub hits:         Vec<IndexHit>,
    /// Documents that matched at all, before the limit.
    pub matched:      usize,
    /// Documents in the index, so an empty report can be told from a blind one.
    pub indexed:      usize,
    /// Question words found in no document.
    pub dead_terms:   Vec<String>,
    /// What was not read, one sentence each.
    pub caveats:      Vec<String>,
}

/// The repository's index, as a tool sees it.
pub trait RepoIndex {
    /// Rank the documents against a question.
    fn search(&self, query: &IndexQuery<'_>) -> IndexReport;

    /// How many documents carry a word, by its stem. Zero for a word no document has.
    fn doc_frequency(&self, word: &str) -> usize;

    /// The day, since 1970-01-01, of the last commit that touched a path relative to the repository
    /// root, so a hit can be marked as older than the canon's last word on its subject. `None`
    /// where git has no commit for it or the index has no repository to ask.
    fn changed_day(&self, _repo_path: &str) -> Option<i64> {
        None
    }
}

/// The index of a context that has none, such as a tool's unit test or a repository with nothing to
/// read. It answers empty to everything and says so in its report.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoIndex;

impl RepoIndex for NoIndex {
    fn search(&self, _query: &IndexQuery<'_>) -> IndexReport {
        IndexReport {
            caveats: vec!["no index was built for this run, so nothing was searched".to_string()],
            ..IndexReport::default()
        }
    }

    fn doc_frequency(&self, _word: &str) -> usize {
        0
    }
}
