//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use letkajenkka::Kind;

use super::*;

/// The `conformance/` directory of the `letkajenkka` this crate builds against: the version the
/// lockfile pins, in the source cargo unpacked from the registry it came from.
fn conformance_dir() -> PathBuf {
    let lock = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Cargo.lock")).expect("the lockfile");
    let lock: toml_edit::DocumentMut = lock.parse().expect("the lockfile is toml");
    let package = lock
        .get("package")
        .and_then(|p| p.as_array_of_tables())
        .and_then(|all| all.iter().find(|p| p.get("name").and_then(|n| n.as_str()) == Some("letkajenkka")))
        .expect("the lockfile holds letkajenkka");
    let version = package.get("version").and_then(|v| v.as_str()).expect("a version");
    let source = package.get("source").and_then(|v| v.as_str()).unwrap_or_default();
    assert!(source.starts_with("registry+"), "letkajenkka is taken from a registry, and the lockfile says {source:?}");
    let home = std::env::var_os("CARGO_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").expect("a home")).join(".cargo"));
    let sources = home.join("registry/src");
    fs::read_dir(&sources)
        .expect("cargo's registry sources")
        .filter_map(Result::ok)
        .map(|e| e.path().join(format!("letkajenkka-{version}")).join("conformance"))
        .find(|p| p.is_dir())
        .unwrap_or_else(|| panic!("letkajenkka {version} is unpacked under {} with its conformance/", sources.display()))
}

/// `(name, query, error, plain names)` of every case in the suite that is a path of names or an
/// error. A case of any other shape is not one a plain reference reader answers.
fn cases() -> Vec<(String, String, Option<String>, Option<Vec<String>>)> {
    let dir = conformance_dir();
    let mut out = Vec::new();
    for file in ["idents.toml", "selectors.toml", "sets.toml", "structure.toml"] {
        let text = fs::read_to_string(dir.join(file)).expect("a conformance file");
        let doc: toml_edit::DocumentMut = text.parse().expect("conformance toml");
        let Some(list) = doc.get("case").and_then(|c| c.as_array_of_tables()) else { continue };
        for case in list {
            let name = case.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let query = case.get("query").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let error = case.get("error").and_then(|v| v.as_str()).map(str::to_string);
            let plain = case.get("steps").and_then(|s| s.as_array()).and_then(|steps| {
                steps
                    .iter()
                    .map(|s| {
                        let t = s.as_inline_table()?;
                        let plain = t.get("selector")?.as_str()? == "ident" && !t.contains_key("constraints");
                        plain.then(|| t.get("value").and_then(|v| v.as_str()).map(str::to_string)).flatten()
                    })
                    .collect::<Option<Vec<_>>>()
            });
            out.push((name, query, error, plain));
        }
    }
    out
}

// Mutation: reading a reference by hand, splitting on the separator and trimming each part (the
// reader before the standard), accepts `foo :: bar`, `Foo::bar`, `foo::bar-baz` and `foo::::bar`,
// and this fails on each.
#[test]
fn every_conformance_case_that_is_a_refusal_is_refused_with_its_name() {
    let mut checked = 0;
    for (name, query, error, _) in cases() {
        let Some(want) = error else { continue };
        // An operation's own failures are the operation reader's, and an operation text is no path
        let got = names(&query);
        match got {
            Err(Refusal::Syntax(e)) => assert_eq!(e.name(), want, "{name}: `{query}`"),
            other => panic!("{name}: `{query}` should be refused as {want}, got {other:?}"),
        }
        checked += 1;
    }
    assert!(checked > 20, "the suite was read, {checked} refusals");
}

// Mutation: lowercasing or trimming a name before comparing it hides a spelling the standard
// refuses; the plain cases below are exactly the names as written.
#[test]
fn every_conformance_case_that_is_a_plain_path_gives_its_names() {
    let mut checked = 0;
    for (name, query, _, plain) in cases() {
        let Some(want) = plain else { continue };
        let got: Vec<String> = names(&query).unwrap_or_else(|e| panic!("{name}: {e:?}")).iter().map(|s| (*s).to_string()).collect();
        assert_eq!(got, want, "{name}: `{query}`");
        checked += 1;
    }
    assert!(checked > 5, "the suite was read, {checked} plain paths");
}

