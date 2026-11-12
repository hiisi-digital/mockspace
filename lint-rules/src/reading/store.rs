//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What is kept between runs: per unit, the trace and the findings, asked again on the next run.
//!
//! A unit's entry sits under a key made of the lint's identity and configuration and the unit it
//! judged, and holds the last few traces the unit was judged under, each with its findings and how
//! long the judging took. On the next run each trace is asked again, question by question, against
//! the current reading; the first whose every answer has the digest it had is replayed. Keeping a
//! few rather than one is what lets a branch switch back and forth and replay both sides.
//!
//! Digests are the store's own ([`VerdictCache::digest`]), each taken at most once per run however
//! many traces hold the question, and a file's is found again through the [`Table`] where its size
//! and time did not move.

use std::time::Duration;

use super::table::Table;
use super::{Question, Reading, Trace, Values};
use crate::LintError;
use crate::reading::disk::Kind;
use crate::reading::memo::Memo;
use crate::verdict::{self, VerdictCache};

/// How many traces one unit's entry keeps.
const KEPT: usize = 4;

const FORMAT: &str = "unit-v1";

/// One trace a unit was judged under, with what it found.
#[derive(Clone)]
pub(crate) struct Alt {
    took:     Duration,
    answers:  Vec<(Question, String)>,
    findings: Vec<LintError>,
}

/// What a lookup found: the findings to replay when a trace still holds, and how long the unit took
/// the last time it was judged, which orders the units that do run.
pub(crate) struct Lookup {
    pub(crate) hit:   Option<(Vec<LintError>, Trace)>,
    pub(crate) last:  Option<Duration>,
    pub(crate) prior: Vec<Alt>,
}

/// Where a view saves the items it splits into: the store and the key of its own unit.
#[derive(Clone)]
pub(crate) struct UnitKey<'r> {
    pub(crate) store: &'r Store<'r>,
    pub(crate) key:   String,
    /// The lint the unit belongs to, which its items are counted under.
    pub(crate) lint:  &'r str,
}

/// The saved units of a run, over one reading and one cache.
pub struct Store<'r> {
    reading: &'r Reading,
    cache:   &'r dyn VerdictCache,
    values:  &'r dyn Values,
    digests: Memo<Question, Option<String>>,
    table:   Table,
}

impl<'r> Store<'r> {
    pub(crate) fn new(reading: &'r Reading, cache: &'r dyn VerdictCache, values: &'r dyn Values) -> Self {
        let table = Table::load(cache, &reading.root().to_string_lossy());
        Self {
            reading,
            cache,
            values,
            digests: Memo::new(),
            table,
        }
    }

