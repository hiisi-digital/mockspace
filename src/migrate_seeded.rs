//! The note of which refs this clone took the seed step of, which is what tells a clone a run of
//! `directory-to-ref` went through from a fresh clone that instantiated the ref.

use std::path::{Path, PathBuf};

use super::git;
use crate::config::Config;
use crate::data_refs::DataRefDecl;

/// The file of the common git directory naming, one a line, the namespace of each ref whose seed
/// step this clone took. `data-refs instantiate` leaves a record of the instance as the seed does,
/// so the record alone does not tell a clone a run went through from a fresh clone that
/// instantiated the ref; this file is written by the seed step only.
const SEEDED: &str = "mock-seeded";

fn seeded_file(cfg: &Config) -> Result<PathBuf, String> {
    let common = git(&cfg.repo_root, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    Ok(Path::new(common.trim_end_matches('\n')).join(SEEDED))
}

/// Notes that the seed step of `decl` is taken in this clone. Written before the ref is made, so
/// a run stopped anywhere inside the seed still reads as one that went through here.
pub(super) fn mark_seeded(cfg: &Config, decl: &DataRefDecl) -> Result<(), String> {
    let file = seeded_file(cfg)?;
    let ns = decl.namespace();
    let held = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("reading {}: {e}", file.display())),
    };
    if held.lines().any(|l| l == ns) {
        return Ok(());
    }
    use std::io::Write as _;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .and_then(|mut f| f.write_all(format!("{ns}\n").as_bytes()))
        .map_err(|e| format!("writing {}: {e}", file.display()))
}

/// Whether the seed step of the ref was taken in this clone and left it the record of the instance,
/// which is what says a run of the migration went through here. A file that cannot be read stops
/// the plan.
pub(super) fn seeded_here(cfg: &Config, decl: &DataRefDecl) -> Result<bool, Vec<String>> {
    let ns = decl.namespace();
    let file = seeded_file(cfg).map_err(|e| vec![format!("migrate: {e}")])?;
    let marked = match std::fs::read_to_string(&file) {
        Ok(text) => text.lines().any(|l| l == ns),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(vec![format!("migrate: reading {}: {e}", file.display())]),
    };
    if !marked {
        return Ok(false);
    }
    decl.open(&cfg.repo_root)
        .and_then(|store| decl.instance(&store, &cfg.mock_dir).and_then(|inst| inst.base().map_err(|e| e.to_string())))
        .map(|base| base.is_some())
        .map_err(|e| vec![format!("migrate: the record of the instance of `{ns}` could not be read: {e}")])
}
