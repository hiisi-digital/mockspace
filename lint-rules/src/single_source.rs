//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Cross-crate lint: one definition per concept.
//!
//! Two findings, both about one concept defined in more than one crate:
//!
//! - the same type name (struct, enum, trait, union) defined in two crates;
//! - the same constant or static, same name, same type and same value, defined
//!   in two crates. A different value under one name is two constants, and the
//!   same value under two names is a coincidence; neither is this finding.
//!
//! It reads every source file of every crate (`LintContext::all_sources`),
//! following `mod` declarations from the crate roots, so a definition in a module
//! file, a nested `mod`, a `#[path]` module or a `cfg`-gated module is seen. A
//! finding in a file other than the crate root names the file, so a
//! `lint:allow(single-source)` marker is read from that file. Config can tell the
//! two findings apart: `type` and `constant` are the finding kinds.
//!
//! Re-exports (`pub use`) are excluded: only actual definitions count.
//!
//! Exempt: test code (a `#[cfg(test)]` item or module, a file declared only
//! through one, a file that opens with `#![cfg(test)]`), macro-generated items
//! inside `macro_rules!`, and type aliases (`type Foo = ...`), which are
//! intentional re-definitions.
//!
//! # Only fully `pub` items are compared
//!
//! A crate's non-`pub` items (private, `pub(crate)`, `pub(super)`, `pub(in ..)`,
//! `pub(self)`) are that crate's own and are not compared across crates; only an
//! item that is plain `pub` is. This is the coordinator's call of 22 October,
//! made pending op's ruling, since the canon is silent on whether a private
//! type in one crate and a public one of the same name in another are one
//! concept. It is the single switch [`COMPARE_ONLY_PUB`]; flipping it compares
//! every item regardless of visibility and prints the qualifier in the message.
//! Only the item's own qualifier is read: a plain `pub` item inside a private
//! module is compared as `pub`.
//!
//! # How values are compared
//!
//! A value is compared as its canonical token form. An integer is its decimal
//! value without underscores or radix, so `1_000u32`, `0x3E8` and `1000` agree; a
//! float is its parsed value; every other literal is compared as written; an
//! operator expression, array, tuple, reference or cast is compared element by
//! element. A literal that is the whole value drops its type suffix, because the
//! constant's type already says it; a literal inside an expression keeps it, since
//! there it decides the result (`!0u8 as u32` is 255, `!0u16 as u32` is 65535).
//! Only a *closed* value is compared, one built from literals and operators alone.
//! A value that names anything (a path, a call, a struct expression, a macro) is
//! open and is never reported, because the same tokens in two crates can mean two
//! values. A type is compared as its tokens.
//!
//! # What it cannot see
//!
//! Each of these is pinned by a test, so a change to any of them is a decision:
//!
//! - Items a macro generates, and the body of `macro_rules!`. Nothing is expanded.
//! - A module whose `#[path]` leaves the crate's `src/` directory, and anything
//!   `include!`d. Only the files in `all_sources` exist to the lint.
//! - Files outside `src/`: `tests/`, `benches/`, `examples/`, `build.rs`.
//! - Items inside a function body, and associated constants in an `impl` or a
//!   `trait`, which belong to their type rather than to the crate.
//! - A `static` in an `extern` block, which has no value of its own.
//! - Names are compared as written. A type re-exported under another name, or two
//!   unrelated types that share a name by accident, are told apart by nobody.
//! - Values are not evaluated: `16` and `1 << 4` are different values here. Two
//!   spellings of one string (`"\u{41}"` and `"A"`, or a raw string) differ too,
//!   and so do `1000` and `1000u32` inside an expression.
//! - An open value (see above), and a type spelled through a different path
//!   (`u32` against `core::primitive::u32`), never match.
//! - A `cfg` other than a test-only one does not exempt anything: the same
//!   constant under `cfg(unix)` in one crate and `cfg(windows)` in another is two
//!   definitions. `cfg_attr` is not read, so a `#[cfg_attr(test, path = "..")]`
//!   is not followed and its module resolves as if it had no `path`.
//! - The crate roots are `src/lib.rs`, `src/main.rs`, `src/bin/<name>.rs` and
//!   `src/bin/<name>/main.rs`. A custom `[lib] path` or `[[bin]] path` is not
//!   read, so the files under it are counted as ordinary source even where a
//!   `cfg(test)` declaration would have exempted them.
//! - A file no `mod` declaration reaches is counted as ordinary source.

mod defs;
mod value;

use std::collections::BTreeMap;

use self::defs::{Def, Item, collect_crate};
use crate::{Lint, LintContext, LintError, Severity, WorkspaceLint};

