//! Bringing the index up to date with the tree, reading only what changed.
//!
//! A file is keyed by its path. If its size and modification time are the ones stored, it is
//! unchanged and is not opened. If they differ it is read and hashed, and if the hash is the one
//! stored only the stat is refreshed. Anything else is replaced, and a path no longer on disk is
//! dropped, so the index never answers with a document that is gone.

use std::collections::{BTreeMap, BTreeSet};
use std::time::UNIX_EPOCH;

use rusqlite::params;
use sha2::{Digest, Sha256};

use super::walk::{Found, Source, walk};
use super::{Index, IndexError, shingle, stamp};

/// A file larger than this is a data blob and not prose, and searching it yields one enormous hit.
const MAX_BYTES: u64 = 4_000_000;

/// What one refresh did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Refreshed {
    /// Files read and stored, new or changed.
    pub read:      usize,
    /// Files whose stat matched and which were not opened.
    pub unchanged: usize,
    /// Files whose stat moved and whose content did not.
    pub touched:   usize,
    /// Paths dropped because they are gone.
    pub removed:   usize,
    /// Captures held out as about another project.
    pub held_out:  usize,
}

struct Known {
    id:    i64,
    size:  i64,
    mtime: i64,
    hash:  String,
}

fn stat_of(file: &std::path::Path) -> Option<(i64, i64)> {
    let m = std::fs::metadata(file).ok()?;
    let size = i64::try_from(m.len()).ok()?;
    let nanos = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos();
    Some((size, i64::try_from(nanos).unwrap_or(i64::MAX)))
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn heads_of(text: &str) -> String {
    text.lines()
        .filter(|l| l.trim_start().starts_with('#'))
        .map(|l| l.trim_start().trim_start_matches('#').trim())
        .collect::<Vec<_>>()
        .join("\n")
}

impl Index {
    /// Bring the index to the tree under `sources`.
    ///
    /// # Errors
    /// When the store cannot be written. A file that cannot be read is counted in the caveats and
    /// is not an error.
    pub fn refresh(&mut self, sources: &[Source]) -> Result<Refreshed, IndexError> {
        let walked = walk(sources);
        let mut known: BTreeMap<String, Known> = BTreeMap::new();
        {
            let mut q = self
                .conn
                .prepare("SELECT path, id, size, mtime, hash FROM doc")?;
            let rows = q.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    Known {
                        id:    r.get(1)?,
                        size:  r.get(2)?,
                        mtime: r.get(3)?,
                        hash:  r.get(4)?,
                    },
                ))
            })?;
            for r in rows.flatten() {
                known.insert(r.0, r.1);
            }
        }

        let mut done = Refreshed::default();
        let mut unreadable = walked.unreadable;
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let tx = self.conn.transaction()?;
        for f in &walked.files {
            seen.insert(f.shown.clone());
            let Some((size, mtime)) = stat_of(&f.file) else {
                unreadable += 1;
                continue;
            };
            let prior = known.get(&f.shown);
            if prior.is_some_and(|k| k.size == size && k.mtime == mtime) {
                done.unchanged += 1;
                continue;
            }
            if u64::try_from(size).unwrap_or(u64::MAX) > MAX_BYTES {
                unreadable += 1;
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&f.file) else {
                unreadable += 1;
                continue;
            };
            let hash = hex(raw.as_bytes());
            if let Some(k) = prior.filter(|k| k.hash == hash) {
                tx.execute(
                    "UPDATE doc SET size = ?1, mtime = ?2 WHERE id = ?3",
                    params![size, mtime, k.id],
                )?;
                done.touched += 1;
                continue;
            }
            if let Some(k) = prior {
                tx.execute("DELETE FROM doc_fts WHERE rowid = ?1", [k.id])?;
                tx.execute("DELETE FROM doc WHERE id = ?1", [k.id])?;
            }
            let held = held_out(f, &raw);
            let words = shingle::words(&raw).count();
            let day = stamp::day_from_path(&f.shown);
            let sketch = if held { Vec::new() } else { shingle::sketch(&raw) };
            tx.execute(
                "INSERT INTO doc (path, file, class, size, mtime, hash, day, words, sketch, held)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    f.shown,
                    f.file.to_string_lossy(),
                    f.class.label(),
                    size,
                    mtime,
                    hash,
                    day,
                    i64::try_from(words).unwrap_or(i64::MAX),
                    shingle::to_bytes(&sketch),
                    i64::from(held),
                ],
            )?;
            if held {
                done.held_out += 1;
            } else {
                let id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO doc_fts (rowid, heads, body) VALUES (?1, ?2, ?3)",
                    params![id, heads_of(&raw), raw],
                )?;
            }
            done.read += 1;
        }
        for (path, k) in &known {
            if !seen.contains(path) {
                tx.execute("DELETE FROM doc_fts WHERE rowid = ?1", [k.id])?;
                tx.execute("DELETE FROM doc WHERE id = ?1", [k.id])?;
                done.removed += 1;
            }
        }
        tx.commit()?;

        self.caveats = caveats(sources, &walked.missing, &walked.skipped, unreadable, &self.held());
        Ok(done)
    }

    fn held(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        if let Ok(mut q) = self
            .conn
            .prepare("SELECT class, count(*) FROM doc WHERE held = 1 GROUP BY class")
        {
            if let Ok(rows) = q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            {
                for (c, n) in rows.flatten() {
                    out.insert(c, usize::try_from(n).unwrap_or(0));
                }
            }
        }
        out
    }
}

/// Whether a file is about another project than the one its source is scoped to.
fn held_out(f: &Found, raw: &str) -> bool {
    f.about
        .as_deref()
        .is_some_and(|a| !raw.to_lowercase().contains(&a.to_lowercase()))
}

fn caveats(
    sources: &[Source],
    missing: &[String],
    skipped: &BTreeMap<String, usize>,
    unreadable: usize,
    held: &BTreeMap<String, usize>,
) -> Vec<String> {
    let mut out = Vec::new();
    if !missing.is_empty() {
        out.push(format!(
            "roots absent from disk and not searched: {}",
            missing.join(", ")
        ));
    }
    let held_total: usize = held.values().sum();
    if held_total > 0 {
        let about = sources
            .iter()
            .find_map(|s| s.about.as_deref())
            .unwrap_or_default();
        out.push(format!(
            "{held_total} captures name no `{about}` and were held out as about another project"
        ));
    }
    if unreadable > 0 {
        out.push(format!("{unreadable} files could not be read"));
    }
    if !skipped.is_empty() {
        let total: usize = skipped.values().sum();
        out.push(format!(
            "{total} files across {} extensions were not text and not read",
            skipped.len()
        ));
    }
    out
}
