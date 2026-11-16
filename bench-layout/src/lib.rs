//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Where the bench harness keeps what it measures.
//!
//! One layout for every bench, relative to the bench tree root
//! (`<mock>/benches/`):
//!
//! ```text
//! results/<bench>/<sweep>_n<point>/<machine class>/<run>/
//!     samples.csv   every timed batch
//!     meta.json     the environment the run was taken in
//!     report.md     the findings rendered from the samples
//!     summary.tsv   per-variant medians and intervals, the history rows
//! ```
//!
//! A run directory holds those four and whatever an `after_cell` hook wrote
//! beside them as the cell's evidence, and nothing deeper.
//!
//! A run directory is written once and never replaced: the run id is the UTC
//! start of the run and the commit it measured, and the machine class sits
//! above it, so a re-run on the same machine adds a sibling and a run on
//! another machine adds a sibling tree. A cell's history is its runs, read
//! across them; there is no separate ledger.
//!
//! Beside the runs the harness owns `results/INDEX.md` (derived from the runs,
//! regenerated), `results/void/` (crash-borne runs, kept as evidence) and
//! `results/.inflight/` (a run in progress). Nothing else under the bench tree
//! is a bench output, and [`place`] says which of the older layouts a stray one
//! came from and whether it translates.

use std::path::{Path, PathBuf};

mod kind;
#[cfg(test)]
mod tests;

pub use kind::{
    Kind,
    LOCK_HEADER,
    SECTION_LOCK_SUFFIX,
    SNAPSHOT_LOCK,
    TRACKING_SOURCE,
    glob_match,
    lock_name,
    source_id,
};

/// The directory under the bench tree root that holds every run.
pub const RESULTS_DIR: &str = "results";
/// A run's timed batches.
pub const SAMPLES_FILE: &str = "samples.csv";
/// A run's environment record.
pub const META_FILE: &str = "meta.json";
/// A run's rendered findings.
pub const REPORT_FILE: &str = "report.md";
/// A run's per-variant summary rows, which are its history.
pub const SUMMARY_FILE: &str = "summary.tsv";
/// Every file a run directory may hold.
pub const RUN_FILES: [&str; 4] = [SAMPLES_FILE, META_FILE, REPORT_FILE, SUMMARY_FILE];
/// The index of every run, derived from the runs.
pub const INDEX_FILE: &str = "INDEX.md";
/// Crash-borne runs, quarantined by the harness.
pub const VOID_DIR: &str = "void";
/// A run in progress, staged until it completes.
pub const INFLIGHT_DIR: &str = ".inflight";
/// The header line of a [`SUMMARY_FILE`]: the schema the old ledger used, and a
/// tenth column, `source`, naming the code a row measured. A snapshot's row
/// carries its [`source_id`], a tracking bench's carries [`TRACKING_SOURCE`],
/// and a row written before the column existed has none.
pub const SUMMARY_HEADER: &str =
    "# schema_v2\ttimestamp\tgit_commit\tbenchmark\tvariant\tn\tmode\tmedian_ns\tci_lo_ns\tci_hi_ns\tsource";

/// One cell of a bench: a sweep at one point, named `<sweep>_n<point>`.
///
/// A flat bench has one sweep named after itself, so its cells read
/// `<bench>_n<point>`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cell {
    /// The sweep, or the bench itself in a flat tree.
    pub sweep: String,
    /// The point (`n`).
    pub n:     u64,
}

impl Cell {
    /// The cell's directory name.
    #[must_use]
    pub fn name(&self) -> String {
        format!("{}_n{}", self.sweep, self.n)
    }

    /// Parse a cell name back. The point is the digits after the last `_n`, so
    /// a sweep that itself contains `_n` (`bit_n_ops_n64`) still parses.
    #[must_use]
    pub fn parse(name: &str) -> Option<Cell> {
        let at = name.rfind("_n")?;
        let (sweep, digits) = (&name[.. at], &name[at + 2 ..]);
        if !is_name(sweep) || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some(Cell {
            sweep: sweep.to_string(),
            n:     digits.parse().ok()?,
        })
    }
}

/// Whether `s` can name a bench or a sweep: non-empty, ASCII letters, digits,
/// `-`, `_` and `.`, and not a dot directory.
#[must_use]
pub fn is_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('.')
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Whether `s` has the shape the harness gives a machine class,
/// `<os>-<arch>-c<cores|x>-<container|bare>-<family>`.
#[must_use]
pub fn is_machine_class(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    let token = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    parts.len() == 5
        && token(parts[0])
        && token(parts[1])
        && parts[2]
            .strip_prefix('c')
            .is_some_and(|c| c == "x" || (!c.is_empty() && c.bytes().all(|b| b.is_ascii_digit())))
        && matches!(parts[3], "container" | "bare")
        && token(parts[4])
}

