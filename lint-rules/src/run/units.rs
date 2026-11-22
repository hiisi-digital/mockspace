//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The units of a run, looked up, judged where no saved trace answers them, and put back in order.

use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::RunInput;
use super::gate::{configure_all, enabled, restamp};
use crate::context::CrateInput;
use crate::reading::store::Lookup;
use crate::reading::{Store, UnitKey, View, pool};
use crate::verdict::{Origin, behaviour_material};
use crate::{
    CrateLint,
    Lint,
    LintContext,
    LintError,
    LintPack,
    PathFilter,
    RepoContext,
    RepoLint,
    WorkspaceLint,
    honour_allows,
    timing,
};

/// The root a crate's findings with no path are about.
const ROOT: &str = "src/lib.rs";

/// What one unit judges.
enum What<'a> {
    /// One file of a crate, for a lint judging one file at a time.
    File {
        lint:  &'a dyn CrateLint,
        krate: &'a CrateInput,
        file:  PathBuf,
    },
    /// A crate whole, shown only the files `filter` admits where it is set.
    Crate {
        lint:   &'a dyn CrateLint,
        krate:  &'a CrateInput,
        filter: Option<&'a PathFilter>,
    },
    /// The crates a workspace lint is bound to, together.
    Workspace {
        lint:  &'a dyn WorkspaceLint,
        bound: Vec<&'a CrateInput>,
    },
    /// The repository, for a repo lint.
    Repo { lint: &'a dyn RepoLint },
}

struct Unit<'a> {
    what: What<'a>,
    lint: &'a dyn Lint,
    key:  String,
}

/// The pack's lints of each kind, empty where there is no pack.
struct Packed<'a> {
    crates:     &'a [Box<dyn CrateLint>],
    workspaces: &'a [Box<dyn WorkspaceLint>],
    repos:      &'a [Box<dyn RepoLint>],
}

/// The built-in lints of a run, configured, beside the pack's.
pub(super) struct Runner<'a> {
    input:      &'a RunInput<'a>,
    pack:       Packed<'a>,
    crates:     Vec<Box<dyn CrateLint>>,
    workspaces: Vec<Box<dyn WorkspaceLint>>,
    repos:      Vec<Box<dyn RepoLint>>,
}

impl<'a> Runner<'a> {
    pub(super) fn new(input: &'a RunInput<'a>, pack: Option<&'a LintPack>) -> Self {
        let mut crates = crate::all_lints();
        let mut workspaces = crate::all_workspace_lints();
        let mut repos = crate::all_repo_lints();
        configure_all(crates.iter_mut().map(|l| l.as_mut() as &mut dyn Lint), input.overrides);
        configure_all(workspaces.iter_mut().map(|l| l.as_mut() as &mut dyn Lint), input.overrides);
        configure_all(repos.iter_mut().map(|l| l.as_mut() as &mut dyn Lint), input.overrides);
        Self {
            input,
            pack: Packed {
                crates:     pack.map_or(&[][..], |p| &p.crate_lints),
                workspaces: pack.map_or(&[][..], |p| &p.workspace_lints),
                repos:      pack.map_or(&[][..], |p| &p.repo_lints),
            },
            crates,
            workspaces,
            repos,
        }
    }

    fn wanted(&self, lint: &dyn Lint) -> bool {
        self.input.only.is_none_or(|n| n == lint.name()) && enabled(lint, self.input.doc_only, self.input.overrides)
    }

    fn binds(&self, lint: &dyn Lint, krate: &str) -> bool {
        self.input.overrides.is_none_or(|cfg| cfg.binds_crate(lint.name(), krate))
    }

