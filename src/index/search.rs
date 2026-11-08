//! Ranking the index's documents against a question.
//!
//! BM25 over the full-text table does the weighing that a count of matching words did badly: a
//! word every document has barely counts, a word one document has counts a lot, and a long
//! document is not favoured for being long. A heading is weighted over the body, a human's words
//! over an agent's, and copies of one text are folded into the best-ranked of them.

use std::collections::BTreeSet;

use mockspace_lint_rules::{DocClass, IndexHit, IndexQuery, IndexReport, RepoIndex};

use super::{Index, shingle};

/// Words that carry no subject, in a question or a heading.
const STOP: &[&str] = &[
    "the", "and", "for", "are", "was", "were", "with", "that", "this", "these", "those", "from",
    "has", "have", "had", "not", "but", "can", "its", "all", "any", "does", "did", "into", "how",
    "what", "when", "where", "which", "who", "why", "will", "would", "should", "could", "than",
    "then", "them", "they", "their", "there", "about", "over", "under", "also", "just", "only",
    "out", "our", "your", "you", "per", "via", "off", "one", "two", "use", "uses", "used",
];

/// How many candidates are weighed after the full-text pass, before the limit. Wide, because
/// folding copies and the class weight both reorder what the full-text pass ranked.
const CANDIDATES: usize = 200;

/// The weight a heading's words carry over the body's, in BM25 column weights.
const HEADING: f64 = 3.0;

/// What an expansion word is worth against a word of the question itself.
const EXPANDED: f64 = 0.5;

/// How many of the question's words, the rarest, must sit together for the proximity pass.
const NEAR_TERMS: usize = 4;

/// How many words apart they may sit.
const NEAR_WINDOW: usize = 12;

/// What saying the words together is worth over saying them anywhere, as a multiple of the
/// ordinary score added on top of it.
const NEARNESS: f64 = 1.5;

/// The content words of a text, in order, without repeats.
#[must_use]
pub fn terms_of(text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    shingle::words(text)
        .filter(|w| w.chars().count() >= 3 && !STOP.contains(&w.as_str()))
        .filter(|w| seen.insert(w.clone()))
        .collect()
}