/// The run id of a run that started at `started` (unix seconds) over `commit`:
/// `YYYYMMDDTHHMMSSZ-<commit>`. A commit that is empty or `unknown` reads
/// `nogit`; any character a directory name should not carry becomes `_`.
#[must_use]
pub fn run_id(started: u64, commit: &str) -> String {
    let commit = commit.trim();
    let commit = if commit.is_empty() || commit == "unknown" {
        "nogit".to_string()
    } else {
        commit
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
            .collect()
    };
    format!("{}-{commit}", utc_stamp(started))
}

/// Whether `s` has the shape of a [`run_id`]. A suffix after the commit is
/// allowed, so a run that met an existing directory of the same id can take
/// one rather than replace it.
#[must_use]
pub fn is_run_id(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() > 17
        && b[.. 8].iter().all(u8::is_ascii_digit)
        && b[8] == b'T'
        && b[9 .. 15].iter().all(u8::is_ascii_digit)
        && b[15] == b'Z'
        && b[16] == b'-'
        && s[17 ..].bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

/// Unix seconds as `YYYYMMDDTHHMMSSZ`, in UTC.
#[must_use]
pub fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// One run's directory, relative to the results root.
#[must_use]
pub fn run_rel(bench: &str, cell: &Cell, machine: &str, run: &str) -> PathBuf {
    Path::new(bench).join(cell.name()).join(machine).join(run)
}

/// Which part of a run an old-layout file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Part {
    /// `<cell>.csv`, now [`SAMPLES_FILE`].
    Samples,
    /// `<cell>.meta.json`, now [`META_FILE`].
    Meta,
    /// `<cell>_findings.md` or `<cell>_report.md`, now [`REPORT_FILE`].
    Report,
    /// A ledger `<cell>.tsv`, now one [`SUMMARY_FILE`] per run it recorded.
    History,
}

impl Part {
    /// The file this part becomes in a run directory.
    #[must_use]
    pub const fn run_file(self) -> &'static str {
        match self {
            Part::Samples => SAMPLES_FILE,
            Part::Meta => META_FILE,
            Part::Report => REPORT_FILE,
            Part::History => SUMMARY_FILE,
        }
    }
}

/// A file in an older layout that translates one to one into a run.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Legacy {
    /// The bench the file belongs to.
    pub bench: String,
    /// The cell it measured.
    pub cell:  Cell,
    /// What it holds.
    pub part:  Part,
}

/// Why a bench output sits where nothing translates it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stray {
    /// Under `results/` but not in a run directory.
    InResults,
    /// A history ledger somewhere other than the two the old harness wrote.
    Ledger,
    /// A file shaped like a harness output, outside `results/`.
    Outside,
}

impl Stray {
    /// What is wrong, in a sentence a finding can carry.
    #[must_use]
    pub const fn says(self) -> &'static str {
        match self {
            Stray::InResults => "sits under results/ but outside any run directory",
            Stray::Ledger => "is a history ledger, which the harness no longer keeps",
            Stray::Outside => "is a bench output outside results/",
        }
    }
}

/// Where a path under the bench tree stands against the layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Not a bench output at all.
    NotOutput,
    /// Exactly where the harness writes it.
    InLayout,
    /// An older layout `mock bench migrate` translates one to one.
    Legacy(Legacy),
    /// A bench output nothing translates; it is moved by hand.
    Stray(Stray),
}

/// The part an old output file name holds, and its cell.
fn old_output(name: &str) -> Option<(&str, Cell, Part)> {
    let (stem, part) = if let Some(s) = name.strip_suffix(".meta.json") {
        (s, Part::Meta)
    } else if let Some(s) = name.strip_suffix("_findings.md") {
        (s, Part::Report)
    } else if let Some(s) = name.strip_suffix("_report.md") {
        (s, Part::Report)
    } else if let Some(s) = name.strip_suffix(".csv") {
        (s, Part::Samples)
    } else {
        return None;
    };
    Some((stem, Cell::parse(stem)?, part))
}

