use std::time::Duration;

use super::*;
use crate::custom_lints::build::{Built, Unit};

fn later() -> SystemTime {
    SystemTime::now() + Duration::from_secs(1)
}

/// What a build reports, for a library at `dylib` that cargo `fresh`ly reused or built after `started`.
fn made(dylib: &Path, started: SystemTime, fresh: bool) -> Built {
    Built { dylib: dylib.to_path_buf(), started, units: Vec::new(), fresh }
}

/// A built tree: the library, its dep-info listing `sources`, under `target/release`.
fn built(root: &Path, sources: &str) -> PathBuf {
    let release = root.join("target").join("release");
    std::fs::create_dir_all(&release).unwrap();
    std::fs::write(release.join("libgen.so"), "library").unwrap();
    // Cargo leaves the variables out of the copy beside the library.
    let uplifted: Vec<&str> = sources.lines().filter(|l| !l.starts_with("# env-dep:")).collect();
    std::fs::write(release.join("libgen.d"), uplifted.join("\n")).unwrap();
    let deps = release.join("deps");
    std::fs::create_dir_all(&deps).unwrap();
    std::fs::write(deps.join("gen.d"), sources).unwrap();
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
        names.push(roots.name(&lints.join("sub").join("part.rs")).unwrap());
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
    assert_eq!(roots.name(&inner.join("f.rs")).as_deref(), Some("inner\tf.rs"));
    let elsewhere = tmp.path().join("elsewhere.rs");
    std::fs::write(&elsewhere, "x").unwrap();
    assert_eq!(roots.name(&elsewhere), None, "under no root, and not declared the same for every tree");
    roots.allow_absolute(tmp.path());
    assert!(roots.name(&elsewhere).unwrap().starts_with("abs\t"));
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

    let deps = record(&made(&dylib, later(), false), &gen_dir, &a, Duration::ZERO).expect("recorded");
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

    assert!(record(&made(&dylib, later(), false), &gen_dir, &roots, Duration::ZERO).is_some(), "the control");
    let before_the_file = SystemTime::now() - Duration::from_secs(3600);
    assert!(
        record(&made(&dylib, before_the_file, false), &gen_dir, &roots, Duration::ZERO).is_none(),
        "a source modified after the build began may have been read half-edited"
    );
    std::fs::remove_file(&source).unwrap();
    assert!(record(&made(&dylib, later(), false), &gen_dir, &roots, Duration::ZERO).is_none(), "a source that is gone");
    std::fs::remove_file(dylib.with_extension("d")).unwrap();
    assert!(record(&made(&dylib, later(), false), &gen_dir, &roots, Duration::ZERO).is_none(), "no dep-info");
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
    std::thread::sleep(Duration::from_millis(20));
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
    assert!(record(&made(&dylib, began, false), &gen_dir, &roots, Duration::ZERO).is_none());
}

#[test]
fn a_source_under_no_root_leaves_nothing_kept_unless_the_directory_is_the_same_for_every_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = tmp.path().join("gen");
    let outside = tmp.path().join("repo").join("name.txt");
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    std::fs::write(&outside, "x").unwrap();
    let dylib = built(&gen_dir, &format!("out.d: {}\n", outside.display()));
    let mut roots = Roots::default();
    assert!(record(&made(&dylib, later(), false), &gen_dir, &roots, Duration::ZERO).is_none());
    roots.add("repo", &tmp.path().join("repo"));
    let deps = record(&made(&dylib, later(), false), &gen_dir, &roots, Duration::ZERO).expect("under the repository root");
    assert_eq!(deps.len(), 1);
}

#[cfg(unix)]
#[test]
fn a_file_behind_a_link_is_named_as_the_compiler_listed_it() {
    let tmp = tempfile::tempdir().unwrap();
    let lints = tmp.path().join("lints");
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&lints).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("name.txt"), "x").unwrap();
    std::os::unix::fs::symlink("../data", lints.join("data")).unwrap();
    let mut roots = Roots::default();
    roots.add("lints", &lints);
    assert_eq!(
        roots.name(&lints.join("data").join("name.txt")).as_deref(),
        Some("lints\tdata/name.txt"),
        "by the path through the link, which another tree resolves to its own file"
    );
}