// Mutation: reading the first operand of `names` as a path whatever its shape lets a set through.
#[test]
fn a_set_a_wildcard_a_constraint_and_an_operation_are_no_plain_path() {
    for text in ["foo::{a, b}", "foo::*", "foo::**", "foo::<shape>", "join(foo::a)", "foo::3", "foo::1..2"] {
        assert_eq!(names(text), Err(Refusal::NotPlain), "{text}");
    }
}

// Mutation: matching `refsto(` by prefix, as the expression reader did, takes `refsto(a::b)::c`
// and `refsto(a::b` for the operation; the standard parses the first as a call written as a step
// and refuses the second as unterminated.
#[test]
fn an_operation_is_read_by_the_standard_with_exactly_one_argument() {
    assert_eq!(operation("refsto(reg::spike::foo)", "refsto"), Some("reg::spike::foo"));
    assert_eq!(operation("refsto(a::b, c::d)", "refsto"), None);
    assert_eq!(operation("refsto()", "refsto"), None);
    assert_eq!(operation("refsto(a::b)::c", "refsto"), None);
    assert_eq!(operation("refsto(a::b", "refsto"), None);
    assert_eq!(operation("pathof(a::b)", "refsto"), None);
}

// Mutation: folding a directory's name up to its first dot, as a file's is, names `v1.2` as `v1`;
// folding a file's whole name keeps the extension a reference never writes.
#[test]
fn a_file_is_named_up_to_its_first_dot_and_a_directory_by_its_whole_name() {
    let entry = |name: &str, is_dir: bool| fold_entry(name, is_dir).map(|f| f.to_string());
    assert_eq!(entry("DESIGN.md.tmpl", false).as_deref(), Some("design"));
    assert_eq!(entry("my-crate", true).as_deref(), Some("my_crate"));
    assert_eq!(entry("v1.2", true), None);
    assert_eq!(entry(".hidden", false), None);
}

// Mutation: cutting the text at its first separator by hand gives `canon` for `canon::Foo` and
// `canon ` for `canon :: foo`, both of which the standard refuses, and `{a, b}` for a set.
#[test]
fn a_root_is_the_first_name_of_a_text_the_standard_reads_as_a_path() {
    assert_eq!(root("canon::foo::bar"), Some("canon"));
    assert_eq!(root("abi_count"), Some("abi_count"));
    for text in ["canon::Foo", "canon :: foo", "{a, b}::c", "*::c", "join(a::b)", "3::a", ""] {
        assert_eq!(root(text), None, "{text}");
    }
}

// Mutation: dropping the hyphen-to-underscore fold from `heading_name`, or keeping the `_` out of
// the forge slug, gives `no_std-no-alloc`, which is no name.
#[test]
fn a_heading_is_named_by_its_forge_slug_folded() {
    assert_eq!(heading_name("The four lanes").as_deref(), Some("the_four_lanes"));
    assert_eq!(heading_name("no_std, no alloc").as_deref(), Some("no_std_no_alloc"));
    assert_eq!(heading_name("Warm's objective").as_deref(), Some("warms_objective"));
    assert_eq!(heading_name("2026 plan"), None);
    assert_eq!(forge_slug("no_std, no alloc"), "no_std-no-alloc");
}

// Mutation: the old reader took `seed::DESIGN::#the-four-lanes`; each spelling below parsed there.
#[test]
fn the_spelling_before_the_standard_is_refused_naming_the_migration() {
    for old in [
        "seed::DESIGN::844",
        "seed::DESIGN::#the-four-lanes",
        "seed::design::#the_four_lanes",
        "mock::design.md::12",
        "mock::my-crate::design::12",
        "mock :: design :: 12",
    ] {
        let why = FileRef::read(old).unwrap_err();
        assert!(matches!(why, Refusal::Syntax(_)), "{old}: {why:?}");
        assert!(why.say(old).contains(MIGRATION), "{old}");
    }
}

