use std::path::Path;

use super::*;

fn member(name: &str) -> Member {
    Member {
        name:     name.to_string(),
        version:  "1.0.0".to_string(),
        source:   Some("registry+x".to_string()),
        features: vec!["a".to_string()],
        content:  None,
    }
}

fn inputs() -> Inputs {
    Inputs {
        toolchain: "rustc 1".to_string(),
        config:    String::new(),
        manifest:  "[package]".to_string(),
        lints:     vec![("one".to_string(), "d1".to_string())],
        collector: "fn collect".to_string(),
        members:   vec![member("serde"), member("toml")],
    }
}

#[test]
fn every_input_that_decides_the_library_moves_the_identity() {
    let base = digest_inputs(&inputs());
    let mutations: Vec<(&str, Box<dyn Fn(&mut Inputs)>)> = vec![
        ("toolchain", Box::new(|i| i.toolchain.push('x'))),
        ("config", Box::new(|i| i.config.push('x'))),
        ("manifest", Box::new(|i| i.manifest.push('x'))),
        ("lint content", Box::new(|i| i.lints[0].1 = "d2".to_string())),
        ("lint name", Box::new(|i| i.lints[0].0 = "two".to_string())),
        ("collector", Box::new(|i| i.collector.push('x'))),
        ("member version", Box::new(|i| i.members[0].version = "2.0.0".to_string())),
        ("member source", Box::new(|i| i.members[0].source = None)),
        ("member features", Box::new(|i| i.members[0].features.clear())),
        ("member content", Box::new(|i| i.members[0].content = Some("c".to_string()))),
        ("member added", Box::new(|i| i.members.push(member("extra")))),
    ];
    for (what, mutate) in mutations {
        let mut changed = inputs();
        mutate(&mut changed);
        assert_ne!(digest_inputs(&changed), base, "{what} did not move the identity");
    }
}

#[test]
fn the_order_the_members_arrive_in_does_not_matter() {
    let mut swapped = inputs();
    swapped.members.reverse();
    assert_eq!(digest_inputs(&swapped), digest_inputs(&inputs()));
}

fn tree(root: &Path, body: &str) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src").join("lib.rs"), body).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();
}

#[test]
fn a_tree_digest_is_its_content_and_not_where_it_sits() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("one").join("tool");
    let b = tmp.path().join("a-much-longer-parent").join("tool");
    tree(&a, "fn x() {}");
    tree(&b, "fn x() {}");
    assert_eq!(digest_tree(&a).unwrap(), digest_tree(&b).unwrap());
    assert!(!digest_tree(&a).unwrap().is_empty());

    std::fs::write(b.join("src").join("lib.rs"), "fn y() {}").unwrap();
    assert_ne!(digest_tree(&a).unwrap(), digest_tree(&b).unwrap(), "an edit");

    tree(&b, "fn x() {}");
    std::fs::rename(b.join("src").join("lib.rs"), b.join("src").join("main.rs")).unwrap();
    assert_ne!(digest_tree(&a).unwrap(), digest_tree(&b).unwrap(), "a rename");
}

#[test]
fn build_output_and_git_state_are_not_part_of_a_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a");
    tree(&a, "fn x() {}");
    let before = digest_tree(&a).unwrap();
    std::fs::create_dir_all(a.join("target")).unwrap();
    std::fs::write(a.join("target").join("junk"), "1").unwrap();
    std::fs::create_dir_all(a.join(".git")).unwrap();
    std::fs::write(a.join(".git").join("HEAD"), "ref").unwrap();
    assert_eq!(digest_tree(&a).unwrap(), before);
}

