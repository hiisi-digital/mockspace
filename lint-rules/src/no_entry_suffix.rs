//! Lint: descriptor types used in `define_registry!` must end in `Descriptor`.
//!
//! The naming convention for all registry descriptor types is `*Descriptor`.
//! Using `*Entry` or other suffixes is inconsistent. This lint scans for
//! `define_registry!` invocations and checks the entry type name.

use crate::{Lint, LintContext, LintError};

pub struct NoEntrySuffix;

impl Lint for NoEntrySuffix {
    fn name(&self) -> &'static str {
        "no-entry-suffix"
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        let mut errors = Vec::new();

        // Scan for define_registry! invocations (Form 1 and Form 2)
        // Form 1: define_registry!(Reg for Trait with DescType);
        // Form 2: define_registry!(Reg for EntryType { ... } with { ... });
        let registry_prefix = format!("{}_registry::define_registry!(", ctx.crate_prefix.replace('-', "_"));
        let dollar_crate_registry_prefix = format!("$crate::{}::define_registry!(", format!("{}_registry", ctx.crate_prefix.replace('-', "_")));

        for (line_idx, line) in ctx.source.lines().enumerate() {
            let trimmed = line.trim();

            // Form 1: "define_registry!(Name for Trait with EntryType)"
            if let Some(rest) = strip_registry_call(trimmed, &registry_prefix, &dollar_crate_registry_prefix) {
                if let Some(entry_name) = extract_form1_entry(rest) {
                    if !entry_name.ends_with("Descriptor") {
                        errors.push(LintError {
                            crate_name: ctx.crate_name.to_string(),
                            line: line_idx + 1,
                            lint_name: "no-entry-suffix",
                        severity: crate::Severity::HARD_ERROR,
                            message: format!(
                                "registry entry type `{entry_name}` should end in `Descriptor` (e.g. `{}Descriptor`)",
                                entry_name
                                    .strip_suffix("Entry")
                                    .unwrap_or(entry_name),
                            ),
                        });
                    }
                }
            }
        }

        errors
    }
}

/// Strip `define_registry!(` or path-qualified forms, return the rest.
fn strip_registry_call<'a>(line: &'a str, registry_prefix: &str, dollar_crate_registry_prefix: &str) -> Option<&'a str> {
    if let Some(rest) = line.strip_prefix("define_registry!(") {
        return Some(rest);
    }
    if let Some(rest) = line.strip_prefix(registry_prefix) {
        return Some(rest);
    }
    if let Some(rest) = line.strip_prefix("$crate::define_registry!(") {
        return Some(rest);
    }
    if let Some(rest) = line.strip_prefix(dollar_crate_registry_prefix) {
        return Some(rest);
    }
    None
}

/// Extract entry type from Form 1: `RegName for TraitName with EntryType);`
fn extract_form1_entry(rest: &str) -> Option<&str> {
    let with_idx = rest.find(" with ")?;
    let after_with = &rest[with_idx + 6..];
    // Entry type ends at `)` or `;` or whitespace
    let end = after_with
        .find(|c: char| c == ')' || c == ';' || c.is_whitespace())
        .unwrap_or(after_with.len());
    let name = &after_with[..end];
    if name.is_empty() || !name.chars().next()?.is_uppercase() {
        return None;
    }
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catches_entry_suffix() {
        assert_eq!(
            extract_form1_entry("ActionRegistry for Action with ActionEntry);"),
            Some("ActionEntry"),
        );
    }

    #[test]
    fn accepts_descriptor_suffix() {
        assert_eq!(
            extract_form1_entry("ActionRegistry for Action with ActionDescriptor);"),
            Some("ActionDescriptor"),
        );
    }

    #[test]
    fn handles_crate_prefixed_call() {
        let prefix = "loimu_registry::define_registry!(";
        let dollar_prefix = "$crate::loimu_registry::define_registry!(";
        assert!(strip_registry_call("loimu_registry::define_registry!(Foo for Bar with Baz);", prefix, dollar_prefix).is_some());
        assert!(strip_registry_call("$crate::define_registry!(Foo for Bar with Baz);", prefix, dollar_prefix).is_some());
    }
}
