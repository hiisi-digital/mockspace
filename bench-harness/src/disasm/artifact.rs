//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The disassembly of one named function of one arm, written as a results
//! artifact to commit beside the runs:
//! `results/<bench>/disasm/<machine class>/<run>/<arm>.<symbol>.txt`
//! ([`mockspace_bench_layout::disasm_rel`]). Written once and never replaced,
//! like a run: a second artifact of the same second and commit takes the next
//! `-<k>` suffix.

use std::path::{Path, PathBuf};

use mockspace_bench_layout as layout;

use super::{SymbolDisassembly, disassemble_symbol};

/// Where an artifact is written and what it says it was taken with.
pub struct ArtifactPlace<'a> {
    /// The bench tree's results root.
    pub results_root: &'a Path,
    /// The bench, as its results directory names it.
    pub bench:        &'a str,
    /// The arm.
    pub arm:          &'a str,
    /// The machine class the library was built and read on.
    pub machine:      &'a str,
    /// The run id: [`layout::run_id`] of now and the commit.
    pub run:          &'a str,
}

/// Disassemble `symbol` of `library` with this host's disassembler and write
/// it at `place`. Returns the file written.
///
/// # Errors
///
/// Where the disassembly fails ([`disassemble_symbol`]), a name cannot be a
/// path component, or the file cannot be written.
pub fn write_symbol_artifact(
    place: &ArtifactPlace<'_>,
    symbol: &str,
    library: &Path,
) -> Result<PathBuf, String> {
    for (what, name) in [("bench", place.bench), ("arm", place.arm), ("symbol", symbol)] {
        if !layout::is_name(name) {
            return Err(format!("{what} `{name}` cannot name a results file"));
        }
    }
    let found = disassemble_symbol(library, symbol)?;
    let path = free_path(place, symbol);
    let parent = path.parent().unwrap_or(place.results_root);
    std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    std::fs::write(&path, artifact_text(place, symbol, library, &found))
        .map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(path)
}

/// The first path for `place` and `symbol` no file holds yet.
fn free_path(place: &ArtifactPlace<'_>, symbol: &str) -> PathBuf {
    let at = |run: &str| {
        place.results_root.join(layout::disasm_rel(
            place.bench,
            place.machine,
            run,
            place.arm,
            symbol,
        ))
    };
    std::iter::once(place.run.to_string())
        .chain((2 ..).map(|k| format!("{}-{k}", place.run)))
        .map(|run| at(&run))
        .find(|p| !p.exists())
        .unwrap_or_else(|| at(place.run))
}

/// The artifact: what was read, with what, on what, then the tool's own text.
fn artifact_text(
    place: &ArtifactPlace<'_>,
    symbol: &str,
    library: &Path,
    found: &SymbolDisassembly,
) -> String {
    let file = library.file_name().map_or_else(
        || library.display().to_string(),
        |f| f.to_string_lossy().into_owned(),
    );
    format!(
        "# `{symbol}` of arm `{arm}` of bench `{bench}`\n\
         # library: {file}\n\
         # tool: {program} ({flavour:?})\n\
         # machine class: {machine}, run: {run}\n\
         \n\
         {text}\n",
        arm = place.arm,
        bench = place.bench,
        program = found.tool.program.display(),
        flavour = found.tool.flavour,
        machine = place.machine,
        run = place.run,
        text = found.text,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place<'a>(root: &'a Path, bench: &'a str, arm: &'a str) -> ArtifactPlace<'a> {
        ArtifactPlace {
            results_root: root,
            bench,
            arm,
            machine: "linux-x86_64-c4-container-intel",
            run: "20261116T093000Z-1a2b3c4",
        }
    }

    #[test]
    fn a_second_artifact_of_the_same_run_id_takes_the_next_suffix() {
        let root = mockspace_manifest::scratch::tempdir().unwrap();
        let p = place(root.path(), "hash", "scalar");
        let first = free_path(&p, "bench_entry");
        assert!(first.ends_with("hash/disasm/linux-x86_64-c4-container-intel/20261116T093000Z-1a2b3c4/scalar.bench_entry.txt"));
        std::fs::create_dir_all(first.parent().unwrap()).unwrap();
        std::fs::write(&first, "taken").unwrap();
        let second = free_path(&p, "bench_entry");
        assert!(
            second.ends_with("20261116T093000Z-1a2b3c4-2/scalar.bench_entry.txt"),
            "{}",
            second.display()
        );
        assert!(layout::is_run_id("20261116T093000Z-1a2b3c4-2"));
    }

    #[test]
    fn a_name_that_cannot_be_a_path_component_is_refused_before_anything_runs() {
        let root = mockspace_manifest::scratch::tempdir().unwrap();
        for (bench, arm, symbol) in [("a/b", "x", "f"), ("a", "../x", "f"), ("a", "x", "")] {
            let err = write_symbol_artifact(
                &place(root.path(), bench, arm),
                symbol,
                Path::new("/nonexistent"),
            )
            .unwrap_err();
            assert!(err.contains("cannot name a results file"), "{err}");
        }
        assert_eq!(
            std::fs::read_dir(root.path()).unwrap().count(),
            0,
            "nothing was written"
        );
    }
}
