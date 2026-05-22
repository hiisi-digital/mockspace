//! Shared helpers for the e2e golden-result test files.
//!
//! Integration test files under `tests/` each compile as a separate
//! cargo test binary, so shared code lives in a `common/mod.rs`
//! submodule that each file includes via `mod common;`. This is the
//! idiomatic Rust pattern.
//!
//! See `e2e_explain.rs` for the inaugural use and `e2e_install.rs`
//! for the filesystem-tree variant.

use std::path::{Path, PathBuf};

/// Resolve the path to a checked-in golden file under
/// `<crate>/tests/goldens/<name>.golden`. Centralised so consumers
/// don't repeat the `CARGO_MANIFEST_DIR` join idiom.
pub fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join(format!("{name}.golden"))
}

/// Compare `actual` against the checked-in golden at `<name>.golden`.
/// On `MOCKSPACE_UPDATE_GOLDENS=1`, writes `actual` to the golden
/// path (creating the parent directory if needed) and passes.
/// Otherwise reads the golden and asserts byte-equality, failing
/// with a diff hint pointing at the regenerate knob.
pub fn assert_matches_golden(name: &str, actual: &str) {
    let path = golden_path(name);
    let update = std::env::var("MOCKSPACE_UPDATE_GOLDENS")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);

    if update {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create goldens directory");
        }
        std::fs::write(&path, actual).expect("write golden");
        return;
    }

    let expected = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => panic!(
            "golden `{name}` missing at {}: {e}.\n\
             To create it, rerun with MOCKSPACE_UPDATE_GOLDENS=1",
            path.display()
        ),
    };

    if expected != actual {
        panic!(
            "golden `{name}` does not match.\n\
             Expected (from {}):\n{expected}\n\
             ---\n\
             Actual:\n{actual}\n\
             ---\n\
             To accept the new output, rerun with MOCKSPACE_UPDATE_GOLDENS=1",
            path.display()
        );
    }
}