#[test]
fn a_variable_the_build_was_given_is_compared_and_one_it_was_not_is_not() {
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = tmp.path().join("gen");
    let info = "out.d: \n# env-dep:CARGO_PKG_NAME=probe\n# env-dep:NO_COLOR=1\n# env-dep:MOCKSPACE_PROBE_UNSET_VARIABLE\n";
    let dylib = built(&gen_dir, info);
    let roots = Roots::default();
    let deps = record(&made(&dylib, later(), false), &gen_dir, &roots, Duration::ZERO).expect("recorded");
    assert_eq!(deps.len(), 3);
    // a variable the build was not given is not compared, whatever this process has
    let cargo_set = Dep { name: "env\tCARGO_PKG_NAME".to_string(), digest: digest_bytes(b"other") };
    assert!(hold(&[cargo_set], &roots));
    let given = Dep { name: "env\tPATH".to_string(), digest: digest_bytes(b"not this process's PATH") };
    assert!(!hold(&[given], &roots), "a variable the build was given, and reads differently now");
}

#[test]
fn a_unit_dep_info_left_by_an_earlier_build_is_not_read_for_this_one() {
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = tmp.path().join("gen");
    let dylib = built(&gen_dir, "out.d: \n# env-dep:NO_COLOR=1\n");
    let roots = Roots::default();
    assert!(record(&made(&dylib, later(), false), &gen_dir, &roots, Duration::ZERO).is_some(), "the control");
    // With `build.build-dir` set rustc writes the unit's dep-info elsewhere, and this one is what
    // an earlier build without it left behind.
    std::fs::File::options()
        .write(true)
        .open(dylib.parent().unwrap().join("deps").join("gen.d"))
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
    assert!(record(&made(&dylib, later(), false), &gen_dir, &roots, Duration::ZERO).is_none());
}

#[test]
fn a_reused_build_holds_the_sources_to_the_time_it_was_made() {
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = tmp.path().join("gen");
    let source = tmp.path().join("lints").join("probe.rs");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, "x").unwrap();
    let mut roots = Roots::default();
    roots.add("lints", source.parent().unwrap());
    let dylib = built(&gen_dir, &format!("out.d: {}\n", source.display()));
    let reused = made(&dylib, later(), true);
    assert!(record(&reused, &gen_dir, &roots, Duration::ZERO).is_some(), "the source is older than the output");
    // The source changes after the output was made, with a modification time set back, and cargo
    // calls the output fresh all the same.
    std::fs::File::options()
        .write(true)
        .open(dylib.parent().unwrap().join("deps").join("gen.d"))
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
    assert!(record(&reused, &gen_dir, &roots, Duration::ZERO).is_none());
}

#[test]
fn a_unit_that_read_the_directory_of_a_manifest_in_the_tree_leaves_nothing_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = tmp.path().join("gen");
    let tool = tmp.path().join("repo").join("tools").join("t");
    std::fs::create_dir_all(&tool).unwrap();
    let info = format!("out.d: \n# env-dep:CARGO_MANIFEST_DIR={}\n", tool.display());
    let dylib = built(&gen_dir, &info);
    let mut roots = Roots::default();
    assert!(record(&made(&dylib, later(), false), &gen_dir, &roots, Duration::ZERO).is_none());
    let own = built(&gen_dir, &format!("out.d: \n# env-dep:CARGO_MANIFEST_DIR={}\n", gen_dir.display()));
    assert!(record(&made(&own, later(), false), &gen_dir, &roots, Duration::ZERO).is_some(), "the shared crate itself");
    roots.allow_absolute(tmp.path().join("repo").as_path());
    assert!(record(&made(&dylib, later(), false), &gen_dir, &roots, Duration::ZERO).is_some());
}

#[test]
fn the_unit_dep_info_is_named_for_the_unit_wherever_the_output_was_linked() {
    let name = |file: &str| unit_info_of(Path::new(file)).unwrap();
    // The library linked up into the profile directory.
    assert_eq!(name("/t/release/libmockspace_lints_ab.so"), Path::new("/t/release/deps/mockspace_lints_ab.d"));
    // A dependency in `deps/` carries its hash, and rustc leaves the `lib` off its dep-info.
    assert_eq!(name("/t/release/deps/libserde-0f1e.rlib"), Path::new("/t/release/deps/serde-0f1e.d"));
    // A cross target adds a directory above the profile and changes nothing below it.
    assert_eq!(
        name("/t/x86_64-unknown-linux-gnu/release/libmockspace_lints_ab.so"),
        Path::new("/t/x86_64-unknown-linux-gnu/release/deps/mockspace_lints_ab.d")
    );
}

