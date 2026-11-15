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

/// The generated lint crate's package name, as the build is handed it.
const OWN: &str = "mockspace-lints-40d923802ebb4e36";

// out of a live consumer commit: the engine crates are path dependencies of
// the lint crate, and each build script's warning was printed twice per commit.
#[test]
fn a_dependency_build_script_warning_is_held_back() {
    for line in [
        "warning: engine-hash@0.0.0: flags-build chose rustc flags for this crate and cargo names no workspace wrapper to append them, so they are dropped",
        // a package name with an underscore
        "warning: engine_hash@0.0.0: a note",
        // a pre-release version
        "warning: engine-hash@1.0.0-rc.1: a note",
        // a message that itself carries `: `
        "warning: engine-hash@0.1.0: note: the flags were: -C target-cpu=native",
        // a dependency whose name is a prefix of the lint crate's own
        "warning: mockspace@0.0.2: a note",
        "warning: mock@1.0.0: a note",
        // the same line in colour, as cargo writes it to a terminal
        "\x1b[1m\x1b[33mwarning\x1b[0m\x1b[1m:\x1b[0m engine-hash@0.0.0: a note",
    ] {
        assert!(is_dependency_build_script_warning(line, OWN), "{line}");
    }
}

#[test]
fn the_lint_crates_own_warnings_and_every_other_line_pass() {
    for line in [
        "warning: mockspace-lints-40d923802ebb4e36@0.0.0: the lint crate's own build script",
        "warning: mockspace_lints_40d923802ebb4e36@0.0.0: the same, spelled as a crate",
        "warning: unused variable: `x`",
        "warning: `mockspace-lints` (lib) generated 1 warning",
        "error: could not compile `engine-hash` (lib) due to 1 previous error",
        "warning: profile package spec `x@1` in profile `release` did not match any packages",
        "   Compiling engine-hash v0.0.0 (/p/crates/engine-hash)",
        "\x1b[1m\x1b[33mwarning\x1b[0m\x1b[1m: unused variable: `x`\x1b[0m",
    ] {
        assert!(!is_dependency_build_script_warning(line, OWN), "{line}");
    }
}

/// The relay passes every byte of a line it keeps, and a line that is not
/// UTF-8 neither stops it nor is lost: the lines after it still come through.
#[test]
fn the_relay_passes_bytes_through_and_survives_a_line_that_is_not_text() {
    let input: &[u8] = b"first\n\xff\xfe not text\nwarning: engine-hash@0.0.0: held back\nlast\n";
    let mut out = Vec::new();
    relay_stderr(input, &mut out, OWN).unwrap();
    assert_eq!(out, b"first\n\xff\xfe not text\nlast\n".to_vec());
}

/// `--color always` reaches cargo only where the output it relays goes to a
/// terminal, so a log or a pipe gets no escape codes.
#[test]
fn colour_is_asked_for_only_on_a_terminal() {
    assert_eq!(colour_args(true), ["--color", "always"]);
    assert!(colour_args(false).is_empty());
}
