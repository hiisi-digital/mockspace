use super::*;

/// A repo with a lint, a pack and two tools, so a scope has something to
/// leave out.
fn repo_with_lints_packs_and_tools() -> (tempfile::TempDir, crate::config::Config) {
    let dir = tempfile::tempdir().unwrap();
    let mock = dir.path().join("mock");
    std::fs::create_dir_all(mock.join("lints")).unwrap();
    std::fs::write(
        mock.join("lints").join("foo.rs"),
        "pub fn lint() -> Box<dyn mockspace::Lint> { todo!() }\n",
    )
    .unwrap();
    for tool in ["ticket-add", "asset-new"] {
        let t = mock.join("tools").join(tool);
        std::fs::create_dir_all(&t).unwrap();
        std::fs::write(
            t.join("Cargo.toml"),
            format!("[package]\nname = \"{tool}\"\nversion = \"0.0.0\"\n"),
        )
        .unwrap();
    }
    std::fs::write(
        dir.path().join("mockspace.toml"),
        "[lint-crates]\nsome-pack = \"1\"\n",
    )
    .unwrap();
    let cfg = crate::config::Config::from_dir(&mock);
    (dir, cfg)
}

#[test]
fn a_tool_scope_carries_the_one_tool_and_no_lints_or_packs() {
    let (dir, cfg) = repo_with_lints_packs_and_tools();
    let path = dir.path().join("mockspace.toml");
    // control: the whole content is all of it, so the scope below is
    // leaving something out rather than there being nothing to leave
    let whole = discover(&cfg, &path).unwrap();
    assert_eq!(whole.lint_files, ["foo"]);
    assert_eq!(whole.packs.len(), 1);
    assert_eq!(whole.tools.len(), 2);

    let scoped = discover_scoped(&cfg, &path, "ticket-add").unwrap();
    assert!(scoped.lint_files.is_empty(), "no lint builds for a tool");
    assert!(scoped.packs.is_empty(), "no pack builds for a tool");
    let tools: Vec<&str> = scoped.tools.iter().map(|t| t.dir.as_str()).collect();
    assert_eq!(tools, ["ticket-add"]);
}

#[test]
fn a_name_that_is_no_tool_directory_scopes_to_nothing() {
    let (dir, cfg) = repo_with_lints_packs_and_tools();
    let path = dir.path().join("mockspace.toml");
    assert!(discover_scoped(&cfg, &path, "foo").is_none());
    assert!(discover_scoped(&cfg, &path, "some-pack").is_none());
}

#[test]
fn collector_lib_shape() {
    let dir = tempfile::tempdir().unwrap();
    let lints = dir.path().join("lints");
    std::fs::create_dir_all(&lints).unwrap();
    std::fs::write(
        lints.join("foo.rs"),
        "pub fn lint() -> Box<dyn mockspace::Lint> { todo!() }\n",
    )
    .unwrap();
    let src = gen_collect_lib(
        &lints,
        &["foo".to_string()],
        &[("some-pack".into(), "\"1\"".into())],
        &[],
    );
    assert!(src.contains("#[unsafe(no_mangle)]"));
    assert!(src.contains("pub extern \"C\" fn __mockspace_collect_lints"));
    assert!(src.contains("mod foo;"));
    // one struct across the boundary, so a new lint kind is additive
    assert!(src.contains("pack: &mut mockspace::LintPack"));
    assert!(src.contains("pack.crate_lints.push(foo::lint());"));
    // pack idents dash->underscore, and one collect() call per pack rather
    // than one call per lint kind
    assert!(src.contains("some_pack::collect(pack);"));
    // foo has no cross_lint(), so no per-file workspace push for it
    assert!(!src.contains("pack.workspace_lints.push(foo::cross_lint());"));
}

