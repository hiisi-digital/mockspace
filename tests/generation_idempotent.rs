//! Regenerating docs on an unchanged source must not churn the output.
//!
//! Every generated file carries a `Generated at:` timestamp line, so a
//! naive rewrite dirties the tree on every run even when nothing changed.
//! `write_generated` skips a write whose only difference is that line.
//! This exercises the real generation path end to end (no proxy), twice,
//! and asserts the second run leaves the file byte-identical.

use std::fs;
use std::path::Path;

use mockspace::config::Config;
use mockspace::render_design;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

#[test]
fn per_crate_doc_regeneration_is_timestamp_stable() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Minimal mock workspace: a repo root with .git, a mock/ dir carrying
    // mockspace.toml, and one crate with a DESIGN.md.tmpl to render.
    fs::create_dir_all(root.join(".git")).unwrap();
    let mock = root.join("mock");
    write(&mock.join("mockspace.toml"), "project_name = \"fixture\"\ncrate_prefix = \"fixture\"\n");
    write(
        &mock.join("crates/fixture-one/src/lib.rs"),
        "//! fixture-one.\n",
    );
    write(
        &mock.join("crates/fixture-one/DESIGN.md.tmpl"),
        "# fixture-one\n\nStable body text that does not change between runs.\n",
    );

    let cfg = Config::from_dir(&mock);
    // The entry flow creates docs/ before generating into it; mirror that.
    fs::create_dir_all(&cfg.docs_dir).unwrap();
    let out = cfg.docs_dir.join("FIXTURE_ONE_OVERVIEW.md");

    // First generation creates the file.
    render_design::generate_per_crate_docs(&cfg);
    assert!(out.exists(), "first run must create the overview file");
    let after_first = fs::read_to_string(&out).unwrap();
    assert!(
        after_first.contains("Generated at:"),
        "the generated file carries a timestamp line"
    );

    // Force the timestamp to differ on the next run: rewrite the header's
    // timestamp to an obviously-old value, so a naive regenerate would
    // change it back and dirty the file.
    let staled = after_first.replacen(
        after_first
            .lines()
            .find(|l| l.contains("Generated at:"))
            .unwrap(),
        "  Generated at: 2000-01-01T00:00:00Z",
        1,
    );
    fs::write(&out, &staled).unwrap();

    // Second generation: identical body, new real timestamp. It must skip
    // the write and leave the staled timestamp in place, proving the
    // timestamp alone never triggers a rewrite.
    render_design::generate_per_crate_docs(&cfg);
    let after_second = fs::read_to_string(&out).unwrap();
    assert_eq!(
        after_second, staled,
        "a timestamp-only difference must not rewrite the file"
    );

    // A real body change must still write.
    write(
        &mock.join("crates/fixture-one/DESIGN.md.tmpl"),
        "# fixture-one\n\nBody text that HAS changed.\n",
    );
    render_design::generate_per_crate_docs(&cfg);
    let after_change = fs::read_to_string(&out).unwrap();
    assert!(
        after_change.contains("HAS changed"),
        "a real content change must be written"
    );
    assert_ne!(after_change, staled, "the changed body must land on disk");
}
