//! Where a definition can sit, as tables the reach tests run over.

/// `(label, root text, files)`: one place a definition can sit.
pub(super) type Placement = (&'static str, String, Vec<(&'static str, String)>);

/// One definition of every kind the lint compares, with its finding kind.
pub(super) const ITEMS: &[(&str, &str)] = &[
    ("struct Dup;", "type"),
    ("pub struct Dup { pub x: u8 }", "type"),
    ("enum Dup { A }", "type"),
    ("trait Dup {}", "type"),
    ("union Dup { a: u8 }", "type"),
    ("const DUP: u32 = 7;", "constant"),
    ("pub const DUP: u32 = 7;", "constant"),
    ("static DUP: u32 = 7;", "constant"),
    ("pub static mut DUP: u32 = 7;", "constant"),
];

/// Every place a definition counts from: `(label, root, files)`, `{}` the item.
pub(super) fn counted_placements(item: &str) -> Vec<Placement> {
    let i = item.to_string();
    vec![
        ("root", i.clone(), vec![]),
        ("module file", "mod m;".into(), vec![(
            "src/m.rs",
            i.clone(),
        )]),
        ("mod.rs", "mod m;".into(), vec![("src/m/mod.rs", i.clone())]),
        ("pub mod", "pub mod m;".into(), vec![(
            "src/m.rs",
            i.clone(),
        )]),
        ("pub(crate) mod", "pub(crate) mod m;".into(), vec![(
            "src/m.rs",
            i.clone(),
        )]),
        ("nested file", "mod a;".into(), vec![
            ("src/a.rs", "mod b;".into()),
            ("src/a/b.rs", i.clone()),
        ]),
        ("nested mod.rs", "mod a;".into(), vec![
            ("src/a/mod.rs", "mod b;".into()),
            ("src/a/b.rs", i.clone()),
        ]),
        ("inline mod", format!("mod m {{ {item} }}"), vec![]),
        (
            "nested inline mod",
            format!("mod m {{ mod n {{ {item} }} }}"),
            vec![],
        ),
        ("file under an inline mod", "mod m { mod n; }".into(), vec![
            ("src/m/n.rs", i.clone()),
        ]),
        (
            "path attribute",
            "#[path = \"sys/linux.rs\"] mod sys;".into(),
            vec![("src/sys/linux.rs", i.clone())],
        ),
        (
            "path attribute in an inline mod",
            "mod m { #[path = \"x.rs\"] mod n; }".into(),
            vec![("src/m/x.rs", i.clone())],
        ),
        (
            "path attribute in an inline mod, non-mod-rs parent",
            "mod f;".into(),
            vec![
                ("src/f.rs", "mod t { #[path = \"x.rs\"] mod n; }".into()),
                ("src/f/t/x.rs", i.clone()),
            ],
        ),
        (
            "inline mod without a path, non-mod-rs parent",
            "mod f;".into(),
            vec![("src/f.rs", "mod t { mod n; }".into()), ("src/f/t/n.rs", i.clone())],
        ),
        (
            "path attribute in an inline mod, mod.rs parent",
            "mod f;".into(),
            vec![
                ("src/f/mod.rs", "mod t { #[path = \"x.rs\"] mod n; }".into()),
                ("src/f/t/x.rs", i.clone()),
            ],
        ),
        (
            "child of a path attribute file",
            "#[path = \"sys/like_mod.rs\"] mod sys;".into(),
            vec![("src/sys/like_mod.rs", "mod child;".into()), ("src/sys/child.rs", i.clone())],
        ),
        (
            "cfg feature module file",
            "#[cfg(feature = \"x\")] mod m;".into(),
            vec![("src/m.rs", i.clone())],
        ),
        (
            "cfg not test inline",
            format!("#[cfg(not(test))] mod m {{ {item} }}"),
            vec![],
        ),
        (
            "cfg any test inline",
            format!("#[cfg(any(test, unix))] mod m {{ {item} }}"),
            vec![],
        ),
        (
            "cfg latest feature",
            format!("#[cfg(feature = \"latest\")] mod m {{ {item} }}"),
            vec![],
        ),
        (
            "cfg_attr is not cfg",
            format!("#[cfg_attr(test, derive(Debug))] {item}"),
            vec![],
        ),
        ("file nothing declares", "".into(), vec![(
            "src/loose.rs",
            i.clone(),
        )]),
    ]
}

