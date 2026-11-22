//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

fn manifest(text: &str) -> BenchManifest {
    // parse through the harness loader so the test goes through
    // the same strict-key path a consumer does. The directory is
    // unique per call: parallel tests sharing a constant would
    // otherwise race on create/remove.
    let dir_guard = mockspace_manifest::scratch::tempdir().unwrap();
    let dir = dir_guard.path().to_path_buf();
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("bench.toml");
    std::fs::write(&p, text).unwrap();
    let m = BenchManifest::load(&p).expect("manifest parses");
    m
}

const TWO_BENCHES: &str = r#"
    [bench.a]
    title = "A"
    workload = "default"
    arms = ["x"]
    points = [64, 256]

    [bench.b]
    title = "B"
    workload = "realistic"
    arms = ["y"]
    points = [256, 4096]
"#;

#[test]
fn dispatch_points_default_to_the_union_of_every_bench() {
    let m = manifest(TWO_BENCHES);
    assert_eq!(dispatch_points(&m), vec![64, 256, 4096]);
}

#[test]
fn a_declared_dispatch_list_overrides_the_union() {
    let m = manifest(&format!("{TWO_BENCHES}\n[dispatch]\npoints = [64]\n"));
    assert_eq!(dispatch_points(&m), vec![64]);
}

#[test]
fn the_generated_main_carries_the_dispatch_and_the_default_hooks() {
    let m = manifest(TWO_BENCHES);
    let src = driver_main_source(&m, None).unwrap();
    assert!(src.contains("byte_routine_dispatch!(out = 8, sizes = [64, 256, 4096])"));
    assert!(src.contains("Hooks::default()"));
    assert!(!src.contains("consumer_hooks"));
}

#[test]
fn a_hooks_lib_is_included_by_path_and_called() {
    let m = manifest(TWO_BENCHES);
    let src = driver_main_source(&m, Some(Path::new("/tree/src/lib.rs"))).unwrap();
    assert!(src.contains("#[path = \"/tree/src/lib.rs\"]"));
    assert!(src.contains("consumer_hooks::hooks()"));
}

#[test]
fn declared_workloads_generate_and_builtins_fill_the_rest() {
    let m = manifest(&format!(
        "{TWO_BENCHES}\n[workload.mine]\nstages = [\"algo_call\", \"scalar_work 7\"]\n"
    ));
    let src = workload_fn_source(&m).unwrap();
    assert!(src.contains("harness::scalar_work(7)"));
    // both builtins are present for the benches that use them
    assert!(src.contains("\"realistic\" =>"));
    assert!(src.contains("harness::heavy_memory(384)"));
}

#[test]
fn an_unknown_stage_is_refused_naming_the_builtins() {
    let m = manifest(&format!(
        "{TWO_BENCHES}\n[workload.mine]\nstages = [\"warp_drive 9\"]\n"
    ));
    let err = workload_fn_source(&m).unwrap_err();
    assert!(err.contains("warp_drive"), "{err}");
    assert!(err.contains("algo_call"), "names the builtins: {err}");
}

#[test]
fn a_wrong_stage_arity_is_refused_with_the_expected_shape() {
    let m = manifest(&format!(
        "{TWO_BENCHES}\n[workload.mine]\nstages = [\"scalar_work\"]\n"
    ));
    let err = workload_fn_source(&m).unwrap_err();
    assert!(err.contains("needs an integer argument"), "{err}");
    let m = manifest(&format!(
        "{TWO_BENCHES}\n[workload.mine]\nstages = [\"algo_call 3\"]\n"
    ));
    let err = workload_fn_source(&m).unwrap_err();
    assert!(err.contains("takes no argument"), "{err}");
}

#[test]
fn a_bench_using_an_undeclared_workload_is_refused_at_generation() {
    let m = manifest(
        r#"
        [bench.a]
        title = "A"
        workload = "nonexistent"
        arms = ["x"]
        points = [64]
    "#,
    );
    let err = workload_fn_source(&m).unwrap_err();
    assert!(err.contains("`a`"), "names the bench: {err}");
    assert!(err.contains("nonexistent"), "{err}");
    assert!(err.contains("default"), "lists known programs: {err}");
}

#[test]
fn the_build_section_overrides_the_generated_dep_spec() {
    let m = manifest(&format!(
        "{TWO_BENCHES}\n[build]\nmockspace = '{{ git = \"x\", rev = \"abc\" }}'\n"
    ));
    assert_eq!(mockspace_dep(&m), "{ git = \"x\", rev = \"abc\" }");
    let plain = manifest(TWO_BENCHES);
    assert_eq!(mockspace_dep(&plain), DEFAULT_MOCKSPACE_DEP);
}

