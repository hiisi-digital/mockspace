//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::*;

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// A tiny arm in the starter's hand-written export shape; `NAME` is the arm.
const ARM_LIB: &str = r#"
use mockspace_bench_core::{abi_hash, timed, FfiBenchCall};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bench_entry(
    input_ptr: *const u8,
    output_ptr: *mut u8,
    _n: usize,
) -> FfiBenchCall {
    let input = unsafe { &*(input_ptr as *const u64) };
    let output = unsafe { &mut *(output_ptr as *mut u64) };
    timed! {
        run { *output = input.wrapping_add(1); }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_name() -> *const u8 {
    b"NAME\0".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_abi_hash() -> u64 {
    abi_hash()
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_output_size(_n: usize) -> usize {
    8
}
"#;

/// Every file under `root` (and `root` itself when it is a file) whose name
/// satisfies `pred`, without following symlinks. Directories count by name too,
/// so a leftover `mockspace-bench-driver-*` directory is found.
fn find(root: &Path, pred: &dyn Fn(&str) -> bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if e.file_name().to_str().is_some_and(pred) {
            out.push(path.clone());
        }
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            find(&path, pred, out);
        }
    }
}

fn found(roots: &[&Path], pred: &dyn Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for r in roots {
        find(r, pred, &mut out);
    }
    out.sort();
    out.dedup();
    out
}

fn is_bench_core_rlib(name: &str) -> bool {
    name.starts_with("libmockspace_bench_core-") && name.ends_with(".rlib")
}

/// Copies of the shared dependency built from this checkout's sources: its
/// dependency record names the path, so another checkout building beside this
/// test in the same target is not counted.
fn bench_core_copies(roots: &[&Path]) -> usize {
    let repo = env!("CARGO_MANIFEST_DIR");
    found(roots, &is_bench_core_rlib)
        .iter()
        .filter(|rlib| {
            std::fs::read_to_string(rlib.with_extension("d")).is_ok_and(|d| d.contains(repo))
        })
        .count()
}

/// Whether `name` is something cargo wrote for one of `crates` (crate names with
/// underscores): an artifact, a dependency record or a fingerprint directory.
fn is_a_build_of(name: &str, crates: &[String]) -> bool {
    crates.iter().any(|c| {
        let stem = name.split('.').next().unwrap_or(name);
        let stem = stem.strip_prefix("lib").unwrap_or(stem);
        let dashed = c.replace('_', "-");
        stem == c
            || stem.strip_prefix(c.as_str()).is_some_and(|r| r.starts_with('-'))
            || stem.strip_prefix(dashed.as_str()).is_some_and(|r| r.starts_with('-'))
    })
}

/// One bench of two arms, run through `mock bench run`. The shared target is
/// wherever cargo resolves it for this process (`CARGO_TARGET_DIR`, or the mock
/// directory's own `target/`), so the test reads both that and the fixture.
///
/// What it holds: once the results are written, nothing the run built is left
/// (no per-arm target, no generated driver build, no arm or driver artifact in
/// the shared target), and the dependency every arm and the driver share was
/// compiled at most once by this run, not once per arm and once for the driver.
#[test]
fn a_run_leaves_no_arm_or_driver_build_and_compiles_the_shared_dependency_once() {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let root = tmp.path();
    let mock_dir = root.join("mock");
    let bench_dir = mock_dir.join("benches");
    let repo = env!("CARGO_MANIFEST_DIR");
    write(
        &bench_dir.join("bench.toml"),
        &format!(
            "[build]\nmockspace = '{{ path = \"{repo}\" }}'\nopt-level = 0\nlto = \"off\"\n\
             codegen-units = 16\n\n[timing]\npasses = 1\nruns_per_pass = 20\nbatch_size = 5\n\
             harness_runs = 1\ncooldowns_ms = [0]\n"
        ),
    );
    // crate names of this test alone, so a build beside it in the shared target
    // is not mistaken for ours
    let arms = [format!("armx{}a", std::process::id()), format!("armx{}b", std::process::id())];
    write(
        &bench_dir.join("pair").join("bench.toml"),
        &format!(
            "title = \"Pair\"\nworkload = \"default\"\narms = [\"{}\", \"{}\"]\npoints = [64]\nmaster_seed = 7\n",
            arms[0], arms[1]
        ),
    );
    for arm in &arms {
        write(
            &bench_dir.join("pair").join("arms").join(arm).join("src").join("lib.rs"),
            &ARM_LIB.replace("NAME", arm),
        );
    }

    let own_target = mock_dir.join("target");
    let shared = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or(own_target.clone());
    let before = bench_core_copies(&[&shared]);

    let cfg = crate::config::Config::from_dir(&mock_dir);
    let code = crate::bench::cmd(&cfg, &["run"]);
    assert_eq!(format!("{code:?}"), format!("{:?}", std::process::ExitCode::SUCCESS), "the fixture run");
    assert!(
        found(&[&bench_dir.join("results")], &|n| n == "samples.csv").len() == 1,
        "the results were written"
    );

    // no arm target, no driver build, in the fixture or the shared target
    let driver = crate::bench_gen::driver_name(&bench_dir);
    let crates = [arms[0].clone(), arms[1].clone(), driver.clone(), driver.replace('-', "_")];
    // builds only: not the arms' source directories, nor the fingerprint directory
    // of a cdylib arm, whose hash is in no file name and which is a few small files
    let left = found(&[&bench_dir.join("target"), &shared], &|n| is_a_build_of(n, &crates));
    let left: Vec<_> = left
        .iter()
        .filter(|p| !p.components().any(|c| c.as_os_str() == ".fingerprint" || c.as_os_str() == "mockspace-bench-arms"))
        .collect();
    assert!(left.is_empty(), "arm or driver builds left behind: {left:#?}");
    assert!(!bench_dir.join("target").join("mock-arms").exists(), "the per-arm target stays");
    assert!(!own_target.join("mockspace-bench").join("target").exists(), "the driver's own target stays");

    // the shared dependency: every copy of it, in the fixture and the shared
    // target, new to this run. One compile means one new copy at most; the old
    // layout made one for each arm target and one for the driver's target.
    let after = bench_core_copies(&[root, &shared]);
    let new_copies = after.saturating_sub(before);
    assert!(new_copies <= 1, "{new_copies} copies of the shared dependency were compiled by one run");
}

#[test]
fn the_shared_target_is_the_mock_directorys_own_unless_the_machine_chose_one() {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    assert_eq!(shared_target(tmp.path(), Some(OsStr::new("/shared"))), None);
    assert!(!tmp.path().join("target").exists(), "declining creates nothing");
    assert_eq!(shared_target(tmp.path(), None), Some(tmp.path().join("target")));
}

#[test]
fn the_keep_flag_is_taken_off_the_driver_args_before_the_separator_only() {
    assert_eq!(take_keep_flag(&["--seed", "3", KEEP_BUILDS]), (vec!["--seed", "3"], true));
    assert_eq!(take_keep_flag(&["--seed", "3"]), (vec!["--seed", "3"], false));
    assert_eq!(take_keep_flag(&["--", KEEP_BUILDS]).1, false, "after `--` it is not ours");
}

/// A package of its own workspace, `kind` the list of its `[lib]` crate types, in `dir`.
fn crate_at(dir: &Path, name: &str, version: &str, kind: &str, deps: &str) -> PathBuf {
    write(
        &dir.join("Cargo.toml"),
        &format!(
            "[workspace]\n\n[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2021\"\n\
             publish = false\n\n[lib]\ncrate-type = [{kind}]\n\n[dependencies]\n{deps}"
        ),
    );
    write(&dir.join("src").join("lib.rs"), "pub fn f() {}\n");
    dir.join("Cargo.toml")
}

/// Build `manifest` into `target` the way the run does, and return cargo's output.
fn cargo_build(manifest: &Path, target: &Path) -> Vec<u8> {
    let out = std::process::Command::new("cargo")
        .args(["build", "--release", "--offline", "--message-format=json-render-diagnostics"])
        .arg("--target-dir")
        .arg(target)
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    out.stdout
}

fn builds(tmp: &Path, keep: bool) -> Builds {
    Builds::new(&tmp.join("mock").join("benches"), keep)
}

/// Entries of `dir` whose names satisfy `pred`.
fn entries(dir: &Path, pred: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| pred(n))
        .collect();
    v.sort();
    v
}

/// A binary package of its own workspace, in `dir`.
fn bin_at(dir: &Path, name: &str) -> PathBuf {
    write(
        &dir.join("Cargo.toml"),
        &format!("[workspace]\n\n[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n"),
    );
    write(&dir.join("src").join("main.rs"), "fn main() {}\n");
    dir.join("Cargo.toml")
}

/// A dependency named `a`, as a registry crate reads (`src/lib.rs`), built in the
/// shared target, then the package `make` writes, also named `a`, built and
/// swept as an arm or the driver is. Returns the scratch guard and the target's
/// `release` directory, after the sweep.
fn sweep_beside_a_same_named_dependency(make: &dyn Fn(&Path) -> PathBuf) -> (tempfile::TempDir, PathBuf) {
    let guard = mockspace_manifest::scratch::tempdir().unwrap();
    let tmp = guard.path().to_path_buf();
    let target = tmp.join("target");
    let release = target.join("release");
    let dep = crate_at(&tmp.join("registry").join("a-1.0.0"), "a", "1.0.0", "\"rlib\"", "");
    let user = crate_at(
        &tmp.join("user"),
        "user",
        "0.0.0",
        "\"rlib\"",
        &format!("a = {{ path = {:?} }}\n", dep.parent().unwrap()),
    );
    let ours = make(&tmp.join("ours"));
    cargo_build(&user, &target);
    let mut run = builds(&tmp, false);
    run.record(&ours, &cargo_build(&ours, &target));
    let a = |n: &str| n.starts_with("a-");
    assert_eq!(entries(&release.join(".fingerprint"), &a).len(), 2, "the package's and the dependency's");
    drop(run);
    (guard, release)
}

/// What a sweep removes is the package it built, and a dependency of the same
/// crate name stays whatever source it reads: `src/lib.rs`, like every arm.
/// The arm is a cdylib, which has no hash in its file names, so its fingerprint
/// directory is the one thing left of it: the same directory serves every arm
/// of that name, whichever tree built it.
#[test]
fn a_sweep_removes_a_hashless_arm_and_leaves_a_dependency_of_the_same_crate_name() {
    let (_guard, release) =
        sweep_beside_a_same_named_dependency(&|dir| crate_at(dir, "a", "0.0.0", "\"cdylib\"", ""));
    assert!(!release.join("liba.so").exists(), "the arm's uplifted file");
    assert!(!release.join("deps").join("liba.so").exists(), "the arm's own file");
    assert!(!release.join("deps").join("a.d").exists(), "the arm's record");
    let deps = entries(&release.join("deps"), &|n| n.starts_with("a-") || n.starts_with("liba-"));
    assert_eq!(deps.len(), 3, "the dependency's record, rlib and rmeta stay: {deps:?}");
    let fingerprints = entries(&release.join(".fingerprint"), &|n| n.starts_with("a-"));
    assert_eq!(fingerprints.len(), 2, "the arm's fingerprint directory stays and so does the dependency's: {fingerprints:?}");
}

/// A binary's file names carry the metadata hash, so the files of its stem and
/// its fingerprint directory go by that hash, and the dependency's, of the same
/// crate name, stay.
#[test]
fn a_sweep_removes_a_hashed_binary_by_its_hash_and_leaves_a_dependency_of_the_same_crate_name() {
    let (_guard, release) = sweep_beside_a_same_named_dependency(&|dir| bin_at(dir, "a"));
    assert!(!release.join("a").exists(), "the binary's uplifted file");
    let fingerprints = entries(&release.join(".fingerprint"), &|n| n.starts_with("a-"));
    assert_eq!(fingerprints.len(), 1, "the binary's fingerprint goes and the dependency's stays: {fingerprints:?}");
    let deps = entries(&release.join("deps"), &|n| n.starts_with("a-") || n.starts_with("liba-"));
    assert_eq!(deps.len(), 3, "only the dependency's record, rlib and rmeta stay: {deps:?}");
    let record = entries(&release.join("deps"), &|n| n.starts_with("a-") && n.ends_with(".d"));
    let text = std::fs::read_to_string(release.join("deps").join(&record[0])).unwrap();
    assert!(text.contains("a-1.0.0"), "the survivor is the dependency's: {text}");
}

/// A cdylib has no hash in its name, so a build of another tree, or an earlier
/// kept run, owns `release/lib<arm>.so` once it has written it. A sweep that did
/// not write what is there leaves it.
#[test]
fn a_sweep_leaves_the_file_a_build_outside_the_run_replaced() {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let target = tmp.path().join("target");
    let release = target.join("release");
    let arm = crate_at(&tmp.path().join("arm"), "a", "0.0.0", "\"cdylib\"", "");
    let mut run = builds(tmp.path(), false);
    run.record(&arm, &cargo_build(&arm, &target));
    // another tree's arm of the same name, built after this one
    for f in [release.join("liba.so"), release.join("deps").join("liba.so")] {
        std::fs::remove_file(&f).unwrap();
        write(&f, "another tree's arm");
    }
    drop(run);
    assert_eq!(std::fs::read_to_string(release.join("liba.so")).unwrap(), "another tree's arm");
    assert!(release.join("deps").join("liba.so").is_file());
}

/// Two arms of one library name in one run write the one path between them, and
/// both are the run's: the sweep removes both.
#[test]
fn a_sweep_removes_every_arm_of_one_library_name_the_run_built() {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let target = tmp.path().join("target");
    let release = target.join("release");
    let first = crate_at(&tmp.path().join("one").join("a"), "a", "0.0.0", "\"cdylib\"", "");
    let second = crate_at(&tmp.path().join("two").join("a"), "a", "0.0.0", "\"cdylib\"", "");
    let mut run = builds(tmp.path(), false);
    run.record(&first, &cargo_build(&first, &target));
    run.record(&second, &cargo_build(&second, &target));
    assert!(release.join("liba.so").is_file());
    drop(run);
    assert!(!release.join("liba.so").exists());
    assert!(!release.join("deps").join("liba.so").exists());
}

/// `--keep-builds` leaves the builds.
#[test]
fn keeping_the_builds_removes_nothing() {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let target = tmp.path().join("target");
    let arm = crate_at(&tmp.path().join("arm"), "a", "0.0.0", "\"cdylib\"", "");
    let mut run = builds(tmp.path(), true);
    run.record(&arm, &cargo_build(&arm, &target));
    drop(run);
    assert!(target.join("release").join("liba.so").is_file());
}