/// A dependency unit beside the library: its output in `deps/`, its dep-info listing `source`.
fn with_unit(dylib: &Path, source: &Path, fresh: bool, tree_code: bool) -> Built {
    let deps = dylib.parent().unwrap().join("deps");
    std::fs::write(deps.join("tool-0f.d"), format!("tool-0f.rlib: {}\n", source.display())).unwrap();
    let unit = Unit { files: vec![deps.join("libtool-0f.rlib")], fresh, tree_code };
    Built { units: vec![unit], ..made(dylib, later(), false) }
}

#[test]
fn a_reused_dependency_unit_holds_its_sources_to_the_time_it_was_made() {
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = tmp.path().join("gen");
    let source = tmp.path().join("lints").join("tool.rs");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, "x").unwrap();
    let mut roots = Roots::default();
    roots.add("lints", source.parent().unwrap());
    let dylib = built(&gen_dir, "out.d: \n");
    let reused = with_unit(&dylib, &source, true, false);
    assert!(record(&reused, &gen_dir, &roots, Duration::ZERO).is_some(), "the source is older than its unit");
    // The unit is old, the library above it was built just now, and the source is newer than the unit.
    let unit_info = dylib.parent().unwrap().join("deps").join("tool-0f.d");
    std::fs::File::options()
        .write(true)
        .open(&unit_info)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
    assert!(record(&reused, &gen_dir, &roots, Duration::ZERO).is_none());
    let rebuilt = with_unit(&dylib, &source, false, false);
    std::fs::File::options()
        .write(true)
        .open(&unit_info)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
    assert!(record(&rebuilt, &gen_dir, &roots, Duration::ZERO).is_some(), "a unit cargo built is held to the build");
}

#[test]
fn a_unit_without_dep_info_leaves_nothing_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = tmp.path().join("gen");
    let dylib = built(&gen_dir, "out.d: \n");
    let unit = Unit { files: vec![dylib.parent().unwrap().join("deps").join("libabsent-1.rlib")], fresh: false, tree_code: false };
    let build = Built { units: vec![unit], ..made(&dylib, later(), false) };
    assert!(record(&build, &gen_dir, &Roots::default(), Duration::ZERO).is_none());
}

#[test]
fn a_build_script_or_macro_of_a_path_package_leaves_nothing_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = tmp.path().join("gen");
    let source = tmp.path().join("lints").join("tool.rs");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, "x").unwrap();
    let mut roots = Roots::default();
    roots.add("lints", source.parent().unwrap());
    let dylib = built(&gen_dir, "out.d: \n");
    assert!(record(&with_unit(&dylib, &source, false, false), &gen_dir, &roots, Duration::ZERO).is_some());
    assert!(record(&with_unit(&dylib, &source, false, true), &gen_dir, &roots, Duration::ZERO).is_none());
}

/// The change time of a file edited inside the window between the compiler's read and ours is
/// after the build began, and the bytes we hold are the new ones. Reading the bytes first is what
/// makes the time catch it; the hook runs between the two calls so the order is what is tested.
#[test]
fn a_source_edited_between_the_read_and_the_stat_is_not_recorded() {
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = tmp.path().join("gen");
    let source = tmp.path().join("lints").join("probe.rs");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, "old").unwrap();
    let mut roots = Roots::default();
    roots.add("lints", source.parent().unwrap());
    let dylib = built(&gen_dir, &format!("out.d: {}\n", source.display()));
    // The file is older than the build, with room for the filesystem's timestamp to tell the two apart.
    std::thread::sleep(Duration::from_millis(60));
    let began = SystemTime::now();
    std::thread::sleep(Duration::from_millis(60));

    let control = record(&made(&dylib, began, false), &gen_dir, &roots, Duration::ZERO);
    assert!(control.is_some(), "the control: a source older than the build, with nobody editing it");

    let edit = source.clone();
    let _hook = crate::custom_lints::store::knobs::between_read_and_stat(move |file| {
        if file == edit {
            std::fs::write(file, "new").unwrap();
        }
    });
    assert!(
        record(&made(&dylib, began, false), &gen_dir, &roots, Duration::ZERO).is_none(),
        "an edit after the read must show in the change time, or the old library is kept beside the new content"
    );
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "new", "the hook ran");
}
