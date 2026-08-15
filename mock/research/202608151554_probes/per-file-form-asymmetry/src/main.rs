//! Probe: the per-file bench form and the `[bench.<name>]` section form
//! do not accept the same keys, and nothing says which.
//!
//! Three structs carry the same twelve-field bench vocabulary:
//! `BenchSection` (`bench-harness/src/config.rs:203`), `ComposedBench`
//! (`bench-harness/src/tree.rs:102`) and `SweepSection`
//! (`bench-harness/src/tree.rs:138`). All three carry
//! `deny_unknown_fields`. Only `BenchSection` has a `normalise` field.
//!
//! So `[normalise]` is writable in one settled form and refused in the
//! other. That may well be the right call, since the flattened
//! `baseline`/`floor`/`delta` keys are the canonical spelling. It is
//! stated nowhere, and the refusal a consumer meets when moving a
//! section into a per-file bench names the key rather than the reason.
//!
//! NEGATIVE CONTROLS, stated before the run:
//!   C1 a per-file bench with the FLATTENED role keys must LOAD.
//!      Without it, the probe shows only that per-file refuses roles.
//!   C2 a root `[bench.x]` section with a `[normalise]` table must LOAD.
//!      Without it, `normalise` is simply dead and there is no asymmetry.

use std::fs;
use std::path::{Path, PathBuf};

use mockspace_bench_harness::tree;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("probe-perfile-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    d
}

/// A tree with one per-file member `warm/bench.toml` carrying `body`.
fn per_file(tag: &str, body: &str) -> Result<(), String> {
    let root = tmp(tag);
    fs::create_dir_all(root.join("warm/arms/k/src")).unwrap();
    fs::write(root.join("bench.toml"), "[timing]\npasses = 2\n").unwrap();
    fs::write(root.join("warm/bench.toml"), body).unwrap();
    let r = tree::load(&root).map(|_| ()).map_err(|e| e.to_string());
    let _ = fs::remove_dir_all(&root);
    r
}

/// A tree whose ROOT carries one `[bench.x]` section with `body` appended.
fn root_section(tag: &str, body: &str) -> Result<(), String> {
    let root = tmp(tag);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("bench.toml"),
        format!("[timing]\npasses = 2\n\n[bench.x]\ntitle = \"X\"\nworkload = \"default\"\narms = [\"k\"]\npoints = [7]\n{body}"),
    )
    .unwrap();
    let r = tree::load(&root).map(|_| ()).map_err(|e| e.to_string());
    let _ = fs::remove_dir_all(&root);
    r
}

fn show(label: &str, r: &Result<(), String>) {
    match r {
        Ok(()) => println!("{label:<52} LOADS"),
        Err(e) => println!("{label:<52} REFUSED\n{}", e.lines().map(|l| format!("      | {l}")).collect::<Vec<_>>().join("\n")),
    }
}

const FLAT_ROLES: &str = "baseline = \"k\"\ndelta = \"ratio\"\n";
const NORMALISE_TABLE: &str = "[normalise]\nbaseline = \"k\"\nmode = \"ratio\"\n";
/// The same table scoped under a root section, where it must be spelled
/// `[bench.x.normalise]` rather than `[normalise]`.
const NORMALISE_TABLE_SCOPED: &str =
    "[bench.x.normalise]\nbaseline = \"k\"\nmode = \"ratio\"\n";

fn main() {
    let base = "title = \"Warm\"\narms = [\"k\"]\npoints = [7]\n";

    // ── negative controls ──
    let c1 = per_file("c1", &format!("{base}{FLAT_ROLES}"));
    let c2 = root_section("c2", NORMALISE_TABLE_SCOPED);
    show("C1 per-file + flattened roles", &c1);
    show("C2 root section + [normalise] table", &c2);
    if c1.is_err() || c2.is_err() {
        println!("\ncontrols failed; there is no asymmetry to report. Findings void.");
        std::process::exit(1);
    }
    println!("\ncontrols pass: both forms accept roles in the spelling they support.\n");

    // ── the finding ──
    let f1 = per_file("f1", &format!("{base}{NORMALISE_TABLE}"));
    show("F1 per-file + [normalise] table", &f1);

    let f2 = root_section("f2", FLAT_ROLES);
    show("F2 root section + flattened roles", &f2);

    // and the sweep form, which is the third copy of the vocabulary
    let f3 = per_file(
        "f3",
        "title = \"Warm\"\narms = [\"k\"]\n[sweep.a]\npoints = [7]\nbaseline = \"k\"\n",
    );
    show("F3 [sweep.a] + flattened roles", &f3);
    let f4 = per_file(
        "f4",
        "title = \"Warm\"\narms = [\"k\"]\n[sweep.a]\npoints = [7]\n[sweep.a.normalise]\nbaseline = \"k\"\n",
    );
    show("F4 [sweep.a] + [normalise] table", &f4);

    let _ = Path::new("");
}