#[test]
fn a_citation_is_a_root_a_path_and_an_anchor() {
    let line = FileRef::read("mock::crates::numeric::design::12").unwrap();
    assert_eq!(line.root, "mock");
    assert_eq!(line.names, ["crates", "numeric", "design"]);
    assert_eq!(line.lines, Some((12, 12)));
    assert_eq!(line.anchor(), Anchor::Lines(12, 12));
    assert_eq!(line.path_text(), "crates/numeric/design");
    assert_eq!(line.render(), "mock::crates::numeric::design::12");

    let span = FileRef::read("seed::design::890..920").unwrap();
    assert_eq!(span.anchor(), Anchor::Lines(890, 920));
    assert_eq!(span.render(), "seed::design::890..920");

    let head = FileRef::read("seed::design::the_four_lanes").unwrap();
    assert_eq!(head.anchor(), Anchor::Heading("the_four_lanes".to_string()));
    assert_eq!(head.path_text(), "design");
    assert_eq!(head.render(), "seed::design::the_four_lanes");
}

// Mutation: accepting a two-step text as a citation makes `std::mem` and `ns::slug` citations.
#[test]
fn a_text_with_no_path_or_no_anchor_is_no_citation() {
    for text in ["std::mem", "seed", "seed::design::0", "seed::design::5..2", "seed::*::1", "seed::{a, b}::1", "seed::design::<x>"] {
        assert!(FileRef::parse(text).is_none(), "{text}");
    }
}

fn tree(files: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for f in files {
        let p = dir.path().join(f);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "# Title\n\n## The four lanes\n\ntext\n").unwrap();
    }
    dir
}

// Mutation: matching an entry by its exact name, as the reader before the standard did for a
// citation written with its extension, finds nothing for `design`.
#[test]
fn a_name_reaches_the_file_whatever_it_calls_itself() {
    let dir = tree(&["DESIGN.md.tmpl", "crates/my-crate/NOTES.md", "crates/numeric/design.md"]);
    let at = |t: &str| FileRef::read(t).unwrap().locate(dir.path());
    assert_eq!(at("mock::design::12"), Location::Found(dir.path().join("DESIGN.md.tmpl")));
    assert_eq!(at("mock::crates::my_crate::notes::3"), Location::Found(dir.path().join("crates/my-crate/NOTES.md")));
    assert_eq!(at("mock::crates::numeric::design::the_four_lanes"), Location::Found(dir.path().join("crates/numeric/design.md")));
    assert_eq!(at("mock::crates::numeric::missing::1"), Location::Missing);
    assert_eq!(at("mock::crates::numeric::1..4"), Location::Missing);
}

// Mutation: taking the first match, or the last, where several files fold to one name.
#[test]
fn several_files_folding_to_one_name_are_ambiguous_and_none_is_picked() {
    let dir = tree(&["DESIGN.md", "design.md.tmpl", "other/readme.md"]);
    let got = FileRef::read("mock::design::1").unwrap().locate(dir.path());
    assert_eq!(got, Location::Ambiguous(vec!["DESIGN.md".to_string(), "design.md.tmpl".to_string()]));
}

// Mutation: reading the whole tail as a path, so a heading is looked for as a file.
#[test]
fn the_last_name_of_a_heading_citation_is_the_heading_and_not_a_file() {
    let dir = tree(&["design.md", "design/the_four_lanes.md"]);
    let got = FileRef::read("mock::design::the_four_lanes").unwrap().locate(dir.path());
    assert_eq!(got, Location::Found(dir.path().join("design.md")));
}

