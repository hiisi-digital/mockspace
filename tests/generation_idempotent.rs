//! Regenerating docs on an unchanged source must not churn the output.
//!
//! Every generated file carries a `Generated at:` timestamp line, so a
//! naive rewrite dirties the tree on every run even when nothing changed.
//! `write_generated` skips a write whose only difference is that line.
//! The first test here calls `document::render_all` directly, which is the
//! writer and not the generation path. It passed for as long as it has existed
//! while `cargo mock` rewrote every document on every run, because the wipe
//! that defeated the skip lived in `entry::dispatch` and nothing here went
//! through it. Its docstring claimed end to end and it was not; the binary test
//! at the bottom of this file is.

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

/// The whole binary, twice, on a tree it just generated.
///
/// **This is the arm the direct-call tests above cannot be.** `docs/` used to be
/// wiped at the top of `entry::dispatch` before a line was written, so
/// `write_generated` compared every document against nothing and rewrote all of
/// them with a fresh `Generated at:` on every run. Sixteen files dirtied per run
/// in a real project, forever, and every test of the writer passed throughout.
///
/// Anything that reintroduces a pre-generation clean reddens this and nothing
/// else in the file.
#[test]
#[ignore = "runs the binary and shells out; run with --ignored"]
fn the_binary_leaves_a_tree_it_just_generated_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // A real repository and a real workspace manifest, because this runs the
    // binary rather than a library entry point and the binary checks for both.
    // The other tests in this file get away with an empty `.git` directory,
    // which is part of why they never exercised the path that was broken.
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status()
        .expect("git init");
    let mock = root.join("mock");
    write(&mock.join("mockspace.toml"), "project_name = \"probe\"\n");
    write(&mock.join("Cargo.toml"), "[workspace]\nmembers = []\nresolver = \"2\"\n");
    write(&mock.join("PROJECT.md.tmpl"), "# Probe\n\nA body that does not change.\n");

    let run = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_mockspace"))
            .arg("--dir")
            .arg(&mock)
            .output()
            .expect("the binary runs")
    };

    run();
    let after_first = snapshot(&root.join("docs"));
    assert!(
        !after_first.is_empty(),
        "control: the first run must generate something, else this passes on an empty directory"
    );

    // A second later, so a rewritten timestamp differs from the first.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    run();
    let after_second = snapshot(&root.join("docs"));

    assert_eq!(
        after_first, after_second,
        "a second generation on an unchanged tree must write nothing"
    );
}

/// Every top-level file under `dir`, by name and content.
fn snapshot(dir: &Path) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let Ok(rd) = fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_file() {
            if let Ok(s) = fs::read_to_string(&p) {
                out.insert(p.file_name().unwrap().to_string_lossy().to_string(), s);
            }
        }
    }
    out
}
