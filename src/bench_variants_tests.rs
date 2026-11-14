//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::fs;
use std::path::Path;

use super::*;

/// A bench tree with three variant crates: one named by its directory, one
/// whose library name differs from its directory, one with no `[lib]`.
fn plant(bench_toml: &str) -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    let crates = [
        ("fnv1a", "[package]\nname = \"fnv1a\"\n"),
        ("satfold-seq", "[package]\nname = \"bench-satfold-seq\"\n[lib]\nname = \"bench_satfold_seq\"\n"),
        ("plain-arm", "[package]\nname = \"plain-arm\"\n"),
    ];
    for (dir, manifest) in crates {
        fs::create_dir_all(t.path().join("variants").join(dir)).unwrap();
        fs::write(t.path().join("variants").join(dir).join("Cargo.toml"), manifest).unwrap();
    }
    fs::write(t.path().join("bench.toml"), bench_toml).unwrap();
    t
}

fn build(dir: &str, target: Option<&Path>) -> VariantBuild {
    VariantBuild { dir: dir.to_string(), target_dir: target.map(Path::to_path_buf) }
}

#[test]
fn an_artifact_path_entry_builds_the_crate_of_that_library_into_the_target_it_names() {
    let t = plant(
        "[bench.sat]\n[[bench.sat.sizes]]\nn = 1\nvariants = [\"../target/release/bench_satfold_seq\", \"../target/release/plain_arm\"]\n",
    );
    let plan = plan_for(t.path(), &["sat"]).unwrap();
    let td = t.path().join("../target");
    assert_eq!(plan.builds, vec![build("satfold-seq", Some(&td)), build("plain-arm", Some(&td))]);
    assert_eq!(plan.unmatched, 0);
}

#[test]
fn an_artifact_path_with_its_platform_name_finds_the_same_crate() {
    let libs = lib_names(&plant("[bench.x]\n").path().to_path_buf());
    let file = format!("{}bench_satfold_seq{}", std::env::consts::DLL_PREFIX, std::env::consts::DLL_SUFFIX);
    let entry = format!("../target/release/{file}");
    let b = build_for(&entry, Path::new("/b"), &libs).unwrap();
    assert_eq!(b, build("satfold-seq", Some(Path::new("/b/../target"))));
}

#[test]
fn a_short_name_and_a_variants_path_build_with_cargos_own_target() {
    let t = plant("[bench.h]\nvariants = [\"fnv1a\", \"variants/plain-arm/target/release/plain_arm\"]\n");
    let plan = plan_for(t.path(), &["h"]).unwrap();
    assert_eq!(plan.builds, vec![build("fnv1a", None), build("plain-arm", None)]);
}

#[test]
fn an_entry_naming_no_crate_is_counted_and_never_built() {
    let t = plant("[bench.h]\nvariants = [\"../target/release/nobody\", \"/opt/elsewhere/libx\"]\n");
    let plan = plan_for(t.path(), &["h"]).unwrap();
    assert!(plan.builds.is_empty());
    assert_eq!(plan.unmatched, 2);
}

#[test]
fn an_empty_selection_plans_every_bench() {
    let t = plant("[bench.a]\nvariants = [\"fnv1a\"]\n[bench.b]\nvariants = [\"../target/release/bench_satfold_seq\"]\n");
    let plan = plan_for(t.path(), &[]).unwrap();
    assert_eq!(plan.builds.len(), 2);
    assert_eq!(plan.builds_of("plain-arm"), vec![build("plain-arm", None)]);
}

#[test]
fn an_undeclared_bench_is_refused_with_the_ones_there_are() {
    let t = plant("[bench.a]\nvariants = [\"fnv1a\"]\n");
    let err = plan_for(t.path(), &["zz"]).unwrap_err();
    assert!(err.contains("`zz` not found") && err.contains("Available: a"), "{err}");
}