/// `cargo metadata` as it names a graph: the generated crate at the root, one
/// registry package and one path package, the path under `prefix`.
fn metadata(prefix: &Path) -> serde_json::Value {
    let gen_manifest = prefix.join("gen").join("Cargo.toml");
    let tool_manifest = prefix.join("tool").join("Cargo.toml");
    serde_json::json!({
        "packages": [
            {"id": "gen 0.0.0", "name": "gen", "version": "0.0.0", "source": null,
             "manifest_path": gen_manifest},
            {"id": "serde 1.0.0", "name": "serde", "version": "1.0.0",
             "source": "registry+https://github.com/rust-lang/crates.io-index",
             "manifest_path": "/cargo/registry/serde/Cargo.toml"},
            {"id": "tool 0.1.0", "name": "tool", "version": "0.1.0", "source": null,
             "manifest_path": tool_manifest},
        ],
        "resolve": {
            "root": "gen 0.0.0",
            "nodes": [
                {"id": "gen 0.0.0", "features": []},
                {"id": "serde 1.0.0", "features": ["std", "derive"]},
                {"id": "tool 0.1.0", "features": []},
            ]
        }
    })
}

#[test]
fn a_graph_read_from_two_trees_of_the_same_content_is_one_graph() {
    let tmp = tempfile::tempdir().unwrap();
    let one = tmp.path().join("one");
    let two = tmp.path().join("two-with-a-longer-name");
    for t in [&one, &two] {
        tree(&t.join("tool"), "fn tool() {}");
        tree(&t.join("gen"), "fn gen_is_tree_specific() { /* path */ }");
    }
    let a = members_from_metadata(&metadata(&one), &one.join("gen").join("Cargo.toml")).unwrap();
    let b = members_from_metadata(&metadata(&two), &two.join("gen").join("Cargo.toml")).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.len(), 2, "the generated crate itself is not a member: {a:?}");
    let serde = a.iter().find(|m| m.name == "serde").unwrap();
    assert_eq!(serde.features, ["derive", "std"]);
    assert_eq!(serde.content, None, "a registry package is its version, not its files");
    let tool = a.iter().find(|m| m.name == "tool").unwrap();
    assert!(tool.content.is_some(), "a path package is its files");
    assert_eq!(tool.source, None, "and not the path they are at");
}

#[test]
fn a_changed_path_dependency_changes_the_graph() {
    let tmp = tempfile::tempdir().unwrap();
    let one = tmp.path().join("one");
    tree(&one.join("tool"), "fn tool() {}");
    tree(&one.join("gen"), "");
    let manifest = one.join("gen").join("Cargo.toml");
    let before = members_from_metadata(&metadata(&one), &manifest).unwrap();
    tree(&one.join("tool"), "fn tool() { changed() }");
    let after = members_from_metadata(&metadata(&one), &manifest).unwrap();
    assert_ne!(before, after);
}

#[test]
fn a_module_called_target_below_the_package_root_is_part_of_the_package() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a");
    tree(&a, "fn x() {}");
    let before = digest_tree(&a).unwrap();
    std::fs::create_dir_all(a.join("src").join("target")).unwrap();
    std::fs::write(a.join("src").join("target").join("mod.rs"), "fn t() {}").unwrap();
    assert_ne!(digest_tree(&a).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn a_link_is_part_of_the_package_and_a_dead_one_is_not_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a");
    tree(&a, "fn x() {}");
    let before = digest_tree(&a).unwrap();
    std::os::unix::fs::symlink("nowhere", a.join("src").join("dead")).unwrap();
    let dead = digest_tree(&a).unwrap();
    assert_ne!(dead, before, "a link to nothing");
    std::fs::remove_file(a.join("src").join("dead")).unwrap();
    std::os::unix::fs::symlink("src", a.join("dir")).unwrap();
    assert_ne!(digest_tree(&a).unwrap(), before, "a link to a directory");
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn the_cargo_config_files_the_build_meets_are_part_of_the_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("repo").join("mock");
    let home = tmp.path().join("home");
    let none = config_digest(&work, &home);

    write(&tmp.path().join("repo").join(".cargo").join("config.toml"), "[build]\nrustflags = [\"-Cx\"]\n");
    let repo = config_digest(&work, &home);
    assert_ne!(repo, none, "a config above the working directory");

    write(&home.join("config.toml"), "[profile.release]\ndebug-assertions = true\n");
    let both = config_digest(&work, &home);
    assert_ne!(both, repo, "cargo's home");

    write(&tmp.path().join("repo").join(".cargo").join("config.toml"), "[build]\nrustflags = [\"-Cy\"]\n");
    assert_ne!(config_digest(&work, &home), both, "an edit");

    // The same config under a longer path is the same input.
    let other = tmp.path().join("a-much-longer-name").join("repo").join("mock");
    write(
        &tmp.path().join("a-much-longer-name").join("repo").join(".cargo").join("config.toml"),
        "[build]\nrustflags = [\"-Cy\"]\n",
    );
    assert_eq!(config_digest(&other, &home), config_digest(&work, &home));
}

