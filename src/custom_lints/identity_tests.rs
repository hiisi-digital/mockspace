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
        flags:     String::new(),
        lints:     vec![("one".to_string(), "d1".to_string())],
        collector: "fn collect".to_string(),
        members:   vec![member("serde"), member("toml")],
    }
}

#[test]
fn the_same_inputs_give_the_same_identity() {
    assert_eq!(digest_inputs(&inputs()), digest_inputs(&inputs()));
    assert!(!digest_inputs(&inputs()).is_empty());
}

#[test]
fn every_input_that_decides_the_library_moves_the_identity() {
    let base = digest_inputs(&inputs());
    let mutations: Vec<(&str, Box<dyn Fn(&mut Inputs)>)> = vec![
        ("toolchain", Box::new(|i| i.toolchain.push('x'))),
        ("flags", Box::new(|i| i.flags.push('x'))),
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