fn match_expr(terms: &[String]) -> String {
    terms
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

struct Cand {
    id:    i64,
    score: f64,
}

/// A pass's candidates with scores as a fraction of the pass's best, times `weight`.
///
/// BM25 is not comparable between passes: a proximity match is one phrase in a long document and
/// scores low beside a count of words in a short one, though it is the stronger evidence. Each
/// pass is measured against its own best and then weighed, so what a pass means is a choice made
/// here and not an accident of document length.
fn normalised(mut cands: Vec<Cand>, weight: f64) -> Vec<Cand> {
    let best = cands.iter().map(|c| c.score).fold(0.0_f64, f64::max);
    if best > 0.0 {
        for c in &mut cands {
            c.score = c.score / best * weight;
        }
    }
    cands
}

impl Index {
    fn ranked(&self, terms: &[String], weight: f64) -> Vec<Cand> {
        if terms.is_empty() {
            return Vec::new();
        }
        let sql = format!(
            "SELECT rowid, -bm25(doc_fts, {HEADING}, 1.0) FROM doc_fts WHERE doc_fts MATCH ?1 \
             ORDER BY 2 DESC LIMIT {CANDIDATES}"
        );
        let Ok(mut q) = self.conn.prepare(&sql) else {
            return Vec::new();
        };
        let Ok(rows) = q.query_map([match_expr(terms)], |r| {
            Ok(Cand {
                id:    r.get(0)?,
                score: r.get::<_, f64>(1)?,
            })
        }) else {
            return Vec::new();
        };
        normalised(rows.flatten().collect(), weight)
    }

    /// Documents that say the question's words close together, weighed by BM25 over that match.
    ///
    /// Counts reward a document for repeating words and say nothing of whether they were said as
    /// one thought. The rarest few words of the question, required within a short window of one
    /// another, is what separates a document about the subject from one that happens to contain
    /// its vocabulary.
    fn near(&self, terms: &[String]) -> Vec<Cand> {
        let mut by_rarity: Vec<(usize, &String)> = terms.iter().map(|t| (self.matches(t), t)).collect();
        by_rarity.retain(|(n, _)| *n > 0);
        by_rarity.sort();
        let chosen: Vec<String> = by_rarity.into_iter().take(NEAR_TERMS).map(|(_, t)| t.clone()).collect();
        if chosen.len() < 2 {
            return Vec::new();
        }
        let phrase = chosen
            .iter()
            .map(|t| format!("\"{}\"", t.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" ");
        let sql = format!(
            "SELECT rowid, -bm25(doc_fts, {HEADING}, 1.0) FROM doc_fts \
             WHERE doc_fts MATCH 'NEAR({}, {NEAR_WINDOW})' ORDER BY 2 DESC LIMIT {CANDIDATES}",
            phrase.replace('\'', "")
        );
        let Ok(mut q) = self.conn.prepare(&sql) else {
            return Vec::new();
        };
        let Ok(rows) = q.query_map([], |r| {
            Ok(Cand {
                id:    r.get(0)?,
                score: r.get::<_, f64>(1)?,
            })
        }) else {
            return Vec::new();
        };
        normalised(rows.flatten().collect(), NEARNESS)
    }

    /// Documents that hold every word of `text` side by side in the order written.
    fn phrased(&self, text: &str) -> Vec<Cand> {
        let words: Vec<String> = shingle::words(text).collect();
        if words.is_empty() {
            return Vec::new();
        }
        let expr = format!("\"{}\"", words.join(" ").replace('"', ""));
        let sql = format!(
            "SELECT rowid, -bm25(doc_fts, {HEADING}, 1.0) FROM doc_fts WHERE doc_fts MATCH ?1 \
             ORDER BY 2 DESC LIMIT {CANDIDATES}"
        );
        let Ok(mut q) = self.conn.prepare(&sql) else {
            return Vec::new();
        };
        let Ok(rows) = q.query_map([expr], |r| {
            Ok(Cand {
                id:    r.get(0)?,
                score: r.get::<_, f64>(1)?,
            })
        }) else {
            return Vec::new();
        };
        normalised(rows.flatten().collect(), 1.0)
    }

    fn matches(&self, word: &str) -> usize {
        self.conn
            .query_row(
                "SELECT count(*) FROM doc_fts WHERE doc_fts MATCH ?1",
                [match_expr(&[word.to_string()])],
                |r| r.get::<_, i64>(0),
            )
            .map_or(0, |n| usize::try_from(n).unwrap_or(0))
    }

    /// The line of a file with most of the question's words, and its text.
    fn best_line(file: &str, terms: &[String]) -> (usize, String) {
        let Ok(text) = std::fs::read_to_string(file) else {
            return (1, String::new());
        };
        // A prefix, not the word: the index stems, a line has to match the same way.
        let stems: Vec<String> = terms
            .iter()
            .map(|t| t.chars().take(5).collect::<String>())
            .collect();
        let mut best = (0usize, 0usize, String::new());
        for (i, line) in text.lines().enumerate() {
            let low = line.to_lowercase();
            // Distinct question words the line starts a word with, so `air` is not found in
            // `chair` and a line saying three different words beats one repeating a single word.
            let here: Vec<&str> = low
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .collect();
            let mut n = stems
                .iter()
                .filter(|s| here.iter().any(|w| w.starts_with(s.as_str())))
                .count();
            // A heading naming the subject is the better line to open, but only a heading that
            // names some of it.
            if n > 0 && line.trim_start().starts_with('#') {
                n += 1;
            }
            if n > best.0 {
                best = (n, i + 1, line.trim().to_string());
            }
        }
        let mut excerpt = best.2;
        if excerpt.chars().count() > 110 {
            excerpt = excerpt.chars().take(107).collect::<String>() + "...";
        }
        (best.1.max(1), excerpt)
    }
}

impl RepoIndex for Index {
    fn search(&self, query: &IndexQuery<'_>) -> IndexReport {
        let own = terms_of(query.text);
        let extra: Vec<String> = query
            .expand
            .iter()
            .flat_map(|e| terms_of(e))
            .filter(|t| !own.contains(t))
            .collect();

        let mut scores: std::collections::BTreeMap<i64, f64> = std::collections::BTreeMap::new();
        let cands = if query.phrase {
            self.phrased(query.text)
        } else {
            self.ranked(&own, 1.0)
                .into_iter()
                .chain(self.ranked(&extra, EXPANDED))
                .chain(self.near(&own))
                .collect()
        };
        for c in cands {
            *scores.entry(c.id).or_default() += c.score;
        }

        let dead_terms: Vec<String> = own
            .iter()
            .filter(|t| self.matches(t) == 0)
            .cloned()
            .collect();

        struct Row {
            path:   String,
            file:   String,
            class:  DocClass,
            day:    Option<i64>,
            sketch: Vec<u64>,
            score:  f64,
        }
        let mut rows: Vec<Row> = Vec::new();
        for (id, base) in &scores {
            let got = self.conn.query_row(
                "SELECT path, file, class, day, sketch FROM doc WHERE id = ?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                        r.get::<_, Vec<u8>>(4)?,
                    ))
                },
            );
            let Ok((path, file, class, day, sketch)) = got else {
                continue;
            };
            let class = DocClass::from_label(&class).unwrap_or(DocClass::Research);
            rows.push(Row {
                path,
                file,
                class,
                day,
                sketch: shingle::from_bytes(&sketch),
                score: base * class.weight(),
            });
        }
        rows.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.path.cmp(&b.path))
        });
        let matched = rows.len();

        // Fold in rank order: a document that is a copy of one already kept stands behind it.
        let mut kept: Vec<(Row, Vec<String>)> = Vec::new();
        for row in rows {
            if let Some((_, copies)) = kept
                .iter_mut()
                .find(|(k, _)| shingle::containment(&k.sketch, &row.sketch) >= shingle::COPY_AT)
            {
                copies.push(row.path);
                continue;
            }
            if kept.len() < query.limit {
                kept.push((row, Vec::new()));
            }
        }

        let all_terms: Vec<String> = own.iter().chain(extra.iter()).cloned().collect();
        let hits = kept
            .into_iter()
            .map(|(row, copies)| {
                let (line, excerpt) = Self::best_line(&row.file, &all_terms);
                IndexHit {
                    path: row.path,
                    line,
                    class: row.class,
                    day: row.day,
                    age_days: row.day.map(|d| query.today - d),
                    score: row.score,
                    excerpt,
                    copies,
                }
            })
            .collect();

        IndexReport {
            hits,
            matched,
            indexed: self.len(),
            dead_terms,
            caveats: self.caveats.clone(),
        }
    }

    fn doc_frequency(&self, word: &str) -> usize {
        self.matches(word)
    }

    fn changed_day(&self, repo_path: &str) -> Option<i64> {
        let root = self.git_root.as_deref()?;
        super::gitdays::last_day(root, repo_path)
    }
}
