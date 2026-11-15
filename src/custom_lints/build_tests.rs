use std::path::PathBuf;

use super::*;

// one real `compiler-artifact` line, out of a live
// `cargo build --message-format json-render-diagnostics`. hand-written json
// here would only test that the test agrees with itself.
const ARTIFACT: &str = r#"{"reason":"compiler-artifact","package_id":"path+file:///p/mock/target/mockspace-lints#mockspace-lints-40d923802ebb4e36@0.0.0","manifest_path":"/p/mock/target/mockspace-lints/Cargo.toml","target":{"kind":["cdylib"],"crate_types":["cdylib"],"name":"mockspace_lints_40d923802ebb4e36","src_path":"/p/mock/target/mockspace-lints/src/lib.rs","edition":"2024","doc":false,"doctest":false,"test":true},"profile":{"opt_level":"3","debuginfo":0,"debug_assertions":false,"overflow_checks":false,"test":false},"features":[],"filenames":["/elsewhere/release/libmockspace_lints_40d923802ebb4e36.dylib"],"executable":null,"fresh":true}"#;

#[test]
fn the_artifact_path_comes_from_cargo_not_from_a_directory() {
    // the whole mechanism: the path is wherever cargo put it, which under
    // an inherited CARGO_TARGET_DIR is not under gen_dir at all.
    assert_eq!(
        cdylib_artifact(ARTIFACT, "mockspace-lints-40d923802ebb4e36"),
        Some(PathBuf::from(
            "/elsewhere/release/libmockspace_lints_40d923802ebb4e36.dylib"
        ))
    );
}

#[test]
fn another_projects_artifact_is_not_ours() {
    // several generated crates can share one target dir, which is why the
    // name is hashed per project in the first place.
    assert!(cdylib_artifact(ARTIFACT, "mockspace-lints-0000000000000000").is_none());
}

#[test]
fn nothing_but_a_cdylib_artifact_answers() {
    // controls. without these, a filter that always took the first
    // filename would pass every arm above.
    let lib = ARTIFACT.replace(r#""kind":["cdylib"]"#, r#""kind":["lib"]"#);
    assert!(cdylib_artifact(&lib, "mockspace-lints-40d923802ebb4e36").is_none());
    let other = ARTIFACT.replace("compiler-artifact", "build-script-executed");
    assert!(cdylib_artifact(&other, "mockspace-lints-40d923802ebb4e36").is_none());
    assert!(cdylib_artifact("not json at all", "mockspace-lints-40d923802ebb4e36").is_none());
    let rlib = ARTIFACT.replace(".dylib", ".rlib");
    assert!(cdylib_artifact(&rlib, "mockspace-lints-40d923802ebb4e36").is_none());
}

#[test]
fn the_build_is_given_where_things_are_and_nothing_that_changes_what_it_makes() {
    for kept in ["PATH", "HOME", "CARGO_HOME", "RUSTUP_TOOLCHAIN", "CARGO_TARGET_DIR", "CARGO_NET_OFFLINE"] {
        assert!(keeps_env(kept), "{kept}");
    }
    for dropped in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
        "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
        "CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS",
        "CARGO_BUILD_TARGET",
        "RUSTC_WRAPPER",
        "RUSTC",
        "GIT_DIR",
        "GIT_INDEX_FILE",
    ] {
        assert!(!keeps_env(dropped), "{dropped}");
    }
}

fn artifact(package_id: &str, kind: &str, fresh: bool) -> String {
    format!(
        r#"{{"reason":"compiler-artifact","package_id":"{package_id}","target":{{"kind":["{kind}"],"name":"t"}},"filenames":["/t/release/deps/t-1"],"fresh":{fresh}}}"#
    )
}

#[test]
fn a_unit_runs_in_the_tree_when_it_is_a_build_script_or_macro_and_its_package_is_a_path() {
    let unit = |id: &str, kind: &str| artifact_unit(&artifact(id, kind, true)).unwrap();
    assert!(unit("path+file:///r/tools/t#t@0.0.0", "custom-build").runs_in_tree());
    assert!(unit("path+file:///r/tools/t#t@0.0.0", "proc-macro").runs_in_tree());
    assert!(unit("t 0.0.0 (path+file:///r/tools/t)", "custom-build").runs_in_tree());
    assert!(!unit("path+file:///r/tools/t#t@0.0.0", "lib").runs_in_tree());
    assert!(!unit("registry+https://github.com/rust-lang/crates.io-index#t@1.0.0", "custom-build").runs_in_tree());
    assert_eq!(unit("path+file:///r/tools/t#t@0.0.0", "proc-macro").kind, UnitKind::ProcMacro);
    assert_eq!(unit("path+file:///r/tools/t#t@0.0.0", "custom-build").kind, UnitKind::BuildScript);
    assert!(unit("registry+x#t@1.0.0", "lib").fresh);
    assert!(artifact_unit(r#"{"reason":"build-script-executed"}"#).is_none());
}

// out of a live kaski commit: the engine crates are path dependencies of the
// lint crate, and each build script's warning was printed twice per commit.
#[test]
fn a_dependency_build_script_warning_is_held_back() {
    assert!(is_dependency_build_script_warning(
        "warning: kaski-engine-hash@0.0.0: hilavitkutin-build chose rustc flags for this crate and cargo names no workspace wrapper to append them, so they are dropped",
        "mockspace_lints_40d923802ebb4e36",
    ));
}

#[test]
fn the_lint_crates_own_warnings_and_every_other_line_pass() {
    let own = "mockspace_lints_40d923802ebb4e36";
    for line in [
        "warning: mockspace-lints@0.0.0: the lint crate's own build script",
        "warning: unused variable: `x`",
        "warning: `mockspace-lints` (lib) generated 1 warning",
        "error: could not compile `kaski-engine-hash` (lib) due to 1 previous error",
        "warning: profile package spec `x@1` in profile `release` did not match any packages",
        "   Compiling kaski-engine-hash v0.0.0 (/p/crates/kaski-engine-hash)",
    ] {
        assert!(!is_dependency_build_script_warning(line, own), "{line}");
    }
}
