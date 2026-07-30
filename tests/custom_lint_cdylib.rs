//! End-to-end validation of the runtime custom-lint cdylib path: generate the
//! cdylib from a repo's `mock/lints/*.rs`, build it against the real
//! `mockspace-lint-rules`, dlopen it, and confirm a real `Box<dyn Lint>`
//! crosses the boundary and dispatches.
//!
//! `#[ignore]` because it runs a `cargo build` (slow, and needs the toolchain);
//! run explicitly with `cargo test -p mockspace --test custom_lint_cdylib -- --ignored`.
//! Catalogue: this guards the dissolve-proxy deal-breaker (custom lints must
//! work through the shared engine). Tracked with the launcher work.

use std::fs;

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn custom_lint_loads_and_dispatches_across_cdylib() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = tmp.path().join("mock");
    let lints = mock.join("lints");
    fs::create_dir_all(&lints).unwrap();
    fs::write(mock.join("mockspace.toml"), "project_name = \"probe\"\n").unwrap();

    // a real lint using the same imports the consumer lints use.
    fs::write(
        lints.join("trivial.rs"),
        r#"use mockspace::{Lint, LintContext, LintError};
pub fn lint() -> Box<dyn Lint> { Box::new(Trivial) }
struct Trivial;
impl Lint for Trivial {
    fn name(&self) -> &'static str { "trivial-cdylib-probe" }
    fn check(&self, _ctx: &LintContext) -> Vec<LintError> { Vec::new() }
}
"#,
    )
    .unwrap();

    let cfg = mockspace::config::Config::from_dir(&mock);
    // build the cdylib's mockspace-lint-rules from the workspace copy, renamed
    // to `mockspace` so the lint file compiles unchanged.
    let lint_rules = concat!(env!("CARGO_MANIFEST_DIR"), "/lint-rules");
    let dep = format!("{{ package = \"mockspace-lint-rules\", path = \"{lint_rules}\" }}");

    let loaded = mockspace::custom_lints::load(&cfg, &mock.join("mockspace.toml"), &dep)
        .expect("load ok")
        .expect("has custom lints");

    assert_eq!(loaded.pack.crate_lints.len(), 1);
    assert!(loaded.pack.workspace_lints.is_empty());
    assert!(loaded.pack.repo_lints.is_empty());
    assert!(loaded.pack.message_lints.is_empty());
    // dispatch across the cdylib boundary.
    assert_eq!(loaded.pack.crate_lints[0].name(), "trivial-cdylib-probe");
}
