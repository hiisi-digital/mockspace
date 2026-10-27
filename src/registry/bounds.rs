//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a field declares about the values it may hold, its `min` and `max` and its closed
//! set, refused at the declaration where the schema could not hold it.
//!
//! Each of these would otherwise fail at every row instead, with a validator error naming
//! the row rather than the declaration, or be dropped on the way and read as a constraint
//! nothing holds.

use super::*;

/// A bound declared on a field that is not an `integer`, or a `min` above its `max`.
///
/// Refused rather than ignored. The schema carries a bound only on an integer, so one
/// written on a string or a list would be dropped on the way and the field would read
/// as constrained while nothing held it, which is the failure `unknown_field_types`
/// exists for one step over. A `min` above the `max` admits no value at all.
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
                    "has `min` {lo} above `max` {hi}, and no value satisfies both"
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

/// A member of a field's `values` that no row could hold: one that does not read as an
/// `integer` or a `boolean` on a field of that type, an integer not written plainly, or
/// an integer outside the field's own `min` and `max`.
pub fn unfit_values(namespaces: &[RegistryNamespace]) -> Vec<RegistryFinding> {
    let mut out = Vec::new();
    for ns in namespaces {
        for f in &ns.fields {
            for v in &f.values {
                let problem = match f.r#type.as_str() {
                    "integer" => {
                        match v.parse::<i64>() {
                            Err(_) => Some(format!("`{v}` does not read as an integer")),
                            Ok(n) if n.to_string() != *v => {
                                Some(format!(
                                    "`{v}` is the integer {n} written another way, and a row holds it as `{n}`"
                                ))
                            },
                            Ok(n)
                                if f.min.is_some_and(|lo| n < lo)
                                    || f.max.is_some_and(|hi| n > hi) =>
                            {
                                Some(format!("{n} lies outside the field's own `min` and `max`"))
                            },
                            Ok(_) => None,
                        }
                    },
                    "boolean" if v != "true" && v != "false" => {
                        Some(format!("`{v}` is neither `true` nor `false`"))
                    },
                    _ => None,
                };
                if let Some(problem) = problem {
                    out.push(RegistryFinding {
                        kind:    "unfit-field-value",
                        message: format!(
                            "namespace `{}` lists a value for the `{}` field `{}` that no row can hold: {problem}.",
                            ns.key, f.r#type, f.name
                        ),
                        source:  None,
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(
        i: usize,
        ty: &str,
        values: &[&str],
        min: Option<i64>,
        max: Option<i64>,
    ) -> RegistryField {
        RegistryField {
            name: format!("f{i}"),
            r#type: ty.into(),
            required: false,
            description: None,
            visibility: FieldVisibility::Public,
            values: values.iter().map(|v| (*v).to_string()).collect(),
            min,
            max,
        }
    }

    fn ns(fields: Vec<RegistryField>) -> Vec<RegistryNamespace> {
        vec![RegistryNamespace {
            key: "probe".into(),
            title: None,
            description: None,
            value_field: None,
            render: RenderMode::Page,
            group_by: None,
            fields,
        }]
    }

    fn bounded(fields: &[(&str, Option<i64>, Option<i64>)]) -> Vec<RegistryNamespace> {
        ns(fields
            .iter()
            .enumerate()
            .map(|(i, (ty, min, max))| field(i, ty, &[], *min, *max))
            .collect())
    }

    #[test]
    fn a_bound_on_an_integer_is_taken_whole_or_half() {
        // the control: without it the arms below pass on a check refusing every bound
        let found = misplaced_bounds(&bounded(&[
            ("integer", Some(0), Some(10)),
            ("integer", Some(5), Some(5)),
            ("integer", None, Some(3)),
            ("integer", Some(-2), None),
            ("string", None, None),
        ]));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_bound_on_every_other_type_is_refused_naming_the_type() {
        let others: Vec<&str> = SCALAR_TYPES
            .iter()
            .copied()
            .filter(|t| *t != "integer")
            .chain(["claim", "claim[]"])
            .collect();
        let found = misplaced_bounds(&bounded(
            &others
                .iter()
                .map(|t| (*t, Some(0), None))
                .collect::<Vec<_>>(),
        ));
        assert_eq!(found.len(), others.len(), "{found:?}");
        for (f, ty) in found.iter().zip(&others) {
            assert_eq!(f.kind, "misplaced-field-bound");
            assert!(f.message.contains(&format!("is `{ty}`")), "{}", f.message);
        }
    }

    #[test]
    fn a_min_above_its_max_is_refused() {
        let found = misplaced_bounds(&bounded(&[("integer", Some(3), Some(2))]));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].message.contains("`f0`") && found[0].message.contains("above"),
            "{}",
            found[0].message
        );
    }

    #[test]
    fn a_closed_set_that_reads_as_its_type_and_sits_in_its_range_passes() {
        // the control for the arm below
        let found = unfit_values(&ns(vec![
            field(0, "integer", &["1", "-2", "5"], Some(-2), Some(5)),
            field(1, "boolean", &["true", "false"], None, None),
            field(2, "string", &["anything", "3"], None, None),
            field(3, "string[]", &["true"], None, None),
        ]));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_closed_set_member_no_row_can_hold_is_refused() {
        let found = unfit_values(&ns(vec![
            field(0, "integer", &["one"], None, None),
            field(1, "integer", &["6"], Some(1), Some(5)),
            field(2, "integer", &["0"], Some(1), None),
            field(3, "boolean", &["yes"], None, None),
            field(4, "integer", &["+5"], None, None),
            field(5, "integer", &["05"], None, None),
        ]));
        assert_eq!(found.len(), 6, "{found:?}");
        assert!(found.iter().all(|f| f.kind == "unfit-field-value"));
        for (f, name) in found.iter().zip(["f0", "f1", "f2", "f3", "f4", "f5"]) {
            assert!(f.message.contains(&format!("`{name}`")), "{}", f.message);
        }
    }
}