/// Whether only fully `pub` items are compared across crates. See the module doc.
const COMPARE_ONLY_PUB: bool = true;

/// Pairs of crate suffixes where duplicate type names are expected.
/// Macro-util crates and their parent crates often define matching types
/// at different abstraction levels (parsing vs. runtime).
/// Full crate names are built as `<prefix>-<suffix>` at check time.
const EXEMPT_PAIRS: &[(&str, &str)] =
    &[("macro-util", "behavior"), ("macro-util", "resource"), ("macro-util", "signal")];

/// How long a value is printed in a message before it is cut.
const VALUE_WIDTH: usize = 48;

pub struct SingleSource;

impl Lint for SingleSource {
    fn name(&self) -> &'static str {
        "single-source"
    }

    /// One concept defined twice under two crates is a mistake in
    /// any project.
    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }
}

impl WorkspaceLint for SingleSource {
    fn check_all(&self, crates: &[(&str, &LintContext)]) -> Vec<LintError> {
        check(crates, COMPARE_ONLY_PUB)
    }
}

/// The lint with the visibility switch passed in, so a test can hold both
/// positions of it.
fn check(crates: &[(&str, &LintContext)], only_pub: bool) -> Vec<LintError> {
    // Get crate prefix from the first context (all share the same prefix).
    let prefix = crates
        .first()
        .map(|(_, ctx)| ctx.crate_prefix)
        .unwrap_or("");

    let all_defs: Vec<Def> = crates
        .iter()
        .flat_map(|&(crate_name, ctx)| collect_crate(crate_name, ctx))
        .filter(|def| !only_pub || def.fully_pub())
        .collect();

    // Build full exempt pairs from prefix + suffixes
    let exempt_full: Vec<(String, String)> = EXEMPT_PAIRS
        .iter()
        .map(|(a, b)| (format!("{prefix}-{a}"), format!("{prefix}-{b}")))
        .collect();

    // Grouped in a sorted map so the findings come out in one order.
    let mut types: BTreeMap<&str, Vec<&Def>> = BTreeMap::new();
    let mut values: BTreeMap<(&str, &str, &str, &str), Vec<&Def>> = BTreeMap::new();
    for def in &all_defs {
        match &def.item {
            Item::Type(_) => types.entry(def.name.as_str()).or_default().push(def),
            Item::Value(v) => {
                values
                    .entry((v.kind, def.name.as_str(), v.ty.as_str(), v.value.as_str()))
                    .or_default()
                    .push(def)
            },
        }
    }

    let mut errors = Vec::new();
    for (name, defs) in &types {
        report(defs, &exempt_full, "type", &mut errors, |dup, first| {
            let Item::Type(kind) = &dup.item else { unreachable!("grouped as a type") };
            format!(
                "{}{kind} `{name}` also defined in {}: one definition per concept",
                qualifier(dup),
                first.crate_name
            )
        });
    }
    for ((kind, name, ty, value), defs) in &values {
        report(defs, &exempt_full, "constant", &mut errors, |dup, first| {
            format!(
                "{}{kind} `{name}: {ty} = {}` also defined with the same type and value in {}: one \
                 definition per concept",
                qualifier(dup),
                shorten(value),
                first.crate_name,
            )
        });
    }
    errors
}

/// The visibility as written, with the space that follows it.
fn qualifier(def: &Def) -> String {
    if def.vis.is_empty() { String::new() } else { format!("{} ", def.vis) }
}

/// One finding per definition outside the crate of the group's first one.
fn report(
    defs: &[&Def],
    exempt: &[(String, String)],
    finding_kind: &'static str,
    errors: &mut Vec<LintError>,
    message: impl Fn(&Def, &Def) -> String,
) {
    let first = defs[0];
    for dup in &defs[1 ..] {
        if dup.crate_name == first.crate_name {
            continue;
        }
        // Skip exempt pairs (e.g., macro-util mirrors parent crate types)
        let is_exempt = exempt.iter().any(|(a, b)| {
            (dup.crate_name == *a && first.crate_name == *b)
                || (dup.crate_name == *b && first.crate_name == *a)
        });
        if is_exempt {
            continue;
        }
        errors.push(LintError {
            path:         dup.path.clone(),
            crate_name:   dup.crate_name.clone(),
            line:         dup.line,
            lint_name:    "single-source",
            severity:     Severity::HARD_ERROR,
            message:      message(dup, first),
            finding_kind: Some(finding_kind),
        });
    }
}

fn shorten(value: &str) -> String {
    if value.chars().count() <= VALUE_WIDTH {
        return value.to_string();
    }
    let cut: String = value.chars().take(VALUE_WIDTH).collect();
    format!("{cut}...")
}

#[cfg(test)]
mod tests;
