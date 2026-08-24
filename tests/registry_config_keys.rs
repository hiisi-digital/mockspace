//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A key in `mockspace.toml` that mockspace does not read is reported, not eaten.
//!
//! The registry's row data is covered by generated schemas run through a TOML
//! validator, and `validate.rs` deliberately does not duplicate that. The config
//! file is covered by none of it: `serde` does not deny unknown fields there, so
//! an unimplemented key or a typo is read and discarded in silence.
//!
//! The case that motivated this is real and is in the workspace: twelve namespace
//! declarations carrying `prefix`, a field mockspace has never had, discarded
//! twelve times with nothing said.

use mockspace::registry::{config_unknown_keys, FINDING_KINDS};

const KNOWN_GOOD: &str = r#"
[[registry.namespace]]
key = "law"
title = "Laws"
description = "what holds"
value_field = "statement"
render = "page"
group_by = "domain"

[[registry.namespace.field]]
name = "statement"
type = "string"
required = true
description = "the law"
visibility = "public"
"#;

#[test]
fn a_config_using_only_known_keys_is_silent() {
    let found = config_unknown_keys(KNOWN_GOOD);
    assert!(
        found.is_empty(),
        "every key here is one the struct carries, so a report means the known-key \
         list has drifted from the struct: {found:?}"
    );
}

#[test]
fn the_prefix_case_that_motivated_this_is_reported() {
    let found = config_unknown_keys(
        r#"
[[registry.namespace]]
key = "law"
prefix = "LAW"
"#,
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].kind, "unknown-config-key");
    assert!(found[0].message.contains("prefix"), "{}", found[0].message);
    assert!(
        found[0].message.contains("law"),
        "the report names the key but not which namespace carries it: {}",
        found[0].message
    );
}

#[test]
fn a_typo_in_a_known_key_is_reported_too() {
    // The same silence covers a misspelling, which is the commoner case and the
    // harder one to notice, because the author believes the key is doing work.
    let found = config_unknown_keys(
        r#"
[[registry.namespace]]
key = "law"
value_feild = "statement"
"#,
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].message.contains("value_feild"), "{}", found[0].message);
}

#[test]
fn an_unknown_key_on_a_field_table_is_reported() {
    let found = config_unknown_keys(
        r#"
[[registry.namespace]]
key = "law"

[[registry.namespace.field]]
name = "statement"
unique = true
"#,
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].message.contains("law.statement"), "{}", found[0].message);
}

#[test]
fn a_config_with_no_registry_at_all_is_silent() {
    assert!(config_unknown_keys("project_name = \"x\"\n").is_empty());
    assert!(config_unknown_keys("").is_empty());
}

#[test]
fn a_config_that_does_not_parse_is_somebody_elses_error() {
    // Not this function's job to report malformed TOML, and returning findings
    // for it would attribute a parse failure to the registry.
    assert!(config_unknown_keys("[[[ not toml").is_empty());
}

/// The list this check reads is hand-kept, so it is constrained rather than trusted.
///
/// `NAMESPACE_KEYS` mirrors `RegistryNamespace`. If a field is added to the struct
/// and not to the list, every config using it is reported as unknown, which is a
/// false positive that reads exactly like a real finding. This catches that by
/// round-tripping a config that names every field the struct actually deserializes.
#[test]
fn the_known_key_list_has_not_drifted_from_the_struct() {
    // Deserialize KNOWN_GOOD through the real path. If serde accepts a key that
    // config_unknown_keys rejects, or vice versa, the two disagree.
    #[derive(serde::Deserialize)]
    struct Raw {
        registry: Reg,
    }
    #[derive(serde::Deserialize)]
    struct Reg {
        namespace: Vec<mockspace::registry::RegistryNamespace>,
    }
    let parsed: Raw =
        toml_edit::de::from_str(KNOWN_GOOD).expect("the fixture deserializes");
    let ns = &parsed.registry.namespace[0];

    assert_eq!(ns.key, "law");
    assert_eq!(ns.value_field.as_deref(), Some("statement"));
    assert_eq!(ns.group_by.as_deref(), Some("domain"));
    assert_eq!(ns.fields.len(), 1, "the field table did not deserialize");
    assert!(
        config_unknown_keys(KNOWN_GOOD).is_empty(),
        "serde reads these keys and the checker calls them unknown"
    );
}

