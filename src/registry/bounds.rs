//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A field's `min` and `max`, refused where they cannot mean what they say.

use super::*;

/// A bound declared on a field that is not an `integer`, or a `min` above its `max`.
///
/// Refused rather than ignored. The schema carries a bound only on an integer, so one
/// written on a string or a list would be dropped on the way and the field would read
/// as constrained while nothing held it, which is the failure `unknown_field_types`
/// exists for one step over. A `min` above the `max` admits no value at all, so every
/// row carrying the field would fail with a validator error that names the row rather
/// than the declaration.
pub fn misplaced_bounds(namespaces: &[RegistryNamespace]) -> Vec<RegistryFinding> {
    let mut out = Vec::new();
    for ns in namespaces {
        for f in &ns.fields {
            if f.min.is_none() && f.max.is_none() {
                continue;
            }
            let problem = if f.r#type != "integer" {
                Some(format!(
                    "is `{}`, and a bound is only held on an `integer`, so it would constrain nothing",
                    f.r#type
                ))
            } else if let (Some(lo), Some(hi)) = (f.min, f.max)
                && lo > hi
            {
                Some(format!(
                    "has `min` {lo} above `max` {hi}, which no value can satisfy"
                ))
            } else {
                None
            };
            if let Some(problem) = problem {
                out.push(RegistryFinding {
                    kind:    "misplaced-field-bound",
                    message: format!(
                        "namespace `{}` declares a bound on field `{}`, which {problem}.",
                        ns.key, f.name
                    ),
                    source:  None,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns(fields: &[(&str, Option<i64>, Option<i64>)]) -> Vec<RegistryNamespace> {
        vec![RegistryNamespace {
            key:         "probe".into(),
            title:       None,
            description: None,
            value_field: None,
            render:      RenderMode::Page,
            group_by:    None,
            fields:      fields
                .iter()
                .enumerate()
                .map(|(i, (ty, min, max))| {
                    RegistryField {
                        name:        format!("f{i}"),
                        r#type:      (*ty).into(),
                        required:    false,
                        description: None,
                        visibility:  FieldVisibility::Public,
                        values:      Vec::new(),
                        min:         *min,
                        max:         *max,
                    }
                })
                .collect(),
        }]
    }

    #[test]
    fn a_bound_on_an_integer_is_taken_whole_or_half() {
        // the control: without it the arms below pass on a check refusing every bound
        let found = misplaced_bounds(&ns(&[
            ("integer", Some(0), Some(10)),
            ("integer", Some(5), Some(5)),
            ("integer", None, Some(3)),
            ("integer", Some(-2), None),
            ("string", None, None),
        ]));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_bound_on_another_type_or_min_above_max_is_refused() {
        let found = misplaced_bounds(&ns(&[
            ("string", Some(0), None),
            ("string[]", None, Some(4)),
            ("boolean", Some(0), Some(1)),
            ("integer", Some(3), Some(2)),
        ]));
        assert_eq!(found.len(), 4, "{found:?}");
        assert!(found.iter().all(|f| f.kind == "misplaced-field-bound"));
        assert!(found[3].message.contains("`f3`"), "{}", found[3].message);
        assert!(found[3].message.contains("above"), "{}", found[3].message);
    }
}
