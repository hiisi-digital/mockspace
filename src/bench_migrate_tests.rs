//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock bench migrate` against real files: hilavitkutin's committed bench
//! outputs, copied unchanged into `tests/fixtures/bench-migrate/hilavitkutin/`.

use super::*;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bench-migrate/hilavitkutin");
const M1: &str = "macos-aarch64-c8-bare-apple";

/// A copy of the fixture tree to migrate, without its README.
fn tree(tag: &str) -> tempfile::TempDir {
    let tmp = tempfile::Builder::new().prefix(&format!("bench-migrate-{tag}-")).tempdir().unwrap();
    for rel in files(Path::new(FIXTURE)) {
        if rel == Path::new("README.md") {
            continue;
        }
        let to = tmp.path().join(&rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(Path::new(FIXTURE).join(&rel), to).unwrap();
    }
    tmp
}

fn fixture(rel: &str) -> Vec<u8> {
    std::fs::read(Path::new(FIXTURE).join(rel)).unwrap()
}

fn run_dir(bench: &str, cell: &str, run: &str) -> PathBuf {
    Path::new("results").join(bench).join(cell).join(M1).join(run)
}

fn hand(plan: &Plan) -> Vec<String> {
    plan.by_hand.iter().map(|(p, _)| p.display().to_string()).collect()
}

/// The case that must refuse: none of these files names the machine that
/// measured it, so without a class nothing moves and every one is named.
#[test]
fn without_a_machine_class_nothing_moves() {
    let t = tree("no-class");
    let plan = plan(t.path(), None);
    assert!(plan.writes.is_empty(), "{:?}", plan.writes.iter().map(|w| &w.target).collect::<Vec<_>>());
    assert_eq!(hand(&plan), [
        ".bench_history/swap_band_n64.tsv",
        "frame_wide/heavy_n65536.meta.json",
        "results/swap_band/swap_band_n64.csv",
        "results/swap_band/swap_band_n64.meta.json",
        "results/swap_band/swap_band_n64_findings.md",
        "rsb_bwide_n256.csv",
        "rsb_bwide_n256.meta.json",
        "rsb_bwide_n256_findings.md",
    ]);
    assert!(plan.by_hand.iter().any(|(_, why)| why.contains("--machine-class")));
}

/// A class that is not the machine the metas kept is refused for the results
/// and for the ledger of the same cell, which has no machine of its own to
/// check against.
#[test]
fn a_class_the_metas_contradict_is_refused() {
    let t = tree("wrong-class");
    let plan = plan(t.path(), Some("linux-x86_64-c4-container-intel"));
    assert!(plan.writes.is_empty());
    assert!(hand(&plan).contains(&".bench_history/swap_band_n64.tsv".to_string()));
    assert!(plan.by_hand.iter().any(|(_, why)| why.contains("Apple M1")), "{:?}", plan.by_hand);
}

/// Every translatable file lands in its run, byte for byte; the ledger splits
/// into one summary per run it recorded, each row as it was written; the
/// sources and the emptied directories go; the member's stray stays and is
/// named; and afterwards nothing but that stray is outside the layout.
#[test]
fn every_translatable_file_lands_in_its_run_byte_for_byte() {
    let t = tree("migrate");
    let root = t.path();
    let plan = plan(root, Some(M1));
    assert_eq!(hand(&plan), ["frame_wide/heavy_n65536.meta.json"]);
    apply(root, &plan).unwrap();

    // The swap_band triple: run named by its meta (1794698888, 266650fb-dirty).
    let run = run_dir("swap_band", "swap_band_n64", "20261114T232808Z-266650fb-dirty");
    assert_eq!(std::fs::read(root.join(&run).join("samples.csv")).unwrap(), fixture("results/swap_band/swap_band_n64.csv"));
    assert_eq!(std::fs::read(root.join(&run).join("meta.json")).unwrap(), fixture("results/swap_band/swap_band_n64.meta.json"));
    assert_eq!(
        std::fs::read(root.join(&run).join("report.md")).unwrap(),
        fixture("results/swap_band/swap_band_n64_findings.md")
    );
    // The root driver's triple, at its own run.
    let rsb = run_dir("rsb_bwide", "rsb_bwide_n256", "20260702T102747Z-677b60f8");
    assert_eq!(std::fs::read(root.join(&rsb).join("samples.csv")).unwrap(), fixture("rsb_bwide_n256.csv"));

    // The ledger: two runs, each its own rows under the header. The newer run
    // is the one whose results moved above (the ledger kept its commit without
    // the `-dirty` mark), so its rows land in that run's directory, which then
    // holds all four files; the older run has no results left and keeps its
    // own id.
    let ledger = String::from_utf8(fixture(".bench_history/swap_band_n64.tsv")).unwrap();
    let rows: Vec<&str> = ledger.lines().filter(|l| !l.starts_with('#')).collect();
    let older = root
        .join(run_dir("swap_band", "swap_band_n64", "20260719T185015Z-8c3a949d"))
        .join("summary.tsv");
    assert_eq!(std::fs::read_to_string(&older).unwrap(), format!("{SUMMARY_HEADER}\n{}\n", rows[.. 3].join("\n")));
    assert_eq!(
        std::fs::read_to_string(root.join(&run).join("summary.tsv")).unwrap(),
        format!("{SUMMARY_HEADER}\n{}\n", rows[3 ..].join("\n"))
    );
    let mut in_run: Vec<String> = std::fs::read_dir(root.join(&run))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    in_run.sort();
    assert_eq!(in_run, ["meta.json", "report.md", "samples.csv", "summary.tsv"]);
    let cell_runs = std::fs::read_dir(root.join("results/swap_band/swap_band_n64").join(M1)).unwrap().count();
    assert_eq!(cell_runs, 2, "one directory per old run, the two halves of one run not split");

    // Sources gone, emptied directories gone, the stray in place.
    assert!(!root.join(".bench_history").exists());
    assert!(!root.join("rsb_bwide_n256.csv").exists());
    assert!(!root.join("results/swap_band/swap_band_n64.csv").exists());
    assert!(root.join("frame_wide/heavy_n65536.meta.json").exists());
    let outside: Vec<PathBuf> = files(root)
        .into_iter()
        .filter(|r| !matches!(layout::place(r, |p| root.join(p).exists()), Placement::InLayout | Placement::NotOutput))
        .collect();
    assert_eq!(outside, [PathBuf::from("frame_wide/heavy_n65536.meta.json")]);

    // And it is idempotent: a second pass has nothing to write.
    let again = super::plan(root, Some(M1));
    assert!(again.writes.is_empty() && again.already.is_empty());
}

/// A target already holding other bytes is never replaced: the source stays,
/// every write from it is withheld, and it is named. A target holding the same
/// bytes is a copy already moved, and only the source goes.
#[test]
fn a_target_with_other_bytes_is_never_replaced_and_one_with_the_same_is_settled() {
    let t = tree("collide");
    let root = t.path();
    let older = root
        .join(run_dir("swap_band", "swap_band_n64", "20260719T185015Z-8c3a949d"))
        .join("summary.tsv");
    std::fs::create_dir_all(older.parent().unwrap()).unwrap();
    std::fs::write(&older, "someone else's\n").unwrap();
    let rsb = root.join(run_dir("rsb_bwide", "rsb_bwide_n256", "20260702T102747Z-677b60f8"));
    std::fs::create_dir_all(&rsb).unwrap();
    std::fs::write(rsb.join("samples.csv"), fixture("rsb_bwide_n256.csv")).unwrap();

    let plan = plan(root, Some(M1));
    assert!(hand(&plan).iter().any(|p| p.ends_with(".bench_history/swap_band_n64.tsv")), "{:?}", hand(&plan));
    assert!(plan.writes.iter().all(|w| !w.source.ends_with("swap_band_n64.tsv")));
    apply(root, &plan).unwrap();
    assert_eq!(std::fs::read_to_string(&older).unwrap(), "someone else's\n");
    assert!(root.join(".bench_history/swap_band_n64.tsv").exists(), "the ledger stays whole");
    assert!(!root.join("rsb_bwide_n256.csv").exists(), "the copy already in place lets its source go");
    assert!(rsb.join("meta.json").exists(), "and the rest of its triple moves");
}

fn meta(ts: u64, commit: &str) -> OldMeta {
    OldMeta {
        timestamp: ts,
        commit: commit.into(),
        ..OldMeta::default()
    }
}

/// A ledger run takes the latest meta of its commit taken no later than it,
/// `-dirty` stripped on both sides; a meta goes to the nearest run after it
/// only, and a run with no meta of its commit keeps its own id.
#[test]
fn a_ledger_run_takes_the_meta_of_the_run_it_recorded() {
    let metas = [meta(100, "abc-dirty"), meta(300, "abc"), meta(150, "def")];
    let groups = [
        (90, "abc".to_string()),
        (101, "abc".to_string()),
        (120, "abc".to_string()),
        (301, "abc-dirty".to_string()),
        (160, "xyz".to_string()),
    ];
    let got: Vec<Option<u64>> = match_metas(&groups, &metas).into_iter().map(|m| m.map(|m| m.timestamp)).collect();
    assert_eq!(got, [None, Some(100), None, Some(300), None]);
}
