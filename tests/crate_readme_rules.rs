//! Each crate's README becomes a path-scoped agent rule.
//!
//! The per-crate doc set splits by depth: README is the short summary, DESIGN
//! is the long form. Deriving the rule from the README means the summary has
//! exactly one home, so anyone working inside a crate picks it up implicitly
//! and it can never drift from a hand-written second copy.

use std::fs;
use std::path::Path;

use mockspace::config::Config;
use mockspace::render_agent;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

/// Build a fixture repo with two crates, only one of which has a README.
fn fixture(root: &Path) -> Config {
    fs::create_dir_all(root.join(".git")).unwrap();
    let mock = root.join("mock");
    write(
        &mock.join("mockspace.toml"),
        "project_name = \"fixture-proj\"\ncrate_prefix = \"fixture\"\n",
    );

    write(&mock.join("crates/fixture-one/src/lib.rs"), "//! one.\n");
    write(
        &mock.join("crates/fixture-one/README.md.tmpl"),
        "# fixture-one\n\nThe gateway crate of {{project_name}}, under `{{mock_dir}}/`.\n",
    );

    // No README: this crate must not produce a rule.
    write(&mock.join("crates/fixture-two/src/lib.rs"), "//! two.\n");

    // An empty README is not a summary. Emitting a rule here would produce a
    // file that is nothing but bookends.
    write(
        &mock.join("crates/fixture-three/src/lib.rs"),
        "//! three.\n",
    );
    write(&mock.join("crates/fixture-three/README.md.tmpl"), "\n  \n");

    Config::from_dir(&mock)
}

fn render(cfg: &Config) {
    let crates = mockspace::parse::discover_crates(&cfg.crates_dir, &cfg.crate_prefix);
    render_agent::generate_agent_rules(&crates, cfg, &Default::default());
}

#[test]
fn crate_readme_becomes_a_scoped_rule() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let cfg = fixture(root);

    render(&cfg);

    let rule = root.join(".claude/rules/crate-readme-fixture-one.md");
    let body = fs::read_to_string(&rule).expect("the crate with a README gets a rule");

    // Assert the frontmatter's shape, not merely that the glob appears
    // somewhere: a rule whose scope is not parseable frontmatter is a rule
    // that silently loads everywhere.
    assert!(
        body.starts_with("---\npaths:\n  - \"mock/crates/fixture-one/**\"\n---"),
        "the rule must open with paths frontmatter scoping it to its own crate, got:\n{body}"
    );
    assert!(
        body.contains("The gateway crate of fixture-proj, under `mock/`."),
        "the README's placeholders must expand, got:\n{body}"
    );
    assert!(
        !body.contains("{{"),
        "no placeholder may survive into a rule, got:\n{body}"
    );

    assert!(
        root.join(".github/instructions/crate-readme-fixture-one.instructions.md")
            .exists(),
        "the copilot counterpart is written too"
    );

    assert!(
        !root
            .join(".claude/rules/crate-readme-fixture-two.md")
            .exists(),
        "a crate with no README must not produce a rule"
    );
    assert!(
        !root
            .join(".claude/rules/crate-readme-fixture-three.md")
            .exists(),
        "a whitespace-only README must not produce a bookends-only rule"
    );
}

/// Emptying the crates directory must still sweep.
///
/// Guard rather than driver: the interesting variant is a `crates/` that is
/// gone entirely, and that one cannot be reached through the render entry
/// point today because `discover_crates` panics on it first. The generator
/// handles it anyway rather than relying on a caller to panic on its behalf.
#[test]
fn emptying_the_crates_dir_still_sweeps() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let cfg = fixture(root);

    render(&cfg);
    let rule = root.join(".claude/rules/crate-readme-fixture-one.md");
    assert!(rule.exists(), "precondition: the rule was written");

    for entry in fs::read_dir(&cfg.crates_dir).unwrap() {
        fs::remove_dir_all(entry.unwrap().path()).unwrap();
    }
    render(&cfg);

    assert!(
        !rule.exists(),
        "removing the last crate must sweep its rule"
    );
}

#[test]
fn a_removed_crate_loses_its_rule() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let cfg = fixture(root);

    render(&cfg);
    let rule = root.join(".claude/rules/crate-readme-fixture-one.md");
    let copilot = root.join(".github/instructions/crate-readme-fixture-one.instructions.md");
    assert!(rule.exists(), "precondition: the rule was written");

    // Renaming or deleting a crate must not leave its rule behind, because
    // the render pipeline is otherwise purely additive.
    fs::remove_dir_all(cfg.crates_dir.join("fixture-one")).unwrap();
    render(&cfg);

    assert!(!rule.exists(), "the orphaned rule must be swept");
    assert!(
        !copilot.exists(),
        "the orphaned copilot rule must be swept too"
    );
}

#[test]
fn a_consumer_rule_is_never_swept() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let cfg = fixture(root);

    render(&cfg);

    // The sweep keys on the generated prefix. A consumer-authored rule sitting
    // in the same directory must be untouched even when it names a crate.
    let handwritten = root.join(".claude/rules/fixture-one-conventions.md");
    write(&handwritten, "hand-authored, not generated\n");
    fs::remove_dir_all(cfg.crates_dir.join("fixture-one")).unwrap();
    render(&cfg);

    assert_eq!(
        fs::read_to_string(&handwritten).unwrap(),
        "hand-authored, not generated\n",
        "the sweep must only ever touch files it generated"
    );
}
