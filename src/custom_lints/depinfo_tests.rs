use std::time::Duration;

use super::*;

fn later() -> SystemTime {
    SystemTime::now() + Duration::from_secs(60)
}

/// A built tree: the library, its dep-info listing `sources`, under `target/release`.
fn built(root: &Path, sources: &str) -> PathBuf {
    let release = root.join("target").join("release");
    std::fs::create_dir_all(&release).unwrap();
    std::fs::write(release.join("libgen.so"), "library").unwrap();
    std::fs::write(release.join("libgen.d"), sources).unwrap();
    release.join("libgen.so")
}

#[test]
fn the_sources_of_a_dep_info_are_its_first_rule() {
    let info = "/t/gen-abc.d: /a/one.rs /a/two\\ words.rs src/lib.rs\n\n/a/one.rs:\n/a/two words.rs:\n\n# env-dep:CARGO_PKG_NAME=gen\n";
    let sources = sources_of(info);
    assert_eq!(sources, [
        PathBuf::from("/a/one.rs"),
        PathBuf::from("/a/two words.rs"),
        PathBuf::from("src/lib.rs")
    ]);
    assert!(sources_of("# env-dep:X\n").is_empty(), "comments are not sources");
    assert!(sources_of("").is_empty());
}

#[test]
fn a_file_is_named_by_its_root_and_not_by_where_the_root_sits() {
    let tmp = tempfile::tempdir().unwrap();
    let mut names = Vec::new();
    for parent in ["one", "a-much-longer-parent"] {
        let lints = tmp.path().join(parent).join("lints");
        std::fs::create_dir_all(lints.join("sub")).unwrap();
        std::fs::write(lints.join("sub").join("part.rs"), "x").unwrap();
        let mut roots = Roots::default();
        roots.add("lints", &lints);
        names.push(roots.name(&lints.join("sub").join("part.rs")));
    }
    assert_eq!(names[0], names[1]);
    assert_eq!(names[0], "lints\tsub/part.rs");
}

#[test]
fn a_file_under_no_root_is_named_absolutely_and_the_deepest_root_wins() {
    let tmp = tempfile::tempdir().unwrap();
    let outer = tmp.path().join("outer");
    let inner = outer.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(inner.join("f.rs"), "x").unwrap();
    let mut roots = Roots::default();
    roots.add("outer", &outer);
    roots.add("inner", &inner);
    assert_eq!(roots.name(&inner.join("f.rs")), "inner\tf.rs");
    let elsewhere = tmp.path().join("elsewhere.rs");
    std::fs::write(&elsewhere, "x").unwrap();
    assert!(roots.name(&elsewhere).starts_with("abs\t"));
}

#[test]
fn what_was_recorded_holds_in_a_tree_of_the_same_content_and_not_after_an_edit() {
    let tmp = tempfile::tempdir().unwrap();
    let trees: Vec<PathBuf> = ["a", "b-with-a-longer-name"]
        .iter()
        .map(|n| {
            let lints = tmp.path().join(n).join("lints");
            std::fs::create_dir_all(&lints).unwrap();
            std::fs::write(lints.join("probe.rs"), "lint").unwrap();
            std::fs::write(lints.join("text.txt"), "one").unwrap();
            lints
        })
        .collect();
    let gen_dir = tmp.path().join("gen");
    let info = format!(
        "{g}/target/release/libgen.d: {l}/probe.rs {l}/text.txt {g}/src/lib.rs\n",
        g = gen_dir.display(),
        l = trees[0].display()
    );
    std::fs::create_dir_all(gen_dir.join("src")).unwrap();
    std::fs::write(gen_dir.join("src").join("lib.rs"), "generated").unwrap();
    let dylib = built(&gen_dir, &info);
    let (mut a, mut b) = (Roots::default(), Roots::default());
    a.add("lints", &trees[0]);
    b.add("lints", &trees[1]);

    let deps = record(&dylib, &gen_dir, &a, later()).expect("recorded");
    assert_eq!(deps.len(), 2, "the generated crate's own files are not inputs: {deps:?}");
    assert!(hold(&deps, &a));
    assert!(hold(&deps, &b), "another tree of the same content");

    std::fs::write(trees[1].join("text.txt"), "two").unwrap();
    assert!(!hold(&deps, &b), "an included file that changed");
    std::fs::remove_file(trees[1].join("text.txt")).unwrap();
    assert!(!hold(&deps, &b), "an included file that is gone");
}

#[test]
fn nothing_is_recorded_that_cannot_be_trusted() {
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = tmp.path().join("gen");
    let source = tmp.path().join("lints").join("probe.rs");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, "x").unwrap();
    let mut roots = Roots::default();
    roots.add("lints", source.parent().unwrap());
    let dylib = built(&gen_dir, &format!("out.d: {}\n", source.display()));

    assert!(record(&dylib, &gen_dir, &roots, later()).is_some(), "the control");
    let before_the_file = SystemTime::now() - Duration::from_secs(3600);
    assert!(
        record(&dylib, &gen_dir, &roots, before_the_file).is_none(),
        "a source modified after the build began may have been read half-edited"
    );
    std::fs::remove_file(&source).unwrap();
    assert!(record(&dylib, &gen_dir, &roots, later()).is_none(), "a source that is gone");
    std::fs::remove_file(dylib.with_extension("d")).unwrap();
    assert!(record(&dylib, &gen_dir, &roots, later()).is_none(), "no dep-info");
}

#[test]
fn a_record_reads_back_as_written_and_a_foreign_one_does_not() {
    let deps = vec![
        Dep { name: "lints\tsub/a.rs".to_string(), digest: "d1".to_string() },
        Dep { name: "abs\t/x/y.rs".to_string(), digest: "d2".to_string() },
    ];
    assert_eq!(decode(&encode(&deps)), Some(deps));
    assert_eq!(decode("something else\nd\tn\n"), None);
    assert_eq!(decode("mockspace-pack-deps-1\nno separator\n"), None);
}

#[test]
fn a_source_whose_timestamp_was_set_back_is_not_trusted() {
    let tmp = tempfile::tempdir().unwrap();
    let began = SystemTime::now();
    let gen_dir = tmp.path().join("gen");
    let source = tmp.path().join("lints").join("probe.rs");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, "written after the build began").unwrap();
    // What `cp -p`, `tar x` and `rsync -t` do: the modification time says long ago.
    std::fs::File::options()
        .write(true)
        .open(&source)
        .unwrap()
        .set_modified(began - Duration::from_secs(3600))
        .unwrap();
    let mut roots = Roots::default();
    roots.add("lints", source.parent().unwrap());
    let dylib = built(&gen_dir, &format!("out.d: {}\n", source.display()));
    assert!(record(&dylib, &gen_dir, &roots, began).is_none());
}