// Mutation: matching a heading by the old project slug (every non-alphanumeric run a hyphen), which
// reads `Warm's objective` as `warm_s_objective`, or ignoring fences, finds the comment line.
#[test]
fn a_heading_is_found_by_its_folded_forge_slug_outside_a_fence() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("d.md");
    fs::write(&file, "```\n# the_four_lanes\n```\n# Intro\n\n## Warm's objective\n\n## The four lanes\n").unwrap();
    assert_eq!(heading_in(&file, "the_four_lanes"), Some((8, "the-four-lanes".to_string())));
    assert_eq!(heading_in(&file, "warms_objective"), Some((6, "warms-objective".to_string())));
    assert_eq!(heading_in(&file, "warm_s_objective"), None);
    assert_eq!(resolve_anchor(&file, &Anchor::Lines(3, 8)), Some(3));
    assert_eq!(resolve_anchor(&file, &Anchor::Lines(3, 9)), None);
    assert_eq!(resolve_anchor(&file, &Anchor::Heading("nothing".to_string())), None);
}

#[test]
fn a_refusal_of_the_kind_the_standard_names_is_the_kind_the_error_carries() {
    assert!(matches!(FileRef::read("a::B::1"), Err(Refusal::Syntax(e)) if e.kind == Kind::Uppercase));
}

// Seven headings as a forge anchors them, which is what `forge_slug` has to give for a reference
// to name a heading by the anchor in the address bar. The first four classes (inner hyphen,
// underscore, dropped dot, dropped punctuation) are the ones a project-made slug missed. Each row
// was taken from the forge's markdown endpoint, and the name beside it is what the heading is
// called in a reference: the same slug folded, hyphens to underscores.
//
// Mutation: dropping `-` and `_` from what the slug keeps, or turning every punctuation run into a
// hyphen (`warm-s-objective`), fails the rows that hold either.
#[test]
fn the_forge_slug_of_a_heading_matches_the_forge_and_its_reference_name_is_that_folded() {
    let table = [
        ("Well-known types", "well-known-types", "well_known_types"),
        ("Trait-contract crates", "trait-contract-crates", "trait_contract_crates"),
        ("no_std, no alloc", "no_std-no-alloc", "no_std_no_alloc"),
        ("Pre-1.0 stability", "pre-10-stability", "pre_10_stability"),
        ("the_sweep probe", "the_sweep-probe", "the_sweep_probe"),
        ("Warm's objective", "warms-objective", "warms_objective"),
        ("R(V), the drift class", "rv-the-drift-class", "rv_the_drift_class"),
    ];
    for (heading, forge, name) in table {
        assert_eq!(forge_slug(heading), forge, "`{heading}` is `{forge}` in the address bar");
        assert_eq!(heading_name(heading).as_deref(), Some(name), "`{heading}`");
    }
}

// The control: a forge slug is no name until it is folded, since a hyphen is no part of one.
#[test]
fn a_forge_slug_with_its_hyphens_is_no_name_until_it_is_folded() {
    assert!(names("seed::design::warms-objective").is_err());
    assert!(names("seed::design::warms_objective").is_ok());
}

// GitHub's slugger lowercases, deletes every character but the space, the hyphen and the ones it
// keeps (the next test), and then turns each remaining space into one hyphen: runs are not
// collapsed, and nothing is trimmed (`mock/research/202611111500_github-heading-anchors.md`). Each
// row below is that algorithm applied by hand to the heading.
//
// Mutation: collapsing a run of spaces, trimming a trailing hyphen, or treating a tab or a
// no-break space as a space, each fails a row.
#[test]
fn the_forge_slug_keeps_every_space_as_a_hyphen_and_trims_nothing() {
    let table = [
        ("a  b", "a--b"),
        ("a   b", "a---b"),
        ("Foo -", "foo--"),
        ("a - b", "a---b"),
        ("-lead", "-lead"),
        ("trail-", "trail-"),
        ("C++ & Rust", "c--rust"),
        ("Tab\tbed", "tabbed"),
        ("no\u{a0}break", "nobreak"),
        ("under_score", "under_score"),
    ];
    for (heading, slug) in table {
        assert_eq!(forge_slug(heading), slug, "{heading:?}");
    }
}

