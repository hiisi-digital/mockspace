//! Probe: what does the bench framework SAY when a person breaks a bench
//! in a plausible way?
//!
//! Each case builds a minimal but VALID benchspace on disk, applies exactly
//! one plausible authoring mistake, runs the loader, and prints the exact
//! diagnostic a person would see.
//!
//! NEGATIVE CONTROL (the case that must fail): case 0 is the unbroken tree.
//! If case 0 does not load cleanly, every other case's diagnostic is about
//! my scaffold rather than about the mistake, and the whole probe is void.
//! Case `control-broken` is the inverse: a tree broken in a way the loader
//! is KNOWN to catch, asserting the harness can produce an error at all.

use std::fs;
use std::path::{Path, PathBuf};

use mockspace_bench_harness::tree;

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, body).unwrap();
}

/// A minimal VALID benchspace: root globals, one composed member with one
/// sweep and two arms.
fn scaffold(root: &Path) {
    write(
        root,
        "bench.toml",
        r#"[timing]
passes = 3
runs_per_pass = 5
"#,
    );
    write(
        root,
        "widths/bench.toml",
        r#"title = "Packed against dense carriers"
master_seed = 0x1234
arms = ["packed", "dense"]

[sweep.width]
points = [1024, 4096]
"#,
    );
    write(root, "widths/arms/packed/src/lib.rs", "// arm\n");
    write(root, "widths/arms/dense/src/lib.rs", "// arm\n");
}

struct Case {
    name:  &'static str,
    /// What a person plausibly did.
    story: &'static str,
    /// Mutate the valid scaffold into the broken one.
    break_it: fn(&Path),
}

fn main() {
    let cases: Vec<Case> = vec![
        Case {
            name:  "00-control-valid",
            story: "NEGATIVE CONTROL: nothing broken. Must load clean.",
            break_it: |_| {},
        },
        Case {
            name:  "00b-control-must-fail",
            story: "NEGATIVE CONTROL: a member with no points and no sweeps. \
                    Known-refused; proves the loader can error at all.",
            break_it: |r| {
                write(
                    r,
                    "widths/bench.toml",
                    "title = \"t\"\narms = [\"packed\", \"dense\"]\n",
                )
            },
        },
        Case {
            name:  "01-typo-in-key",
            story: "Typed `arm =` instead of `arms =`.",
            break_it: |r| {
                write(
                    r,
                    "widths/bench.toml",
                    "title = \"t\"\narm = [\"packed\"]\n\n[sweep.width]\npoints = [1024]\n",
                )
            },
        },
        Case {
            name:  "02-typo-in-section",
            story: "Typed `[sweeps.width]` instead of `[sweep.width]`.",
            break_it: |r| {
                write(
                    r,
                    "widths/bench.toml",
                    "title = \"t\"\narms = [\"packed\"]\n\n[sweeps.width]\npoints = [1024]\n",
                )
            },
        },
        Case {
            name:  "03-missing-title",
            story: "Forgot `title`, which is the one required field.",
            break_it: |r| {
                write(
                    r,
                    "widths/bench.toml",
                    "arms = [\"packed\"]\n\n[sweep.width]\npoints = [1024]\n",
                )
            },
        },
        Case {
            name:  "04-unbalanced-toml",
            story: "Left a bracket open. The single most common text mistake.",
            break_it: |r| {
                write(
                    r,
                    "widths/bench.toml",
                    "title = \"t\"\narms = [\"packed\", \"dense\"\n\n[sweep.width]\npoints = [1024]\n",
                )
            },
        },
        Case {
            name:  "05-arm-named-not-present",
            story: "Renamed the arm directory and forgot the manifest.",
            break_it: |r| {
                fs::rename(r.join("widths/arms/dense"), r.join("widths/arms/densee")).unwrap()
            },
        },
        Case {
            name:  "06-arm-present-not-named",
            story: "Added an arm directory and forgot to list it. Silent?",
            break_it: |r| write(r, "widths/arms/simd/src/lib.rs", "// arm\n"),
        },
        Case {
            name:  "07-points-in-both-places",
            story: "Left a top-level `points` when adding a sweep.",
            break_it: |r| {
                write(
                    r,
                    "widths/bench.toml",
                    "title = \"t\"\narms = [\"packed\"]\npoints = [1]\n\n[sweep.width]\npoints = [1024]\n",
                )
            },
        },
        Case {
            name:  "08-seed-as-decimal-string",
            story: "Wrote the seed as a quoted hex string.",
            break_it: |r| {
                write(
                    r,
                    "widths/bench.toml",
                    "title = \"t\"\nmaster_seed = \"0x1234\"\narms = [\"packed\"]\n\n[sweep.width]\npoints = [1024]\n",
                )
            },
        },
        Case {
            name:  "09-root-toml-typo",
            story: "Typo in the ROOT globals file: `runs_per_pas`.",
            break_it: |r| {
                write(
                    r,
                    "bench.toml",
                    "[timing]\npasses = 3\nruns_per_pas = 5\n",
                )
            },
        },
        Case {
            name:  "10-member-listed-absent",
            story: "Listed a member in [benchspace] and never created it.",
            break_it: |r| {
                write(
                    r,
                    "bench.toml",
                    "[benchspace]\nmembers = [\"widths\", \"depths\"]\n",
                )
            },
        },
        Case {
            name:  "11-empty-arms",
            story: "Wrote the sweep before writing any arms.",
            break_it: |r| {
                write(
                    r,
                    "widths/bench.toml",
                    "title = \"t\"\n\n[sweep.width]\npoints = [1024]\n",
                )
            },
        },
        Case {
            name:  "12-no-bench-toml-at-all",
            story: "Made the bench directory, forgot the file.",
            break_it: |r| fs::remove_file(r.join("widths/bench.toml")).unwrap(),
        },
        Case {
            name:  "13-baseline-names-missing-arm",
            story: "`baseline` names an arm that is not in `arms`.",
            break_it: |r| {
                write(
                    r,
                    "widths/bench.toml",
                    "title = \"t\"\narms = [\"packed\", \"dense\"]\nbaseline = \"scalar\"\n\n[sweep.width]\npoints = [1024]\n",
                )
            },
        },
        Case {
            name:  "14-points-as-strings",
            story: "Quoted the point values.",
            break_it: |r| {
                write(
                    r,
                    "widths/bench.toml",
                    "title = \"t\"\narms = [\"packed\"]\n\n[sweep.width]\npoints = [\"1024\"]\n",
                )
            },
        },
    ];

    for c in &cases {
        let root = PathBuf::from(std::env::temp_dir())
            .join("diagprobe")
            .join(c.name);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        scaffold(&root);
        (c.break_it)(&root);

        println!("╔══ {} ", c.name);
        println!("║ story: {}", c.story);
        match tree::load(&root) {
            Ok(t) => {
                let mut keys: Vec<_> = t.manifest.bench.keys().cloned().collect();
                keys.sort();
                let mut arms: Vec<_> = t
                    .arms
                    .iter()
                    .map(|a| format!("{}/{}", a.bench, a.arm))
                    .collect();
                arms.sort();
                println!("║ RESULT: Ok");
                println!("║   cells: {keys:?}");
                println!("║   arms:  {arms:?}");
            },
            Err(e) => {
                println!("║ RESULT: Err");
                for line in format!("{e}").lines() {
                    println!("║   {line}");
                }
            },
        }
        println!("╚══");
        println!();
    }
}