    /// Every unit of the run, in the order a serial run judges them.
    fn units<'s>(&'s self, store: Option<&Store<'_>>) -> Vec<Unit<'s>> {
        let input = self.input;
        let reading = input.reading;
        let key = |lint: &dyn Lint, origin: Origin, parts: &[&[u8]]| -> String {
            let Some(store) = store else { return String::new() };
            let behaviour = behaviour_material(lint.name(), origin, input.overrides);
            let mut all: Vec<&[u8]> = vec![behaviour.as_bytes()];
            all.extend_from_slice(parts);
            store.unit_key(&all)
        };
        let pack = &self.pack;
        let crate_lints: Vec<(Origin, &dyn CrateLint)> = self
            .crates
            .iter()
            .map(|l| (Origin::BuiltIn, l.as_ref()))
            .chain(pack.crates.iter().map(|l| (Origin::Pack, l.as_ref())))
            .filter(|(_, l)| self.wanted(*l))
            .collect();
        let mut out = Vec::new();
        for krate in input.crates {
            if input.scope.is_some_and(|names| !names.iter().any(|n| *n == krate.name)) {
                continue;
            }
            let scalars = LintContext::scalars(krate, input.workspace);
            let files = &reading.crate_files(&krate.dir).files;
            for &(origin, lint) in &crate_lints {
                if !self.binds(lint, &krate.name) {
                    continue;
                }
                let filter = input.overrides.and_then(|cfg| cfg.filter_for(lint.name()));
                let kept: Vec<&PathBuf> = files.iter().filter(|f| filter.is_none_or(|p| p.allows(f))).collect();
                // A filter admitting no file leaves the lint nothing to look at, which is not a pass.
                if filter.is_some() && kept.is_empty() {
                    continue;
                }
                if !lint.per_file() {
                    out.push(Unit {
                        what: What::Crate {
                            lint,
                            krate,
                            filter,
                        },
                        lint,
                        key: key(lint, origin, &[b"crate", scalars.as_bytes()]),
                    });
                    continue;
                }
                let root = [PathBuf::from(ROOT)];
                let kept: Vec<&PathBuf> = if kept.is_empty() { root.iter().collect() } else { kept };
                for file in kept {
                    let rel = crate::reading::names::text_of(file.as_os_str());
                    out.push(Unit {
                        what: What::File {
                            lint,
                            krate,
                            file: file.clone(),
                        },
                        lint,
                        key: key(lint, origin, &[b"file", scalars.as_bytes(), rel.as_bytes()]),
                    });
                }
            }
        }

        let workspace_lints = self
            .workspaces
            .iter()
            .map(|l| (Origin::BuiltIn, l.as_ref()))
            .chain(pack.workspaces.iter().map(|l| (Origin::Pack, l.as_ref())));
        for (origin, lint) in workspace_lints {
            if !self.wanted(lint) {
                continue;
            }
            // Bound to nothing, a cross-crate lint does not run: a check over an empty set is a
            // pass it never established.
            let bound: Vec<&CrateInput> = input.crates.iter().filter(|k| self.binds(lint, &k.name)).collect();
            if bound.is_empty() && !input.crates.is_empty() {
                continue;
            }
            let scalars: Vec<String> = bound.iter().map(|k| LintContext::scalars(k, input.workspace)).collect();
            let mut parts: Vec<&[u8]> = vec![b"workspace"];
            parts.extend(scalars.iter().map(|s| s.as_bytes()));
            let key = key(lint, origin, &parts);
            out.push(Unit {
                what: What::Workspace {
                    lint,
                    bound,
                },
                lint,
                key,
            });
        }

        if let Some(repo) = input.repo {
            let repo_lints = self
                .repos
                .iter()
                .map(|l| (Origin::BuiltIn, l.as_ref()))
                .chain(pack.repos.iter().map(|l| (Origin::Pack, l.as_ref())));
            let root = crate::reading::names::text_of(repo.repo_root.as_os_str());
            let mock = crate::reading::names::text_of(repo.mock_dir.as_os_str());
            for (origin, lint) in repo_lints {
                if !self.wanted(lint) {
                    continue;
                }
                out.push(Unit {
                    what: What::Repo {
                        lint,
                    },
                    lint,
                    key: key(lint, origin, &[b"repo", root.as_bytes(), mock.as_bytes()]),
                });
            }
        }
        out
    }