    pub(crate) fn cache(&self) -> &'r dyn VerdictCache {
        self.cache
    }

    /// Write back what the run learned of file digests. Called once, after every unit.
    pub fn finish(&self) {
        if !self.reading.is_overlaid() {
            self.table.save(self.cache);
        }
    }

    /// The key a unit's entry sits under.
    pub(crate) fn unit_key(&self, parts: &[&[u8]]) -> String {
        let mut all: Vec<&[u8]> = Vec::with_capacity(parts.len() + 1);
        all.push(FORMAT.as_bytes());
        all.extend_from_slice(parts);
        self.cache.digest(&all)
    }

    fn digest(&self, parts: &[&[u8]]) -> String {
        self.cache.digest(parts)
    }

    /// The digest the current reading gives `q`, or `None` where it cannot be answered.
    pub(crate) fn digest_of(&self, q: &Question) -> Option<String> {
        self.digests.get_or(q.clone(), || self.answer(q)).clone()
    }

    fn answer(&self, q: &Question) -> Option<String> {
        let r = self.reading;
        Some(match q {
            Question::File(key) => self.file_digest(key),
            Question::List(key) => match r.listing(&r.path_of(key)) {
                Some(l) => self.digest(&[b"list", &l.material()]),
                None => self.digest(&[b"list", b"\x01absent"]),
            },
            Question::Kind(key) => {
                let letter: &[u8] = r.kind(&r.path_of(key)).map_or(b"\x01absent", Kind::letter);
                self.digest(&[b"kind", letter])
            },
            Question::Git {
                cwd,
                args,
            } => self.digest(&[b"git", &r.git_answer(cwd, args).material()]),
            Question::Run {
                program,
                args,
                cwd,
                stdin,
                inputs,
            } => {
                let mut held: Vec<String> = vec![program.clone(), cwd.clone(), stdin.clone().unwrap_or_default()];
                held.push(args.len().to_string());
                held.extend(args.iter().cloned());
                for input in inputs {
                    held.push(input.clone());
                    held.push(self.file_digest(input));
                }
                let parts: Vec<&[u8]> = std::iter::once(&b"run"[..]).chain(held.iter().map(|s| s.as_bytes())).collect();
                self.digest(&parts)
            },
            Question::Value(key) => {
                let material = self.values.material(key)?;
                self.digest(&[b"value", key.as_bytes(), &material])
            },
        })
    }

    /// A file's digest: of the bytes this run read where it read them, which is what a lint was
    /// shown; from the table where the file's size and time did not move; and otherwise of the
    /// bytes read now.
    fn file_digest(&self, key: &str) -> String {
        let r = self.reading;
        let abs = r.path_of(key);
        if let Some(read) = r.held(&abs) {
            let d = self.digest(&[b"file", &read.bytes]);
            if let Some(stat) = &read.stat {
                self.table.put(key, stat, read.taken, &d);
            }
            return d;
        }
        match r.stat(&abs) {
            None => return self.digest(&[b"file", b"\x01absent"]),
            Some(s) if s.kind != Kind::File => return self.digest(&[b"file", b"\x01kind", s.kind.letter()]),
            Some(s) => {
                if let Some(d) = self.table.trusted(key, s) {
                    return d;
                }
            },
        }
        match r.read(&abs) {
            Some(read) => {
                let d = self.digest(&[b"file", &read.bytes]);
                if let Some(stat) = &read.stat {
                    self.table.put(key, stat, read.taken, &d);
                }
                d
            },
            None => self.digest(&[b"file", b"\x01unreadable"]),
        }
    }

    /// The unit saved under `key`, replayed when one of its traces still holds.
    pub(crate) fn lookup(&self, key: &str) -> Lookup {
        let prior = self.cache.replay(key).as_deref().map(decode).unwrap_or_default();
        let last = prior.first().map(|a| a.took);
        let hit = prior.iter().find(|alt| self.holds(alt)).map(|alt| {
            let trace = Trace {
                questions: alt.answers.iter().map(|(q, _)| q.clone()).collect(),
                untraced:  None,
            };
            (alt.findings.clone(), trace)
        });
        Lookup {
            hit,
            last,
            prior,
        }
    }

    fn holds(&self, alt: &Alt) -> bool {
        alt.answers.iter().all(|(q, d)| self.digest_of(q).as_deref() == Some(d.as_str()))
    }

    /// Save what a unit found under the trace it was judged by, ahead of the traces kept before.
    /// Nothing is saved over an overlaid reading, for an untraced unit, or for a trace holding a
    /// question the run cannot answer.
    pub(crate) fn save(&self, key: &str, prior: Vec<Alt>, trace: &Trace, findings: &[LintError], took: Duration) {
        if self.reading.is_overlaid() || trace.untraced.is_some() {
            return;
        }
        let mut answers = Vec::with_capacity(trace.questions.len());
        for q in &trace.questions {
            let Some(d) = self.digest_of(q) else { return };
            answers.push((q.clone(), d));
        }
        let fresh = Alt {
            took,
            answers,
            findings: findings.to_vec(),
        };
        let older: Vec<Alt> = prior.into_iter().filter(|a| a.answers != fresh.answers).collect();
        let mut alts = vec![fresh];
        alts.extend(older);
        alts.truncate(KEPT);
        self.cache.remember(key, &encode(&alts));
    }
}

