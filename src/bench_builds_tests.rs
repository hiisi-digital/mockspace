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

fn is_a_bench_build(name: &str) -> bool {
    name.starts_with("mockspace-bench-driver-")
        || name.starts_with("mockspace_bench_driver_")
        || name == "liba.so"
        || name == "libb.so"
        || name.starts_with("liba-")
        || name.starts_with("libb-")
        || name.starts_with("a-")
        || name.starts_with("b-")
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
    write(
        &bench_dir.join("pair").join("bench.toml"),
        "title = \"Pair\"\nworkload = \"default\"\narms = [\"a\", \"b\"]\npoints = [64]\nmaster_seed = 7\n",
    );
    for arm in ["a", "b"] {
        write(
            &bench_dir.join("pair").join("arms").join(arm).join("src").join("lib.rs"),
            &ARM_LIB.replace("NAME", arm),
        );
    }

    let own_target = mock_dir.join("target");
    let shared = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or(own_target.clone());
    let before = found(&[&shared], &is_bench_core_rlib).len();

    let cfg = crate::config::Config::from_dir(&mock_dir);
    let code = crate::bench::cmd(&cfg, &["run"]);
    assert_eq!(format!("{code:?}"), format!("{:?}", std::process::ExitCode::SUCCESS), "the fixture run");
    assert!(
        found(&[&bench_dir.join("results")], &|n| n == "samples.csv").len() == 1,
        "the results were written"
    );

    // no arm target, no driver build, in the fixture or the shared target
    let left = found(&[root, &shared], &is_a_bench_build);
    let left: Vec<_> = left.iter().filter(|p| !p.starts_with(bench_dir.join("results"))).collect();
    assert!(left.is_empty(), "arm or driver builds left behind: {left:#?}");
    assert!(!bench_dir.join("target").join("mock-arms").exists(), "the per-arm target stays");
    assert!(!own_target.join("mockspace-bench").join("target").exists(), "the driver's own target stays");

    // the shared dependency: every copy of it, in the fixture and the shared
    // target, new to this run. One compile means one new copy at most; the old
    // layout made one for each arm target and one for the driver's target.
    let after = found(&[root, &shared], &is_bench_core_rlib).len();
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

fn built(name: &str, src: &Path, files: Vec<PathBuf>) -> Built {
    Built {
        name: name.into(),
        sources: [src.display().to_string().into_bytes(), b"src/lib.rs".to_vec()],
        files,
    }
}

/// What a sweep removes is the package's own: a dependency of the same crate
/// name, whose dependency record names a source elsewhere, stays.
#[test]
fn a_sweep_removes_the_arms_files_and_leaves_a_dependency_of_the_same_name() {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let release = tmp.path().join("target").join("release");
    let arm_src = tmp.path().join("mock").join("pair").join("a").join("src").join("lib.rs");
    let other = "fedcba9876543210";
    let deps = release.join("deps");
    let fp = release.join(".fingerprint");
    // the arm, a cdylib named `a`: cargo keeps `deps/liba.so` and links it up
    write(&deps.join("liba.so"), "arm");
    std::fs::hard_link(deps.join("liba.so"), release.join("liba.so")).unwrap();
    write(&release.join("liba.d"), "arm");
    write(&deps.join("a.d"), &format!("deps/a.d: {}\n", arm_src.display()));
    write(&fp.join("a-0123456789abcdef").join("dep-lib-a"), &arm_src.display().to_string());
    // a dependency also named `a`, from elsewhere
    write(&deps.join(format!("liba-{other}.rlib")), "dependency");
    write(&deps.join(format!("liba-{other}.d")), &format!("deps/liba-{other}.d: /registry/a/src/main.rs\n"));
    write(&fp.join(format!("a-{other}")).join("dep-lib-a"), "/registry/a/other.rs");
    write(&release.join("libzed.so"), "someone else's");

    let mut builds = Builds::new(&tmp.path().join("mock"), false);
    builds.built.push(built("a", &arm_src, vec![release.join("liba.so")]));
    drop(builds);

    let left = found(&[tmp.path()], &|_| true);
    let left: Vec<String> = left
        .iter()
        .filter(|p| p.is_file())
        .map(|p| p.strip_prefix(tmp.path()).unwrap().display().to_string())
        .collect();
    assert_eq!(
        left,
        vec![
            format!("target/release/.fingerprint/a-{other}/dep-lib-a"),
            format!("target/release/deps/liba-{other}.d"),
            format!("target/release/deps/liba-{other}.rlib"),
            "target/release/libzed.so".to_string(),
        ]
    );
}

/// A binary keeps a hashed file in `deps/`, and its record names its source as
/// rustc wrote it, relative to the package directory.
#[test]
fn a_sweep_removes_a_hashed_binary_and_its_relative_source_record() {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let release = tmp.path().join("target").join("release");
    let src = tmp.path().join("gen").join("src").join("lib.rs");
    let deps = release.join("deps");
    let hash = "0123456789abcdef";
    write(&deps.join(format!("drv_x-{hash}")), "bin");
    std::fs::hard_link(deps.join(format!("drv_x-{hash}")), release.join("drv-x")).unwrap();
    write(&release.join("drv-x.d"), "x");
    write(&deps.join(format!("drv_x-{hash}.d")), "deps/drv_x.d: src/lib.rs\n");
    write(&release.join(".fingerprint").join(format!("drv-x-{hash}")).join("dep-bin-drv-x"), "src/lib.rs");
    let mut builds = Builds::new(&tmp.path().join("mock"), false);
    builds.built.push(built("drv-x", &src, vec![release.join("drv-x")]));
    drop(builds);
    let left: Vec<PathBuf> = found(&[&release], &|_| true).into_iter().filter(|p| p.is_file()).collect();
    assert!(left.is_empty(), "{left:#?}");
}

/// `--keep-builds` leaves the builds.
#[test]
fn keeping_the_builds_removes_nothing() {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let release = tmp.path().join("target").join("release");
    write(&release.join("libx.so"), "arm");
    let mut builds = Builds::new(&tmp.path().join("mock"), true);
    builds.built.push(built("x", &tmp.path().join("x"), vec![release.join("libx.so")]));
    drop(builds);
    assert!(release.join("libx.so").is_file());
}