    /// The unit's lint run over `view`, its findings as the lint gave them.
    fn judge(&self, what: &What<'_>, view: &View<'_>) -> Vec<LintError> {
        let ws = self.input.workspace;
        match what {
            What::File {
                lint,
                krate,
                file,
            } => lint.check(&LintContext::new(view, krate, ws, file)),
            What::Crate {
                lint,
                krate,
                filter: None,
            } => lint.check(&LintContext::new(view, krate, ws, Path::new(ROOT))),
            What::Crate {
                lint,
                krate,
                filter: Some(filter),
            } => {
                // Which file `source()` reads follows from what is in the crate and which of its
                // files can be read as text, so the listing and every file the filter admits are in
                // the trace: a file sorting first, or one turning readable, reruns the lint.
                for rel in &view.crate_files(&krate.dir).files {
                    if filter.allows(rel) {
                        let _ = view.text(krate.dir.join(rel));
                    }
                }
                let allowed: Vec<_> = view
                    .reading()
                    .crate_sources(&krate.dir)
                    .iter()
                    .filter(|s| filter.allows(s.rel_path))
                    .copied()
                    .collect();
                let Some(first) = allowed.first() else { return Vec::new() };
                lint.check(&LintContext::new(view, krate, ws, first.rel_path).narrowed_to(&allowed))
            },
            What::Workspace {
                lint,
                bound,
            } => {
                let ctxs: Vec<LintContext<'_>> =
                    bound.iter().map(|k| LintContext::new(view, k, ws, Path::new(ROOT))).collect();
                let pairs: Vec<(&str, &LintContext<'_>)> = ctxs.iter().map(|c| (c.crate_name, c)).collect();
                lint.check_all(&pairs)
            },
            What::Repo {
                lint,
            } => {
                let Some(mut repo) = self.input.repo else { return Vec::new() };
                let wanted = lint.invocation_wanted();
                if !wanted {
                    repo.invocation = None;
                }
                lint.check_repo(&RepoContext::new(view, repo, wanted))
            },
        }
    }

    /// What a unit's findings come to once the runner has stamped their file, honoured the
    /// `lint:allow` markers and applied the configured severities. The same for a replayed unit as
    /// for a judged one.
    fn finish(&self, unit: &Unit<'_>, mut found: Vec<LintError>) -> Vec<LintError> {
        let reading = self.input.reading;
        let text = |dir: &Path, rel: &str| reading.text(&dir.join(rel));
        match &unit.what {
            What::File {
                krate,
                file,
                ..
            } => {
                // The crate root stays bare, because that is where a reader already looks. Any
                // other file is carried as a field the renderer turns into a location.
                let rel = file.to_string_lossy();
                if rel != ROOT {
                    for err in &mut found {
                        err.path = Some(rel.to_string());
                    }
                }
                if unit.lint.markers_decide() {
                    found = honour_allows(found, unit.lint.name(), |err| {
                        text(&krate.dir, err.path.as_deref().unwrap_or(&rel))
                    });
                }
            },
            What::Crate {
                krate,
                ..
            } => {
                // A crate-scoped finding with no path is about the crate and not about its root:
                // one marker on the root's banner must not silence the lint for every file.
                if unit.lint.markers_decide() {
                    found = honour_allows(found, unit.lint.name(), |err| {
                        err.path.as_deref().and_then(|p| text(&krate.dir, p))
                    });
                }
            },
            What::Workspace {
                bound,
                ..
            } => {
                if unit.lint.markers_decide() {
                    let per_file = unit.lint.per_file();
                    found = honour_allows(found, unit.lint.name(), |err| {
                        let krate = bound.iter().find(|k| k.name == err.crate_name)?;
                        match err.path.as_deref() {
                            None if per_file => text(&krate.dir, ROOT),
                            None => None,
                            Some(p) => text(&krate.dir, p),
                        }
                    });
                }
            },
            What::Repo {
                ..
            } => {},
        }
        restamp(unit.lint, self.input.overrides, &mut found);
        found
    }

    pub(super) fn run(&self, store: Option<&Store<'_>>) -> Vec<LintError> {
        let units = self.units(store);
        let threads = self.input.reading.threads();
        let lookups: Vec<Lookup> = match store {
            Some(s) => {
                timing::time("phase:verify", || {
                    pool::each_index(self.input.reading.pool(), units.len(), |i| {
                        let found = s.lookup(&units[i].key);
                        s.cache().tally(found.hit.is_some(), units[i].lint.name());
                        found
                    })
                })
            },
            None => {
                units
                    .iter()
                    .map(|_| Lookup {
                        hit:   None,
                        last:  None,
                        prior: Vec::new(),
                    })
                    .collect()
            },
        };

        let mut slots: Vec<Option<Vec<LintError>>> = Vec::with_capacity(units.len());
        let mut misses: Vec<(usize, Option<Duration>, Vec<crate::reading::store::Alt>)> = Vec::new();
        for (i, l) in lookups.into_iter().enumerate() {
            match l.hit {
                Some((findings, _)) => slots.push(Some(findings)),
                None => {
                    slots.push(None);
                    misses.push((i, l.last, l.prior));
                },
            }
        }
        let replayed = units.len() - misses.len();
        if timing::enabled() {
            for _ in 0 .. replayed {
                timing::count(|| "units:replayed".to_string());
            }
            for _ in 0 .. misses.len() {
                timing::count(|| "units:run".to_string());
            }
        }

        // Longest first, a unit never judged before ahead of every one that was, so the slowest
        // does not start last. On one thread the units run in the serial order instead.
        let mut order: Vec<usize> = (0 .. misses.len()).collect();
        if threads > 1 {
            order.sort_by_key(|&j| Reverse(misses[j].1.unwrap_or(Duration::MAX)));
        }
        let reading = self.input.reading;
        let judged = pool::in_order(self.input.reading.pool(), misses.len(), &order, |j| {
            let (i, _, prior) = &misses[j];
            let unit = &units[*i];
            let at = store.map(|s| {
                UnitKey {
                    store: s,
                    key:   unit.key.clone(),
                    lint:  unit.lint.name(),
                }
            });
            let view = View::new(reading, at);
            let start = Instant::now();
            let findings = self.judge(&unit.what, &view);
            let took = start.elapsed();
            if timing::enabled() {
                timing::record(&format!("lint:{}", unit.lint.name()), took);
            }
            if let Some(s) = store {
                s.save(&unit.key, prior.clone(), &view.into_trace(), &findings, took);
            }
            findings
        });
        for ((i, ..), findings) in misses.iter().zip(judged) {
            slots[*i] = Some(findings);
        }

        units
            .iter()
            .zip(slots)
            .flat_map(|(unit, found)| self.finish(unit, found.unwrap_or_default()))
            .collect()
    }
}
