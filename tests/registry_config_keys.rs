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
