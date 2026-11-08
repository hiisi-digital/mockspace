//! The index as the engine builds it for a run: where its roots are, where its file is, and what
//! a tool is told when it cannot be had.

use std::path::{Path, PathBuf};

use mockspace_lint_rules::{DocClass, IndexQuery, IndexReport, RepoIndex};

use super::{Index, Source, cache_root, index_path};
use crate::config::Config;

/// The environment variable naming the directory of a human's captures, which wins over the
/// repository's `index_captures`.
pub const CAPTURES_VAR: &str = "MOCKSPACE_CAPTURES";

/// Today, days since 1970-01-01, from the system clock.
#[must_use]
pub fn today() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    i64::try_from(secs / 86_400).unwrap_or(0)
}

/// The directory of captures, if one can be found: the variable, else the first `index_captures`
/// path that exists below the repository root or any directory above it.
///
/// A variable naming nothing finds nothing, and does not fall back: a mistyped name is a missing
/// root a report names, not a different directory read in silence.
#[must_use]
pub fn captures_dir(repo_root: &Path, declared: &[String], named: Option<&str>) -> Option<PathBuf> {
    if let Some(n) = named.filter(|n| !n.trim().is_empty()) {
        let p = PathBuf::from(n);
        return p.is_dir().then_some(p);
    }
    for dir in repo_root.ancestors() {
        for rel in declared {
            let p = dir.join(rel);
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    None
}

/// The roots of a repository's index.
#[must_use]
pub fn sources_for(cfg: &Config) -> Vec<Source> {
    let named = std::env::var(CAPTURES_VAR).ok();
    let mut out = vec![
        Source {
            dir:   cfg.mock_dir.join("research"),
            shown: "research".to_string(),
            class: DocClass::Research,
            about: None,
        },
        Source {
            dir:   cfg.mock_dir.join("design_rounds"),
            shown: "design_rounds".to_string(),
            class: DocClass::Design,
            about: None,
        },
    ];
    if let Some(dir) = captures_dir(&cfg.repo_root, &cfg.index_captures, named.as_deref()) {
        out.push(Source {
            dir,
            shown: "op-responses".to_string(),
            class: DocClass::Human,
            about: Some(cfg.project_name.clone()),
        });
    }
    out
}

/// The file each row was declared in, relative to the repository root, so a tool can ask when the
/// canon that holds a row last changed. A source outside the root is shown as it is.
#[must_use]
pub fn row_sources(
    reg: &crate::registry::Registry,
    repo_root: &Path,
) -> std::collections::BTreeMap<String, String> {
    reg.rows
        .iter()
        .map(|(q, row)| {
            let shown = row
                .source
                .strip_prefix(repo_root)
                .unwrap_or(&row.source)
                .to_string_lossy()
                .replace('\\', "/");
            (q.clone(), shown)
        })
        .collect()
}

/// What a tool is handed when the index could not be built: it says why, to everything.
struct Unavailable(String);

impl RepoIndex for Unavailable {
    fn search(&self, _query: &IndexQuery<'_>) -> IndexReport {
        IndexReport {
            caveats: vec![format!("the index could not be built: {}", self.0)],
            ..IndexReport::default()
        }
    }

    fn doc_frequency(&self, _word: &str) -> usize {
        0
    }
}

/// The repository's index, built on the first question and not before, so a tool that never asks
/// pays nothing, and one that asks pays once.
pub struct ForRun {
    sources: Vec<Source>,
    file:    Option<PathBuf>,
    root:    PathBuf,
    built:   std::cell::OnceCell<Box<dyn RepoIndex>>,
}

/// The index for a run. Never fails: a cache that cannot be written is replaced by one in memory,
/// and a store that cannot start is an index that says so to every question.
#[must_use]
pub fn for_run(cfg: &Config) -> ForRun {
    ForRun {
        sources: sources_for(cfg),
        file:    cache_root().map(|c| index_path(&c, &cfg.repo_root)),
        root:    cfg.repo_root.clone(),
        built:  std::cell::OnceCell::new(),
    }
}

impl ForRun {
    fn get(&self) -> &dyn RepoIndex {
        self.built
            .get_or_init(|| {
                let opened = match &self.file {
                    Some(f) => Index::open(f),
                    None => Index::in_memory(),
                };
                let mut idx = match opened.or_else(|_| Index::in_memory()) {
                    Ok(i) => i,
                    Err(e) => return Box::new(Unavailable(e.to_string())),
                };
                match idx.refresh_dated(&self.sources, Some(&self.root)) {
                    Ok(_) => Box::new(idx),
                    Err(e) => Box::new(Unavailable(e.to_string())),
                }
            })
            .as_ref()
    }
}

impl RepoIndex for ForRun {
    fn search(&self, query: &IndexQuery<'_>) -> IndexReport {
        self.get().search(query)
    }

    fn doc_frequency(&self, word: &str) -> usize {
        self.get().doc_frequency(word)
    }

    fn changed_day(&self, repo_path: &str) -> Option<i64> {
        self.get().changed_day(repo_path)
    }
}

#[cfg(test)]
mod tests;