/// The kind this produces is in the published list.
///
/// `FINDING_KINDS` carries a note saying nothing reads it. That is still true of
/// the severity map, and it is no longer true of the list's completeness: a kind
/// emitted and not listed now fails here.
#[test]
fn the_kind_this_produces_is_published() {
    let found = config_unknown_keys("[[registry.namespace]]\nkey = \"x\"\nnope = 1\n");
    assert_eq!(found.len(), 1);
    assert!(
        FINDING_KINDS.contains(&found[0].kind),
        "`{}` is produced and is not in FINDING_KINDS, so per-kind severity cannot \
         reach it",
        found[0].kind
    );
}

/// The list this check reads is caught against a real struct value, not only
/// against `KNOWN_GOOD`.
///
/// `the_known_key_list_has_not_drifted_from_the_struct` above proves
/// `NAMESPACE_KEYS` and `FIELD_KEYS` agree with `KNOWN_GOOD`, which is a third
/// hand-maintained list: nothing forces it to be updated in step with either
/// list, or with the struct they both claim to mirror. A field added to
/// `RegistryNamespace` or `RegistryField`, left off `NAMESPACE_KEYS`/
/// `FIELD_KEYS` and never written into `KNOWN_GOOD` passes that test
/// unnoticed, because `KNOWN_GOOD` never mentions the new field either.
///
/// This constructs a real `RegistryNamespace` populating every field it has,
/// serialises it with `serde` rather than typing it out by hand, and checks
/// the result. A field the struct actually carries reaches the emitted TOML
/// on its own; nothing here has to remember it exists.
#[test]
fn every_field_a_real_instance_carries_is_a_known_key() {
    #[derive(serde::Serialize)]
    struct SerReg {
        registry: SerRegBody,
    }
    #[derive(serde::Serialize)]
    struct SerRegBody {
        namespace: Vec<mockspace::registry::RegistryNamespace>,
    }

    let ns = mockspace::registry::RegistryNamespace {
        key:         "law".into(),
        title:       Some("Laws".into()),
        description: Some("what holds".into()),
        value_field: Some("statement".into()),
        render:      mockspace::registry::RenderMode::Page,
        group_by:    Some("domain".into()),
        fields:      vec![mockspace::registry::RegistryField {
            name:        "statement".into(),
            r#type:      "string".into(),
            required:    true,
            description: Some("the law".into()),
            visibility:  mockspace::registry::FieldVisibility::Public,
            values:      Vec::new(),
        }],
    };

    let text = toml_edit::ser::to_string_pretty(&SerReg {
        registry: SerRegBody {
            namespace: vec![ns],
        },
    })
    .expect("a real RegistryNamespace instance serialises");

    let found = config_unknown_keys(&text);
    assert!(
        found.is_empty(),
        "a field RegistryNamespace or RegistryField actually carries is reported \
         as unknown, so NAMESPACE_KEYS or FIELD_KEYS has drifted from the struct: \
         {found:?}\ngenerated config:\n{text}"
    );
}

// --- `[ref.roots.<name>]` ------------------------------------------------------

/// Every key `RawRefRoot` carries is accepted.
///
/// The control for the arm below: without it, a rejection is equally consistent
/// with a check that rejects everything in these tables.
#[test]
fn every_key_a_ref_root_declares_is_known() {
    let found = config_unknown_keys(
        r#"
[ref.roots.seed]
path = "mock/research/seed"
frozen = true
links = false
label = "Prior research"
internal = true
"#,
    );
    assert!(
        found.is_empty(),
        "a ref root using its whole declared surface must be clean, or the arm \
         below establishes nothing: {found:?}"
    );
}