#[test]
fn generated_manifests_carry_the_workspace_header_and_the_feature() {
    let arm = ArmSource {
        bench:        "hash".into(),
        arm:          "fnv-fast".into(),
        dir:          PathBuf::from("/tree/hash/arms/fnv-fast"),
        has_manifest: false,
    };
    let toml = arm_cargo_toml(&arm, DEFAULT_MOCKSPACE_DEP, &[]).unwrap();
    assert!(toml.starts_with("# GENERATED"));
    assert!(
        toml.contains("[workspace]"),
        "the header that keeps an outer workspace from capturing the crate"
    );
    assert!(
        toml.contains("name = \"fnv_fast\""),
        "dashes become underscores"
    );
    assert!(toml.contains("crate-type = [\"cdylib\"]"));
    assert!(
        toml.contains("features = [\"std\"]"),
        "bench-core needs std"
    );
    let driver = driver_cargo_toml(DEFAULT_MOCKSPACE_DEP, &[], "mockspace-bench-driver-0").unwrap();
    assert!(driver.contains("[workspace]"));
    assert!(driver.contains("mockspace-bench-harness"));
}

#[test]
fn a_cdylib_under_support_is_refused_naming_the_rule() {
    let dir_guard = mockspace_manifest::scratch::tempdir().unwrap();
    let dir = dir_guard.path().to_path_buf();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"kit\"\n[lib]\ncrate-type = [\"cdylib\"]\n",
    )
    .unwrap();
    let src = SupportSource {
        bench: None,
        name:  "kit".into(),
        dir:   dir.clone(),
    };
    let err = support_package_name(&src).unwrap_err();
    assert!(err.contains("belongs under arms/"), "{err}");
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"kit\"\n").unwrap();
    assert_eq!(support_package_name(&src).unwrap(), "kit");
}

#[test]
fn a_path_dep_spec_expands_per_crate_and_a_git_spec_applies_verbatim() {
    let path_spec = "{ path = \"/repo/mockspace\" }";
    assert_eq!(
        dep_for_crate(path_spec, "bench-core"),
        "{ path = \"/repo/mockspace/bench-core\" }"
    );
    assert_eq!(
        dep_for_crate(DEFAULT_MOCKSPACE_DEP, "bench-core"),
        DEFAULT_MOCKSPACE_DEP,
        "a git spec resolves by crate name and must not be rewritten"
    );
}

#[test]
fn write_if_changed_is_idempotent() {
    let dir_guard = mockspace_manifest::scratch::tempdir().unwrap();
    let dir = dir_guard.path().to_path_buf();
    let p = dir.join("x/y.rs");
    write_if_changed(&p, "one").unwrap();
    let t1 = std::fs::metadata(&p).unwrap().modified().unwrap();
    write_if_changed(&p, "one").unwrap();
    let t2 = std::fs::metadata(&p).unwrap().modified().unwrap();
    assert_eq!(t1, t2, "unchanged content must not rewrite the file");
    write_if_changed(&p, "two").unwrap();
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "two");
}

/// Two trees' drivers never share a name, so building one into a target
/// directory both use cannot replace the other's while it runs; one tree's
/// name is stable across runs.
#[test]
fn every_tree_has_a_driver_name_of_its_own() {
    let a = mockspace_manifest::scratch::tempdir().unwrap();
    let b = mockspace_manifest::scratch::tempdir().unwrap();
    assert_ne!(driver_name(a.path()), driver_name(b.path()));
    assert_eq!(driver_name(a.path()), driver_name(a.path()));
    assert!(driver_name(a.path()).starts_with("mockspace-bench-driver-"));
    let toml = driver_cargo_toml(DEFAULT_MOCKSPACE_DEP, &[], &driver_name(a.path())).unwrap();
    assert!(toml.contains(&format!("name = \"{}\"", driver_name(a.path()))), "{toml}");
}

/// While one run holds a tree's lock a second cannot take it, and it can once
/// the first lets go.
#[test]
fn a_second_run_of_a_tree_waits_for_the_first() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let held = lock_tree(dir.path()).unwrap();
    let other = std::fs::OpenOptions::new().write(true).open(dir.path().join(".run-lock")).unwrap();
    assert!(matches!(other.try_lock(), Err(std::fs::TryLockError::WouldBlock)));
    drop(held);
    assert!(other.try_lock().is_ok());
}

/// A Windows path in the `[build] mockspace` spec is escaped TOML, and is read
/// back as the path it names before a crate's directory is joined to it, then
/// written escaped again; slicing the text between the quotes doubled every
/// backslash on each pass.
#[test]
fn a_windows_path_spec_expands_to_the_crate_under_that_path() {
    let spec = format!(
        "{{ path = {} }}",
        mockspace_manifest::toml_text::basic_string(r"C:\repo\mockspace")
    );
    let expanded = dep_for_crate(&spec, "bench-core");
    let doc: toml_edit::DocumentMut = format!("x = {expanded}").parse().unwrap();
    let path = doc["x"]["path"].as_str().unwrap();
    assert_eq!(
        path,
        std::path::Path::new(r"C:\repo\mockspace")
            .join("bench-core")
            .display()
            .to_string()
    );
}