/// Where the file at `rel`, a path relative to the bench tree root, stands.
///
/// The older layouts it recognises, and translates:
///
/// - `results/<bench>/<sweep>_n<point>.csv`, `.meta.json`, `_findings.md`,
///   `_report.md`: the harness's own flat result files.
/// - `<bench>_n<point>.csv` and its siblings at the bench root: a
///   consumer-owned driver writing into its working directory.
/// - `.bench_history/<bench>_n<point>.tsv`: the flat ledger.
/// - `history/<bench>/<sweep>_n<point>.tsv`: the nested ledger.
///
/// Every other ledger, every other output and every other file under
/// `results/` is a [`Stray`]. Inside a crate, under `support/` or `arms/` or below
/// any directory holding a `Cargo.toml` other than the bench root, a file named
/// like an output counts only beside its `<sweep>_n<point>.meta.json`: input
/// data and golden files there carry the shape and are not outputs. Anywhere
/// else such a file is one, with its meta or without, since that is where a
/// runner of its own wrote bare CSVs. `exists` answers whether a path relative
/// to the bench tree root is there. Callers skip `target/` and `.git/`.
#[must_use]
pub fn place(rel: &Path, exists: impl Fn(&Path) -> bool) -> Placement {
    let parts: Vec<&str> = rel.iter().filter_map(|c| c.to_str()).collect();
    let Some(&name) = parts.last() else {
        return Placement::NotOutput;
    };
    if parts.first() == Some(&RESULTS_DIR) {
        return place_in_results(&parts);
    }
    if let Some(at) = parts.iter().position(|p| *p == ".bench_history") {
        if at == 0 && parts.len() == 2 {
            if let Some(cell) = name.strip_suffix(".tsv").and_then(Cell::parse) {
                return Placement::Legacy(Legacy {
                    bench: cell.sweep.clone(),
                    cell,
                    part: Part::History,
                });
            }
        }
        return Placement::Stray(Stray::Ledger);
    }
    if parts.first() == Some(&"history") && parts.len() > 1 {
        if parts.len() == 3 && is_name(parts[1]) {
            if let Some(cell) = name.strip_suffix(".tsv").and_then(Cell::parse) {
                return Placement::Legacy(Legacy {
                    bench: parts[1].to_string(),
                    cell,
                    part: Part::History,
                });
            }
        }
        return Placement::Stray(Stray::Ledger);
    }
    if let Some((stem, cell, part)) = old_output(name) {
        let meta = rel.with_file_name(format!("{stem}.meta.json"));
        let in_crate = parts.iter().any(|p| *p == "support" || *p == "arms")
            || rel
                .ancestors()
                .skip(1)
                .filter(|a| !a.as_os_str().is_empty())
                .any(|a| exists(&a.join("Cargo.toml")));
        if in_crate && part != Part::Meta && !exists(&meta) {
            return Placement::NotOutput;
        }
        if parts.len() == 1 {
            return Placement::Legacy(Legacy {
                bench: cell.sweep.clone(),
                cell,
                part,
            });
        }
        return Placement::Stray(Stray::Outside);
    }
    Placement::NotOutput
}

fn place_in_results(parts: &[&str]) -> Placement {
    match parts {
        [_, file] if *file == INDEX_FILE => Placement::InLayout,
        [_, dir, ..] if *dir == VOID_DIR || *dir == INFLIGHT_DIR => Placement::InLayout,
        [_, bench, cell, machine, run, _]
            if is_name(bench)
                && Cell::parse(cell).is_some()
                && is_machine_class(machine)
                && is_run_id(run) =>
        {
            Placement::InLayout
        },
        [_, bench, file] if is_name(bench) => {
            match old_output(file) {
                Some((_, cell, part)) => {
                    Placement::Legacy(Legacy {
                        bench: (*bench).to_string(),
                        cell,
                        part,
                    })
                },
                None => Placement::Stray(Stray::InResults),
            }
        },
        _ => Placement::Stray(Stray::InResults),
    }
}

/// One run directory found under a results root.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RunDir {
    /// The bench.
    pub bench:   String,
    /// The cell.
    pub cell:    Cell,
    /// The machine class.
    pub machine: String,
    /// The run id.
    pub run:     String,
    /// The directory itself.
    pub dir:     PathBuf,
}

/// Every run directory under `results_root`, sorted by bench, cell, machine
/// and run, which is oldest first within a machine since a run id opens with
/// its UTC start.
#[must_use]
pub fn runs(results_root: &Path) -> Vec<RunDir> {
    let mut out = Vec::new();
    for bench in subdirs(results_root) {
        if !is_name(&bench) || bench == VOID_DIR {
            continue;
        }
        let bench_dir = results_root.join(&bench);
        for cell_name in subdirs(&bench_dir) {
            let Some(cell) = Cell::parse(&cell_name) else {
                continue;
            };
            let cell_dir = bench_dir.join(&cell_name);
            for machine in subdirs(&cell_dir) {
                if !is_machine_class(&machine) {
                    continue;
                }
                let machine_dir = cell_dir.join(&machine);
                for run in subdirs(&machine_dir) {
                    if !is_run_id(&run) {
                        continue;
                    }
                    out.push(RunDir {
                        bench: bench.clone(),
                        cell: cell.clone(),
                        machine: machine.clone(),
                        dir: machine_dir.join(&run),
                        run,
                    });
                }
            }
        }
    }
    out.sort();
    out
}

/// The names of the directories directly under `dir`, sorted; empty when it
/// cannot be read.
fn subdirs(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect();
    names.sort();
    names
}