/// A root-level key written one line too far down lands in a ref-root table,
/// where nothing reads it.
///
/// This is the case the check exists for and it is not hypothetical. `canon_paths`
/// went in below a `[ref.roots.*]` header, was read as a key of that root, and
/// was discarded in silence, so the feature it configures stayed off while the
/// config plainly said it was on. TOML gives a bare key to whichever table header
/// precedes it, and these tables sit near the top of a config, which is exactly
/// where root-level settings are also written.
#[test]
fn a_root_level_key_that_landed_in_a_ref_root_is_reported() {
    let found = config_unknown_keys(
        r#"
[ref.roots.seed]
path = "mock/research/seed"
frozen = true
canon_paths = ["mock/registry/*.toml"]
"#,
    );
    assert_eq!(found.len(), 1, "expected exactly one finding: {found:?}");
    assert_eq!(found[0].kind, "unknown-config-key");
    assert!(
        found[0].message.contains("canon_paths") && found[0].message.contains("ref.roots.seed"),
        "the finding names the key and the table it fell into: {}",
        found[0].message
    );
    assert!(
        found[0].message.contains("precedes it"),
        "and says why it happened, because the author's mistake is invisible in \
         the file: {}",
        found[0].message
    );
}

/// A ref root that is not a table is skipped rather than crashing.
#[test]
fn a_malformed_ref_roots_section_is_not_a_panic() {
    assert!(config_unknown_keys("[ref]\nroots = 3\n").is_empty());
    assert!(config_unknown_keys("ref = 3\n").is_empty());
}

/// The three shapes the first version of the `[ref.roots.*]` check could not
/// see, each measured at zero findings before this landed.
///
/// The middle one is the motivating defect's own home: `canon_paths` is a root
/// key, so a typo in it at the root was still discarded in silence while the
/// check that exists to end that silence reported nothing.
#[test]
fn the_silence_ends_at_the_root_and_inside_an_inline_table() {
    // a root written inline is the same configuration as a `[ref.roots.x]`
    // header, and `as_table` returned None for it
    let f = config_unknown_keys("[ref.roots]\nseed = { path = \"p\", canon_paths = [\"x\"] }\n");
    assert_eq!(f.len(), 1, "inline root: {f:?}");
    assert!(f[0].message.contains("canon_paths"), "{:?}", f[0]);

    // the document root, which nothing looked at
    let f = config_unknown_keys("canon_pathz = [\"x\"]\n");
    assert_eq!(f.len(), 1, "root typo: {f:?}");
    assert!(f[0].message.contains("canon_pathz"), "{:?}", f[0]);

    // `[ref]` itself carries only `roots`
    let f = config_unknown_keys("[ref]\nbogus = 1\n");
    assert_eq!(f.len(), 1, "ref level: {f:?}");
    assert!(f[0].message.contains("bogus"), "{:?}", f[0]);
}

/// The control, and it is the one that matters: a real config must produce
/// nothing. A root check that reports every section header would fail the gate
/// on every project in the workspace, which is a worse defect than the silence.
///
/// The two pin keys at the top are not decoration. Run against a real config
/// this reported both of them, because the launcher reads them and the engine
/// does not, and a fixture built only from what the engine knows would never
/// have shown it.
#[test]
fn a_real_config_is_clean_at_every_level() {
    let real = "\
mockspace_branch = \"dev\"\n\
mock_dir = \"mock\"\n\
project_name = \"probe\"\n\
canon_paths = [\"mock/registry/*.toml\"]\n\
src_dirs = [\"crates\"]\n\
\n[domain_kinds]\nx = \"y\"\n\
\n[lints.no-alloc]\ncommit = \"error\"\n\
\n[ref.roots.seed]\npath = \"mock/research/seed\"\nfrozen = true\nlinks = false\nlabel = \"Prior research\"\ninternal = true\n\
\n[[registry.namespace]]\nkey = \"law\"\n";
    let f = config_unknown_keys(real);
    assert!(f.is_empty(), "a real config must be clean: {f:?}");
}
