//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What the provenance check says of a citation whose file shares its stem with another, and of one
//! naming an extension where nothing needs it (letkajenkka `SPEC.md` 9.1).

use std::fs;

use super::*;

/// The findings of a row of `law` citing each of `cited` through a `ref[]` field, against a root
/// `panel` holding `files`.
fn findings(files: &[&str], cited: &str) -> Vec<RegistryFinding> {
    let dir = tempfile::tempdir().unwrap();
    for f in files {
        let p = dir.path().join("panel").join(f);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "one\ntwo\n").unwrap();
    }
    let ns = RegistryNamespace {
        key:         "law".into(),
        title:       None,
        description: None,
        value_field: None,
        group_by:    None,
        fields:      vec![RegistryField {
            name:        "rests_on".into(),
            r#type:      "ref[]".into(),
            required:    false,
            description: None,
            visibility:  FieldVisibility::Public,
            values:      Vec::new(),
            min:         None,
            max:         None,
        }],
    };
    let row = RegistryRow {
        slug:      "seam".into(),
        namespace: "law".into(),
        source:    PathBuf::from("t.toml"),
        fields:    [("rests_on".to_string(), cited.to_string())]
            .into_iter()
            .collect(),
    };
    let mut reg = Registry::default();
    let q = row.qualified();
    reg.by_namespace.insert("law".into(), vec![q.clone()]);
    reg.rows.insert(q, row);
    let roots = BTreeMap::from([("panel".to_string(), "panel".to_string())]);
    let frozen = BTreeSet::from(["panel".to_string()]);
    validate_provenance(dir.path(), &roots, &frozen, &reg, std::slice::from_ref(&ns))
}

const PROBES: &[&str] = &[
    "probes/unblock_value.out",
    "probes/unblock_value.sh",
    "probes/alone.md",
    "probes/run",
    "probes/run.sh",
];

// Mutation: the message of the reader before this, which said to rename one of the two, holds no
// spelling; offering a spelling for every file names `run<>` for the file with no extension.
#[test]
fn an_ambiguous_citation_is_told_the_spelling_of_each_file_where_only_extensions_differ() {
    let found = findings(PROBES, "panel::probes::unblock_value::1");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].kind, "ambiguous-provenance");
    assert!(
        found[0]
            .message
            .contains("panel::probes::unblock_value<out>::1, panel::probes::unblock_value<sh>::1"),
        "{}",
        found[0].message
    );

    let found = findings(PROBES, "panel::probes::run::1");
    assert_eq!(found[0].kind, "ambiguous-provenance");
    assert!(
        found[0]
            .message
            .contains("differ by more than an extension"),
        "{}",
        found[0].message
    );
}

// Mutation: treating `NeedlessExtension` as found lets `alone<md>` pass; no finding for the two
// spellings that name the file they need to would leave the first assertion with one.
#[test]
fn a_named_extension_passes_where_it_is_needed_and_is_refused_where_it_is_not() {
    {
        let f = findings(PROBES, "panel::probes::unblock_value<out>::1");
        assert!(f.is_empty(), "{f:?}");
    }
    {
        let f = findings(PROBES, "panel::probes::run<sh>::2");
        assert!(f.is_empty(), "{f:?}");
    }
    let found = findings(PROBES, "panel::probes::alone<md>::1");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].kind, "needless-extension");
    assert!(
        found[0].message.contains("`panel::probes::alone::1`"),
        "{}",
        found[0].message
    );
    {
        let f = findings(PROBES, "panel::probes::alone::1");
        assert!(f.is_empty(), "{f:?}");
    }
}

// Mutation: reading a misplaced shape as no citation at all, as `NotPlain`, gives a finding that
// does not say where the one shape a citation takes goes.
#[test]
fn a_shape_on_another_step_is_malformed_and_says_where_the_extension_goes() {
    let found = findings(PROBES, "panel::probes<md>::alone::1");
    assert_eq!(found[0].kind, "malformed-provenance");
    assert!(
        found[0].message.contains("on the step naming the file"),
        "{}",
        found[0].message
    );
}