fn encode(alts: &[Alt]) -> String {
    let mut out = format!("{FORMAT} {}\n", alts.len());
    for a in alts {
        out.push_str(&format!("alt {} {}\n", a.took.as_micros(), a.answers.len()));
        for (q, d) in &a.answers {
            out.push_str(d);
            out.push('\t');
            out.push_str(&q.encode());
            out.push('\n');
        }
        out.push_str(&verdict::encode(&a.findings));
    }
    out
}

/// The traces [`encode`] wrote, or none at all for text in no shape it writes.
fn decode(text: &str) -> Vec<Alt> {
    decode_strict(text).unwrap_or_default()
}

fn decode_strict(text: &str) -> Option<Vec<Alt>> {
    let mut lines = text.lines();
    let (format, n) = lines.next()?.split_once(' ')?;
    if format != FORMAT {
        return None;
    }
    let n: usize = n.parse().ok()?;
    let mut alts = Vec::with_capacity(n);
    for _ in 0 .. n {
        let head = lines.next()?;
        let mut h = head.split(' ');
        let (Some("alt"), Some(micros), Some(count)) = (h.next(), h.next(), h.next()) else { return None };
        let took = Duration::from_micros(micros.parse().ok()?);
        let count: usize = count.parse().ok()?;
        let mut answers = Vec::with_capacity(count);
        for _ in 0 .. count {
            let (d, q) = lines.next()?.split_once('\t')?;
            answers.push((Question::decode(q)?, d.to_string()));
        }
        let verdict_head = lines.next()?;
        let (_, c) = verdict_head.split_once(' ')?;
        let c: usize = c.parse().ok()?;
        let mut block = String::from(verdict_head);
        block.push('\n');
        for _ in 0 .. c {
            block.push_str(lines.next()?);
            block.push('\n');
        }
        let findings = verdict::decode(&block)?;
        alts.push(Alt {
            took,
            answers,
            findings,
        });
    }
    Some(alts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_reads_back_as_written_and_a_cut_one_reads_as_nothing() {
        let alts = vec![
            Alt {
                took:     Duration::from_micros(1234),
                answers:  vec![(Question::File("a.rs".into()), "d1".into())],
                findings: vec![LintError::error("c".into(), 3, "lint-a", "msg\twith tab".into())],
            },
            Alt {
                took:     Duration::from_micros(5),
                answers:  vec![],
                findings: vec![],
            },
        ];
        let text = encode(&alts);
        let back = decode(&text);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].took, Duration::from_micros(1234));
        assert_eq!(back[0].answers, alts[0].answers);
        assert_eq!(back[0].findings[0].message, "msg\twith tab");
        let cut = &text[.. text.len() - 10];
        assert!(decode(cut).is_empty(), "a cut entry is a miss, never a partial replay");
    }

    /// The entry `tests/an_incremental_run_equals_a_full_one_over_a_history.rs` writes over every
    /// saved unit to show its comparison can fail: one trace asking nothing, which always holds,
    /// and one wrong finding. Pinned here so a change of format cannot make that plant a miss,
    /// which would let the walk's control pass for the wrong reason.
    #[test]
    fn the_entry_the_history_walk_plants_reads_as_one_wrong_finding() {
        let planted = "unit-v1 1\nalt 0 0\nverdict-v2 1\nplanted\t1\tplanted\tEEE\t~\t~\tplanted by the test\n";
        let alts = decode(planted);
        assert_eq!(alts.len(), 1);
        assert!(alts[0].answers.is_empty());
        assert_eq!(alts[0].findings.len(), 1);
        assert_eq!(alts[0].findings[0].message, "planted by the test");
    }
}