/// Every place a definition is test code and so defines nothing.
pub(super) fn test_placements(item: &str) -> Vec<Placement> {
    let i = item.to_string();
    vec![
        (
            "item under cfg(test)",
            format!("#[cfg(test)] {item}"),
            vec![],
        ),
        (
            "inline test mod",
            format!("#[cfg(test)] mod tests {{ {item} }}"),
            vec![],
        ),
        (
            "inner cfg(test) in an inline mod",
            format!("mod t {{ #![cfg(test)] {item} }}"),
            vec![],
        ),
        ("test mod file", "#[cfg(test)] mod tests;".into(), vec![(
            "src/tests.rs",
            i.clone(),
        )]),
        ("test mod.rs", "#[cfg(test)] mod tests;".into(), vec![(
            "src/tests/mod.rs",
            i.clone(),
        )]),
        (
            "cfg all test",
            "#[cfg(all(test, unix))] mod tests;".into(),
            vec![("src/tests.rs", i.clone())],
        ),
        (
            "file opening with an inner cfg(test)",
            "mod m;".into(),
            vec![("src/m.rs", format!("#![cfg(test)]\n{item}"))],
        ),
        ("child of a test file", "#[cfg(test)] mod t;".into(), vec![
            ("src/t.rs", "mod u;".into()),
            ("src/t/u.rs", i.clone()),
        ]),
        (
            "child of a test file, three deep",
            "#[cfg(test)] mod t;".into(),
            vec![
                ("src/t.rs", "mod u;".into()),
                ("src/t/u.rs", "mod v;".into()),
                ("src/t/u/v.rs", i.clone()),
            ],
        ),
        (
            "file under an inline test mod",
            "#[cfg(test)] mod tests { mod c; }".into(),
            vec![("src/tests/c.rs", i.clone())],
        ),
        (
            "path attribute in an inline test mod, non-mod-rs parent",
            "mod f;".into(),
            vec![
                (
                    "src/f.rs",
                    "#[cfg(test)] mod t { #[path = \"x.rs\"] mod n; }".into(),
                ),
                ("src/f/t/x.rs", i.clone()),
            ],
        ),
        (
            "path attribute in an inline test mod",
            "#[cfg(test)] mod t { #[path = \"x.rs\"] mod n; }".into(),
            vec![("src/t/x.rs", i.clone())],
        ),
        (
            "test mod behind a path attribute",
            "#[cfg(test)]\n#[path = \"x.rs\"]\nmod m;".into(),
            vec![("src/x.rs", i.clone())],
        ),
        (
            "child of a test path attribute file",
            "#[cfg(test)] #[path = \"sys/m.rs\"] mod m;".into(),
            vec![("src/sys/m.rs", "mod c;".into()), ("src/sys/c.rs", i.clone())],
        ),
    ]
}

/// Roots other than `src/lib.rs`: `(label, where the root sits, root, files)`.
pub(super) type Rooted = (
    &'static str,
    &'static str,
    String,
    Vec<(&'static str, String)>,
);

pub(super) fn rooted_placements(item: &str) -> Vec<Rooted> {
    let i = item.to_string();
    vec![
        ("main.rs only", "src/main.rs", "mod m;".into(), vec![(
            "src/m.rs",
            i.clone(),
        )]),
        (
            "bin file",
            "src/bin/tool.rs",
            "mod m { mod n; }".into(),
            vec![("src/bin/m/n.rs", i.clone())],
        ),
        (
            "bin directory",
            "src/bin/tool/main.rs",
            "mod m;".into(),
            vec![("src/bin/tool/m.rs", i.clone())],
        ),
        (
            "bin root itself",
            "src/bin/tool.rs",
            item.to_string(),
            vec![],
        ),
    ]
}
