//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The disassembly of one named function of one arm, written as a results
//! artifact to commit beside the runs:
//! `results/<bench>/disasm/<machine class>/<run>/<arm>/<symbol>.txt`
//! ([`mockspace_bench_layout::disasm_rel`]). Written once and never replaced,
//! like a run: the file is created only where none is, and a second artifact
//! of the same second and commit takes the next `-<k>` suffix on its run.

use std::io::{ErrorKind, Write};
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
    write_new(place, symbol, |run| {
        artifact_text(place, run, symbol, library, &found)
    })
}

/// Create the artifact under the first run id (`place.run`, then `-2`, `-3`,
/// ...) whose file does not exist yet, with `text` of the run id it took.
/// Creation is exclusive, so a file another writer made in between is never
/// replaced: that run id is passed over.
fn write_new(
    place: &ArtifactPlace<'_>,
    symbol: &str,
    text: impl Fn(&str) -> String,
) -> Result<PathBuf, String> {
    let runs =
        std::iter::once(place.run.to_string()).chain((2 ..).map(|k| format!("{}-{k}", place.run)));
    for run in runs {
        let rel = layout::disasm_rel(place.bench, place.machine, &run, place.arm, symbol);
        let path = place.results_root.join(rel);
        let parent = path.parent().unwrap_or(place.results_root);
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(text(&run).as_bytes())
                    .map_err(|e| format!("writing {}: {e}", path.display()))?;
                return Ok(path);
            },
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("creating {}: {e}", path.display())),
        }
    }
    unreachable!("the run ids are endless")
}

/// The artifact: what was read, with what, on what, then the tool's own text.
fn artifact_text(
    place: &ArtifactPlace<'_>,
    run: &str,
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
    fn a_second_artifact_takes_the_next_run_and_says_which() {
        let root = mockspace_manifest::scratch::tempdir().unwrap();
        let p = place(root.path(), "hash", "scalar");
        let text = |run: &str| format!("run: {run}\n");
        let first = write_new(&p, "bench_entry", text).unwrap();
        assert!(
            first.ends_with(
                "hash/disasm/linux-x86_64-c4-container-intel/20261116T093000Z-1a2b3c4/scalar/bench_entry.txt"
            ),
            "{}",
            first.display()
        );
        let second = write_new(&p, "bench_entry", text).unwrap();
        assert!(
            second.ends_with("20261116T093000Z-1a2b3c4-2/scalar/bench_entry.txt"),
            "{}",
            second.display()
        );
        assert_eq!(
            std::fs::read_to_string(&second).unwrap(),
            "run: 20261116T093000Z-1a2b3c4-2\n",
            "the artifact names the run directory it sits in"
        );
        assert_eq!(
            std::fs::read_to_string(&first).unwrap(),
            "run: 20261116T093000Z-1a2b3c4\n",
            "the first stands untouched"
        );
        assert!(layout::is_run_id("20261116T093000Z-1a2b3c4-2"));
    }

    #[test]
    fn the_header_names_the_run_directory_the_artifact_sits_in() {
        let p = place(Path::new("/r"), "hash", "scalar");
        let found = SymbolDisassembly {
            tool:       super::super::Disassembler {
                program: "llvm-objdump".into(),
                flavour: super::super::Flavour::Llvm,
            },
            text:       "0000000000001000 <bench_entry>:\n    1000:\tretq".to_string(),
            normalized: "retq".to_string(),
        };
        let text = artifact_text(
            &p,
            "20261116T093000Z-1a2b3c4-2",
            "bench_entry",
            Path::new("/x/libscalar.so"),
            &found,
        );
        assert!(text.contains("run: 20261116T093000Z-1a2b3c4-2\n"), "{text}");
        assert!(text.contains("# library: libscalar.so\n"), "{text}");
        assert!(text.ends_with("    1000:\tretq\n"), "{text}");
    }

    #[test]
    fn a_file_already_there_is_never_replaced() {
        let root = mockspace_manifest::scratch::tempdir().unwrap();
        let p = place(root.path(), "hash", "scalar");
        let taken = root
            .path()
            .join(layout::disasm_rel(p.bench, p.machine, p.run, p.arm, "f"));
        std::fs::create_dir_all(taken.parent().unwrap()).unwrap();
        std::fs::write(&taken, "someone else's").unwrap();
        let written = write_new(&p, "f", |_| "mine".to_string()).unwrap();
        assert_ne!(written, taken);
        assert_eq!(std::fs::read_to_string(&taken).unwrap(), "someone else's");
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
