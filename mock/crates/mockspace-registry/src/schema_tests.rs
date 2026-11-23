//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The generated schemas: where a `values` list lands, and the editor binding.

#[cfg(test)]
mod closed_value_set_placement {
    //! Where a `values` list lands in the emitted schema, for every field type
    //! it can sit on.
    //!
    //! The first version put the `enum` next to `"type": "array"` for all four
    //! array shapes, which no array value can satisfy, so every row carrying
    //! such a field would have failed with an opaque validator error. It was
    //! tested on `string` alone, where the placement happens to be right: one
    //! of six shapes, and the five untested ones were the broken ones.
    use super::super::*;
    use crate::{FieldVisibility, RegistryField, RegistryNamespace};

    fn schema_for(ty: &str, values: &[&str]) -> String {
        schema_with(ty, values, None, None)
    }

    fn schema_with(ty: &str, values: &[&str], min: Option<i64>, max: Option<i64>) -> String {
        let ns = RegistryNamespace {
            key:         "probe".into(),
            title:       None,
            description: None,
            value_field: None,
            group_by:    None,
            path: Vec::new(),
            fields:      vec![RegistryField {
                name: "f".into(),
                r#type: ty.into(),
                required: false,
                description: None,
                visibility: FieldVisibility::Public,
                values: values.iter().map(|s| s.to_string()).collect(),
                min,
                max,
            }],
        };
        let tmp = mockspace_manifest::scratch::tempdir().unwrap();
        let n = generate_schemas(tmp.path(), &tmp.path().join("mock"), &[ns]);
        assert!(n > 0, "control: the generator wrote something");
        let dir = tmp
            .path()
            .join("target")
            .join("mockspace")
            .join("registry-schemas");
        let mut out = String::new();
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            out.push_str(&std::fs::read_to_string(e.path()).unwrap());
        }
        out
    }

    /// The `f` property's own body. The document wraps every namespace in
    /// `"type": "array", "items": {...}`, so a search over the whole file
    /// cannot tell a field's array from that one, which is how the first
    /// version of this test managed to pass on a scalar.
    fn property(schema: &str) -> String {
        let i = schema.find("\"f\": {").expect("the probe field is emitted");
        let rest = &schema[i ..];
        let j = rest.find("\n        }").expect("its body closes");
        rest[.. j].to_string()
    }

    #[test]
    fn an_arrays_enum_constrains_its_members_and_a_scalars_constrains_itself() {
        // each member in the field's own type: an integer's or a boolean's written as a
        // string would match no value of that type, and every row would fail
        for (ty, json, values, members) in [
            ("string", "\"type\": \"string\"", ["a", "b"], "\"a\", \"b\""),
            ("integer", "\"type\": \"integer\"", ["1", "-2"], "1, -2"),
            ("integer", "\"type\": \"integer\"", ["+5", "05"], "5, 5"),
            (
                "boolean",
                "\"type\": \"boolean\"",
                ["true", "false"],
                "true, false",
            ),
        ] {
            let p = property(&schema_for(ty, &values));
            assert_eq!(
                p,
                format!("\"f\": {{\n          {json},\n          \"enum\": [{members}]"),
                "{ty}: a scalar's enum sits on the property, in its type"
            );
        }
        assert!(
            property(&schema_for("string", &["3", "true"]))
                .ends_with("\"enum\": [\"3\", \"true\"]"),
            "a string's members stay strings, however they read"
        );
        for ty in ["string[]", "ref[]"] {
            let p = property(&schema_for(ty, &["a", "b"]));
            assert_eq!(
                p,
                "\"f\": {\n          \"type\": \"array\", \"items\": { \"type\": \"string\", \"enum\": [\"a\", \"b\"] }",
                "{ty}: an array's enum constrains its members, so it goes inside items, \
                 never beside `type: array` where no array value can satisfy it"
            );
        }
    }

    #[test]
    fn an_integers_bounds_sit_on_its_type_and_one_without_has_none() {
        assert_eq!(
            property(&schema_with("integer", &[], Some(0), Some(10))),
            "\"f\": {\n          \"type\": \"integer\", \"minimum\": 0, \"maximum\": 10"
        );
        assert_eq!(
            property(&schema_with("integer", &[], None, Some(-3))),
            "\"f\": {\n          \"type\": \"integer\", \"maximum\": -3"
        );
        assert_eq!(
            property(&schema_for("integer", &[])),
            "\"f\": {\n          \"type\": \"integer\""
        );
    }

    #[test]
    fn no_values_means_no_enum_at_any_type() {
        // the control. without it every arm above passes on a generator that
        // emits an enum unconditionally.
        for ty in ["string", "integer", "boolean", "string[]", "ref[]"] {
            let p = property(&schema_for(ty, &[]));
            assert!(!p.contains("enum"), "{ty}: {p}");
        }
    }
}

/// The editor binding is written where nobody else's file is.
#[cfg(test)]
mod the_editor_binding_is_not_written_over_somebody_elses {
    use super::super::*;

    #[test]
    fn a_taplo_file_that_is_not_ours_survives_a_first_run() {
        // `.taplo.toml` is an ordinary file in a Rust repository, and this is
        // the third instance of one class: generation writing over what it
        // found. The other two were `.claude/settings.json` and the documents
        // directory, and both were fixed by reading the file before writing
        // it. This one was not.
        let d = mockspace_manifest::scratch::tempdir().unwrap();
        let theirs = d.path().join(".taplo.toml");
        let before = "[[rule]]\ninclude = [\"my.toml\"]\nschema.path = \"mine.json\"\n";
        fs::write(&theirs, before).unwrap();

        assert!(
            !write_taplo_if_ours(&theirs, "# ours\n"),
            "somebody else's file was written to"
        );
        assert_eq!(fs::read_to_string(&theirs).unwrap(), before);
    }

    #[test]
    fn a_taplo_file_we_wrote_is_updated_and_an_absent_one_is_created() {
        // The permit half. A guard that refused every existing file would pass
        // the test above and freeze the binding at whatever the first run
        // produced, so a namespace added later would never reach an editor.
        let d = mockspace_manifest::scratch::tempdir().unwrap();
        let ours = d.path().join(".taplo.toml");

        assert!(
            write_taplo_if_ours(&ours, "# absent\n"),
            "an absent file was not created"
        );
        assert_eq!(fs::read_to_string(&ours).unwrap(), "# absent\n");

        let marked = format!("# {}\n# old\n", crate::GENERATED_MARKER);
        fs::write(&ours, &marked).unwrap();
        let next = format!("# {}\n# new\n", crate::GENERATED_MARKER);
        assert!(
            write_taplo_if_ours(&ours, &next),
            "our own file was not updated"
        );
        assert_eq!(fs::read_to_string(&ours).unwrap(), next);
    }

    #[test]
    fn the_header_the_first_version_wrote_is_still_recognised_as_ours() {
        // A repository generated before the shared marker went into the header
        // carries the plain sentence instead. Reading that as somebody else's
        // file would freeze its binding forever, on exactly the repositories
        // that have been using this longest.
        let d = mockspace_manifest::scratch::tempdir().unwrap();
        let ours = d.path().join(".taplo.toml");
        fs::write(
            &ours,
            "# Generated by mockspace. Do not edit; change [[registry.namespace]]\n",
        )
        .unwrap();
        assert!(write_taplo_if_ours(&ours, "# next\n"));
        assert_eq!(fs::read_to_string(&ours).unwrap(), "# next\n");
    }
}