/// All four entry points, which the scanner recognised as two until
/// recently. `RepoLint` is the one handed paths rather than packages, so it
/// is the only kind a repository with no packages can register, and it was
/// reachable from an imported pack and from nowhere else.
///
/// The trap is in the names. `cross_lint(`, `repo_lint(` and
/// `message_lint(` all end in `lint(`, so a probe for the plain kind that
/// is not anchored on the space after `fn` claims every file that defines
/// any of them. That is what the negatives below are for, and the scanner
/// carries a comment about it while pinning nothing.
#[test]
fn every_entry_point_is_recognised_and_none_stands_in_for_another() {
    let dir = tempfile::tempdir().unwrap();
    let lints = dir.path().join("lints");
    std::fs::create_dir_all(&lints).unwrap();

    for (stem, decl) in [
        ("plain", "pub fn lint()"),
        ("cross", "pub fn cross_lint()"),
        ("repo", "pub fn repo_lint()"),
        ("msg", "pub fn message_lint()"),
    ] {
        std::fs::write(
            lints.join(format!("{stem}.rs")),
            format!("{decl} {{ todo!() }}\n"),
        )
        .unwrap();
    }

    let modules: Vec<String> = ["plain", "cross", "repo", "msg"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let src = gen_collect_lib(&lints, &modules, &[], &[]);

    assert!(src.contains("pack.crate_lints.push(plain::lint());"));
    assert!(src.contains("pack.workspace_lints.push(cross::cross_lint());"));
    assert!(src.contains("pack.repo_lints.push(repo::repo_lint());"));
    assert!(src.contains("pack.message_lints.push(msg::message_lint());"));

    // the part with teeth: a file defining only one kind registers only
    // that kind, so a suffix match on `lint(` cannot pass this
    assert!(!src.contains("pack.crate_lints.push(cross::lint());"));
    assert!(!src.contains("pack.crate_lints.push(repo::lint());"));
    assert!(!src.contains("pack.crate_lints.push(msg::lint());"));
    assert!(!src.contains("pack.repo_lints.push(plain::repo_lint());"));
    assert!(!src.contains("pack.message_lints.push(plain::message_lint());"));
}

/// A file may define more than one, and each is pushed once onto its own
/// list. The control for the test above, which uses one kind per file and
/// so cannot tell "recognises each" from "recognises whichever it sees
/// first".
#[test]
fn one_file_may_register_several_kinds() {
    let dir = tempfile::tempdir().unwrap();
    let lints = dir.path().join("lints");
    std::fs::create_dir_all(&lints).unwrap();
    std::fs::write(
        lints.join("both.rs"),
        "pub fn lint() { todo!() }\npub fn repo_lint() { todo!() }\n",
    )
    .unwrap();

    let src = gen_collect_lib(&lints, &["both".to_string()], &[], &[]);
    assert!(src.contains("pack.crate_lints.push(both::lint());"));
    assert!(src.contains("pack.repo_lints.push(both::repo_lint());"));
    assert!(!src.contains("pack.workspace_lints.push(both::cross_lint());"));
    assert!(!src.contains("pack.message_lints.push(both::message_lint());"));
}

fn tool(dir: &str, package: &str, at: &Path) -> ToolCrate {
    ToolCrate {
        dir:     dir.to_string(),
        package: package.to_string(),
        path:    at.join(dir),
    }
}

#[test]
fn a_tool_crate_is_collected_through_the_same_entry_point_a_pack_uses() {
    let dir = tempfile::tempdir().unwrap();
    let lints = dir.path().join("lints");
    std::fs::create_dir_all(&lints).unwrap();
    let t = tool("phrase-search", "demo-phrase-search", dir.path());

    let src = gen_collect_lib(&lints, &[], &[], std::slice::from_ref(&t));
    // dash-to-underscore on the package name, and one collect() call, so a
    // tool crate shipping a lint beside its tool registers both.
    assert!(src.contains("demo_phrase_search::collect(pack);"), "{src}");
    // and it must NOT invent a tool-specific symbol: the negative that
    // pins "same pattern as lints" rather than a parallel path.
    assert!(!src.contains("demo_phrase_search::tool()"), "{src}");
    assert!(!src.contains("pack.tools.push"), "{src}");
}

#[test]
fn a_tool_crate_lands_on_the_dependency_line_by_package_name_and_path() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = crate::config::Config::from_dir(dir.path());
    let gen_dir = dir.path().join("gen_dir");
    let t = tool("phrase-search", "demo-phrase-search", dir.path());
    write_cdylib_crate(
        &gen_dir,
        &dir.path().join("lints"),
        &[],
        &[],
        std::slice::from_ref(&t),
        "{ package = \"mockspace-lint-rules\", git = \"u\", rev = \"r\" }",
        &cfg,
    )
    .unwrap();
    let manifest = std::fs::read_to_string(gen_dir.join("Cargo.toml")).unwrap();
    assert!(
        manifest.contains(&format!(
            "demo-phrase-search = {{ path = \"{}\" }}",
            t.path.display()
        )),
        "{manifest}"
    );
    // The case that must fail: depending on the DIRECTORY name would emit
    // `phrase-search = ...`, which cargo cannot resolve to that package.
    assert!(
        !manifest.contains("\nphrase-search = {"),
        "the directory name must not be used as the package name: {manifest}"
    );
}

#[test]
fn a_repo_with_only_tools_still_gets_a_cdylib() {
    // The early return in `load` short-circuits on "no lints and no packs".
    // Before tools were counted there, a repo whose only custom content is
    // a tool built nothing and `mock <tool>` found nothing.
    let dir = tempfile::tempdir().unwrap();
    let mock = dir.path().join("mock");
    std::fs::create_dir_all(mock.join("tools").join("only-tool").join("src")).unwrap();
    std::fs::write(
        mock.join("tools").join("only-tool").join("Cargo.toml"),
        "[package]\nname = \"only-tool\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();
    std::fs::write(mock.join("mockspace.toml"), "project_name = \"x\"\n").unwrap();

    // no mock/lints, no [lint-crates]: the two things the old guard checked
    assert!(!mock.join("lints").exists());
    assert!(parse_lint_crates(&mock.join("mockspace.toml")).is_empty());
    // and yet there is something to build
    assert_eq!(discover_tool_crates(&mock.join("tools")).len(), 1);
}

#[test]
fn manifest_renames_lint_rules_to_mockspace() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = crate::config::Config::from_dir(dir.path());
    let gen_dir = dir.path().join("gen_dir");
    let dep = "{ package = \"mockspace-lint-rules\", git = \"u\", rev = \"r\" }";
    write_cdylib_crate(
        &gen_dir,
        &dir.path().join("lints"),
        &[],
        &[("p".into(), "\"1\"".into())],
        &[],
        dep,
        &cfg,
    )
    .unwrap();
    let manifest = std::fs::read_to_string(gen_dir.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("crate-type = [\"cdylib\"]"));
    assert!(manifest.contains(&format!("mockspace = {dep}")));
    assert!(manifest.contains("p = \"1\""));
    // a leading [workspace] makes the crate its own root so cargo builds it
    // standalone under the consumer's <mock>/target/ (not as a member).
    assert!(manifest.trim_start().starts_with("[workspace]"));
}