// The slugger's generator deletes the other numbers, the punctuation but the connector kind, the
// symbols, the controls, the format characters and the separators, less anything alphabetic, the
// space and the hyphen; it keeps the rest. One row for each class the two readings of "letters and
// numbers" disagree on, and one for each kept class beside them.
//
// Mutation: keeping what `char::is_alphanumeric` keeps, as the slug did, deletes the combining
// mark and the connector and keeps the superscript two; dropping the alphabetic test deletes the
// circled letter.
#[test]
fn the_forge_slug_keeps_marks_and_connectors_and_deletes_other_numbers() {
    let table = [
        ("e\u{301}", "e\u{301}"),
        ("a\u{203f}b", "a\u{203f}b"),
        ("x\u{b2}", "x"),
        ("\u{663}", "\u{663}"),
        ("\u{2160}", "\u{2170}"),
        ("\u{24b6}", "\u{24d0}"),
        ("a\u{2192}b", "ab"),
        ("a\u{200d}b", "ab"),
    ];
    for (heading, slug) in table {
        assert_eq!(forge_slug(heading), slug, "{heading:?}");
    }
}

// A pass that resolves many citations opens a scope, and a directory is listed once in it however
// many citations walk through it. Outside a scope nothing is held, so a file made between two calls
// is found.
//
// Mutation: no cache lists the shared directory once for each of the sixty citations, and a cache
// kept past the scope misses the file made after it.
#[test]
fn a_directory_is_listed_once_in_a_scope_and_never_held_outside_one() {
    let dir = tree(&["crates/my-crate/NOTES.md", "crates/other/NOTES.md", "DESIGN.md"]);
    let cites: Vec<FileRef> = (0 .. 60)
        .map(|i| FileRef::read(&format!("mock::crates::{}::notes::{}", if i % 2 == 0 { "my_crate" } else { "other" }, i + 1)).unwrap())
        .collect();

    let before = dir_reads();
    let scope = cache_scope();
    for c in &cites {
        assert!(matches!(c.locate(dir.path()), Location::Found(_)));
    }
    let in_scope = dir_reads() - before;
    // the root, `crates`, and the two crate directories
    assert_eq!(in_scope, 4, "each directory listed once");
    drop(scope);

    let before = dir_reads();
    for c in &cites {
        let _ = c.locate(dir.path());
    }
    assert!(dir_reads() - before >= 60, "no scope, no cache");

    let _scope = cache_scope();
    let late = FileRef::read("mock::crates::other::late::1").unwrap();
    assert_eq!(late.locate(dir.path()), Location::Missing);
    drop(_scope);
    fs::write(dir.path().join("crates/other/LATE.md"), "x\n").unwrap();
    assert!(matches!(late.locate(dir.path()), Location::Found(_)), "a scope that ended holds nothing");
}

// Scopes nest: the inner one ending does not drop what the outer holds.
#[test]
fn a_nested_scope_leaves_the_outer_cache_standing() {
    let dir = tree(&["a/B.md"]);
    let c = FileRef::read("mock::a::b::1").unwrap();
    let outer = cache_scope();
    assert!(matches!(c.locate(dir.path()), Location::Found(_)));
    let inner = cache_scope();
    drop(inner);
    let before = dir_reads();
    assert!(matches!(c.locate(dir.path()), Location::Found(_)));
    assert_eq!(dir_reads(), before, "still held");
    drop(outer);
}

// A list of references is split at the commas of its own level by the syntax, so a comma inside a set
// is not a boundary and a list written without the space after the comma is still a list.
//
// Mutation: `split(", ")` reads `a::b,c::d` as one item and `{a, b}` as two broken ones.
#[test]
fn a_list_of_references_is_split_by_the_syntax() {
    assert_eq!(list("a::b, c::d").collect::<Vec<_>>(), ["a::b", "c::d"]);
    assert_eq!(list("a::b,c::d").collect::<Vec<_>>(), ["a::b", "c::d"]);
    assert_eq!(list("a::{b, c}, d::e").collect::<Vec<_>>(), ["a::{b, c}", "d::e"]);
    assert_eq!(list("  a::b  ").collect::<Vec<_>>(), ["a::b"]);
    assert_eq!(list("").count(), 0);
    assert_eq!(list(" , ,").count(), 0, "an empty item is no item");
}