/// A generated crate with no dependencies, so `cargo metadata` needs no network.
fn generated(tmp: &Path, manifest_extra: &str) -> PathBuf {
    let dir = tmp.join("gen");
    write(&dir.join("src").join("lib.rs"), "");
    let manifest = dir.join("Cargo.toml");
    write(
        &manifest,
        &format!("[workspace]\n\n[package]\nname = \"probe-gen\"\nversion = \"0.0.0\"\nedition = \"2024\"\n{manifest_extra}"),
    );
    manifest
}

#[test]
fn every_input_is_read_where_it_is_collected() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = generated(tmp.path(), &format!("# {}/repo/tool\n", tmp.path().display()));
    let lints_dir = tmp.path().join("lints");
    write(&lints_dir.join("one.rs"), "fn one() {}");
    let lints = vec![("one".to_string(), lints_dir.join("one.rs"))];
    let work = tmp.path().join("work");
    let home = tmp.path().join("home");
    write(&work.join(".cargo").join("config.toml"), "[build]\n");
    let repo = tmp.path().join("repo");
    let tree = Tree {
        generated_manifest: &manifest,
        lints_dir:          &lints_dir,
        lints:              &lints,
        collector:          "fn collect".to_string(),
        repo_root:          &repo,
        working_dir:        &work,
        cargo_home:         &home,
    };
    let (inputs, roots) = collect_inputs(&tree).expect("collected");

    assert!(inputs.toolchain.starts_with("rustc "), "the compiler's own words: {:?}", inputs.toolchain);
    assert!(inputs.toolchain.contains("host:"), "{:?}", inputs.toolchain);
    assert_eq!(inputs.config, config_digest(&work, &home), "the config the build meets");
    assert_ne!(inputs.config, config_digest(&tmp.path().join("elsewhere"), &home));
    assert_eq!(inputs.collector, "fn collect");
    assert_eq!(inputs.lints, [("one".to_string(), digest_bytes(b"fn one() {}"))]);
    assert!(inputs.members.is_empty(), "the generated crate itself is not a member");
    assert!(
        inputs.manifest.contains("# <repo>/tool"),
        "the tree's path is taken out of the manifest: {:?}",
        inputs.manifest
    );
    assert!(!inputs.manifest.contains(&repo.display().to_string()));
    let mut expected = Roots::default();
    expected.add("repo", &repo);
    expected.add("lints", &lints_dir);
    expected.allow_absolute(&home);
    assert_eq!(roots, expected, "the repository and its lint directory, and no root for the generated crate");

    // And through the whole path: every input moves the key.
    let key = |t: &Tree| of_generated_crate(t).unwrap().key;
    let base = key(&tree);
    write(&lints_dir.join("one.rs"), "fn one() { 1; }");
    assert_ne!(key(&tree), base, "a lint edit");
    write(&lints_dir.join("one.rs"), "fn one() {}");
    assert_eq!(key(&tree), base, "and its undoing");
    write(&work.join(".cargo").join("config.toml"), "[build]\nrustflags = []\n");
    assert_ne!(key(&tree), base, "a config edit");
}

#[test]
fn a_generated_manifest_that_differs_only_by_the_tree_is_one_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let text = |root: &Path| {
        let manifest = generated(&tmp.path().join(root.file_name().unwrap()), &format!("# {}/tool\n", root.display()));
        manifest_text(&manifest, root).unwrap()
    };
    assert_eq!(text(&tmp.path().join("one")), text(&tmp.path().join("two-with-a-longer-name")));
}

