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
    write(
        &mock.join("mockspace.toml"),
        "project_name = \"fixture\"\ncrate_prefix = \"fixture\"\n",
    );
    write(
        &mock.join("crates/fixture-one/src/lib.rs"),
        "//! fixture-one.\n",
    );
    write(
        &mock.join("crates/fixture-one/DESIGN.md.tmpl"),
        "# fixture-one\n\nStable body text that does not change between runs.\n",
    );

    let cfg = Config::from_dir(&mock);
    let crates = mockspace::parse::discover_crates(&cfg.crates_dir, &cfg.crate_prefix);
    let ph = render_design::Placeholders::compute(&crates, &cfg);
    render_design::ensure_docs_dir(&cfg);
    let out = cfg.docs_dir.join("ONE.md");

    // First generation creates the file.
    {
        let plan = mockspace::document::plan(&cfg, &crates);
        mockspace::document::render_all(&plan, &ph, &Default::default(), &cfg);
    }
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
    {
        let plan = mockspace::document::plan(&cfg, &crates);
        mockspace::document::render_all(&plan, &ph, &Default::default(), &cfg);
    }
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
    {
        let plan = mockspace::document::plan(&cfg, &crates);
        mockspace::document::render_all(&plan, &ph, &Default::default(), &cfg);
    }
    let after_change = fs::read_to_string(&out).unwrap();
    assert!(
        after_change.contains("HAS changed"),
        "a real content change must be written"
    );
    assert_ne!(after_change, staled, "the changed body must land on disk");
}

/// A passthrough template must get the same placeholder vocabulary the
/// design template gets.
///
/// The substitution used to run only for `DESIGN.md.tmpl`; every other
/// `*.md.tmpl` was copied verbatim, so a `{{project_name}}` in
/// `WORKFLOW.md.tmpl` reached the published doc literally.
///
/// Deliberately does NOT create `docs/`: the render path owns its output
/// directory, because a repo generating for the first time has none.
#[test]
fn passthrough_templates_expand_placeholders() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join(".git")).unwrap();
    let mock = root.join("mock");
    write(
        &mock.join("mockspace.toml"),
        "project_name = \"fixture-proj\"\ncrate_prefix = \"fixture\"\n",
    );
    write(
        &mock.join("crates/fixture-one/src/lib.rs"),
        "//! fixture-one.\npub struct Thing;\n",
    );
    // Exercise the computed members too, not only the two scalar ones.
    write(
        &mock.join("WORKFLOW.md.tmpl"),
        "# {{project_name}}. Workflow\n\n\
         Rounds live in `{{mock_dir}}/design_rounds/`.\n\n\
         Crates: {{crate_count}}\n\n\
         {{crate_layers}}\n{{deep_dives}}\n{{crate_summaries}}\n{{macros_table}}\n",
    );

    let cfg = Config::from_dir(&mock);
    let crates = mockspace::parse::discover_crates(&cfg.crates_dir, &cfg.crate_prefix);
    let ph = render_design::Placeholders::compute(&crates, &cfg);

    assert!(
        !cfg.docs_dir.exists(),
        "fixture must start with no docs/ so the render path owns creating it"
    );

    let registry = mockspace::registry::load_registry(&cfg.mock_dir, &cfg.registry_namespaces);
    let written = {
        let plan = mockspace::document::plan(&cfg, &Default::default());
        mockspace::document::render_all(&plan, &ph, &registry, &cfg)
    };
    assert_eq!(written.len(), 1, "the one passthrough template renders");

    let out = fs::read_to_string(cfg.docs_dir.join("WORKFLOW.md")).unwrap();
    assert!(
        out.contains("# fixture-proj. Workflow"),
        "project_name must expand, got:\n{out}"
    );
    assert!(
        out.contains("`mock/design_rounds/`"),
        "mock_dir must expand, got:\n{out}"
    );
    assert!(
        !out.contains("{{"),
        "no placeholder may survive into a published doc, got:\n{out}"
    );
}

/// A crate's own DESIGN.md.tmpl gets the vocabulary too.
///
/// Per-crate overviews were rendered verbatim, so the same literal-placeholder
/// leak applied to every crate doc.
#[test]
fn per_crate_docs_expand_placeholders() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join(".git")).unwrap();
    let mock = root.join("mock");
    write(
        &mock.join("mockspace.toml"),
        "project_name = \"fixture-proj\"\ncrate_prefix = \"fixture\"\n",
    );
    write(
        &mock.join("crates/fixture-one/src/lib.rs"),
        "//! fixture-one.\npub struct Thing;\n",
    );
    write(
        &mock.join("crates/fixture-one/DESIGN.md.tmpl"),
        "# fixture-one\n\nPart of {{project_name}}, sources under `{{mock_dir}}/`.\n",
    );

    let cfg = Config::from_dir(&mock);
    let crates = mockspace::parse::discover_crates(&cfg.crates_dir, &cfg.crate_prefix);
    let ph = render_design::Placeholders::compute(&crates, &cfg);
    render_design::ensure_docs_dir(&cfg);

    {
        let plan = mockspace::document::plan(&cfg, &crates);
        mockspace::document::render_all(&plan, &ph, &Default::default(), &cfg);
    }

    let out = fs::read_to_string(cfg.docs_dir.join("ONE.md")).unwrap();
    assert!(
        out.contains("Part of fixture-proj, sources under `mock/`."),
        "a crate template must expand the vocabulary, got:\n{out}"
    );
    assert!(
        !out.contains("{{"),
        "no placeholder may survive, got:\n{out}"
    );
}

/// The link a summary emits must be the file the writer wrote.
///
/// These were built independently and drifted the moment the naming changed:
/// every Overview link in every consumer repository pointed at a file that no
/// longer existed, and the repo used for hand-testing did not use crate
/// summaries at all, so nothing surfaced it.
#[test]
fn summary_links_match_the_files_written() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    let mock = root.join("mock");
    write(
        &mock.join("mockspace.toml"),
        "project_name = \"fixture-proj\"\ncrate_prefix = \"fixture\"\nordered_docs = true\n",
    );
    write(
        &mock.join("crates/fixture-one/src/lib.rs"),
        "//! one.\npub struct Thing;\n",
    );
    write(&mock.join("crates/fixture-one/README.md.tmpl"), "one.\n");
    write(&mock.join("crates/fixture-one/DESIGN.md.tmpl"), "# one\n");
    write(
        &mock.join("crates/fixture-one/DEEPDIVE_topic.md.tmpl"),
        "# deep\n",
    );

    let cfg = Config::from_dir(&mock);
    let crates = mockspace::parse::discover_crates(&cfg.crates_dir, &cfg.crate_prefix);
    let ph = render_design::Placeholders::compute(&crates, &cfg);
    render_design::ensure_docs_dir(&cfg);
    {
        let plan = mockspace::document::plan(&cfg, &crates);
        mockspace::document::render_all(&plan, &ph, &Default::default(), &cfg);
    }

    let summaries = ph.apply("{{crate_summaries}}");
    let mut checked = 0;
    for line in summaries.lines() {
        let Some(open) = line.find("](") else { continue };
        let Some(close) = line[open ..].find(')') else { continue };
        let target = &line[open + 2 .. open + close];
        assert!(
            cfg.docs_dir.join(target).exists(),
            "summary links {target}, which was never written. Files: {:?}",
            std::fs::read_dir(&cfg.docs_dir)
                .map(|d| {
                    d.filter_map(|e| e.ok())
                        .map(|e| e.file_name())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        );
        checked += 1;
    }
    assert!(
        checked > 0,
        "no links in the summaries, so this proved nothing"
    );
}