#[test]
fn collecting_nothing_unreadable_is_none_and_never_a_partial_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = generated(tmp.path(), "");
    let lints_dir = tmp.path().join("lints");
    let lints = vec![("missing".to_string(), lints_dir.join("missing.rs"))];
    let tree = Tree {
        generated_manifest: &manifest,
        lints_dir:          &lints_dir,
        lints:              &lints,
        collector:          String::new(),
        repo_root:          tmp.path(),
        working_dir:        tmp.path(),
        cargo_home:         tmp.path(),
    };
    assert!(of_generated_crate(&tree).is_none());
}

fn git_in(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A package at `<repo>/pkg` in a fresh git repository, with its files staged.
fn tracked_package(repo: &Path) -> std::path::PathBuf {
    git_in(repo.parent().unwrap(), &["init", "-q", repo.to_str().unwrap()]);
    let pkg = repo.join("pkg");
    tree(&pkg, "fn x() {}");
    git_in(repo, &["add", "pkg"]);
    pkg
}

#[test]
fn a_file_git_does_not_track_is_not_part_of_a_package_in_a_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let pkg = tracked_package(&repo);
    let before = digest_tree(&pkg).unwrap();

    std::fs::write(pkg.join("scratch.rs"), "fn scratch() {}").unwrap();
    std::fs::create_dir_all(pkg.join("src").join("notes")).unwrap();
    std::fs::write(pkg.join("src").join("notes").join("draft.rs"), "fn d() {}").unwrap();
    assert_eq!(digest_tree(&pkg).unwrap(), before, "an untracked file moved the identity");

    std::fs::write(repo.join(".gitignore"), "ignored.bin\n").unwrap();
    std::fs::write(pkg.join("ignored.bin"), [1, 2, 3]).unwrap();
    assert_eq!(digest_tree(&pkg).unwrap(), before, "an ignored file moved the identity");
}

#[test]
fn a_tracked_file_is_part_of_a_package_whatever_its_state() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let pkg = tracked_package(&repo);
    let before = digest_tree(&pkg).unwrap();

    std::fs::write(pkg.join("src").join("lib.rs"), "fn y() {}").unwrap();
    let edited = digest_tree(&pkg).unwrap();
    assert_ne!(edited, before, "an edit not yet staged");

    // The case that must fail to stay equal: a file added to the index is a file of the package.
    std::fs::write(pkg.join("src").join("extra.rs"), "fn e() {}").unwrap();
    git_in(&repo, &["add", "pkg/src/extra.rs"]);
    let added = digest_tree(&pkg).unwrap();
    assert_ne!(added, edited, "a staged new file");

    // A tracked file gone from the disk is a change, and is not an error.
    std::fs::remove_file(pkg.join("src").join("extra.rs")).unwrap();
    assert_ne!(digest_tree(&pkg).unwrap(), added, "a tracked file deleted from the disk");
}

#[test]
fn a_package_outside_any_repository_is_read_whole() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    tree(&pkg, "fn x() {}");
    let before = digest_tree(&pkg).unwrap();
    std::fs::write(pkg.join("more.rs"), "fn m() {}").unwrap();
    assert_ne!(digest_tree(&pkg).unwrap(), before, "git cannot say, so every file counts");
}

#[test]
fn a_package_git_does_not_track_at_all_is_read_whole() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    git_in(tmp.path(), &["init", "-q", repo.to_str().unwrap()]);
    let pkg = repo.join("pkg");
    tree(&pkg, "fn x() {}");
    let before = digest_tree(&pkg).unwrap();
    // A pack drafted before its first `git add`: the manifest is not in any lint source or dep-info.
    std::fs::write(pkg.join("Cargo.toml"), "[package]\nedition = \"2021\"\n").unwrap();
    assert_ne!(digest_tree(&pkg).unwrap(), before, "a manifest edit in an untracked package");
    std::fs::write(pkg.join("src").join("lib.rs"), "fn y() {}").unwrap();
    let edited = digest_tree(&pkg).unwrap();
    assert_ne!(edited, before, "a source edit in an untracked package");
}
