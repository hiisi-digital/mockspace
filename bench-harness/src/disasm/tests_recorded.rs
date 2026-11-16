//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The duplicate decision and the symbol lookup over real recorded output of
//! each object format and tool flavour (`disasm/fixtures/`, see its README),
//! so every platform's shape is held on every host.
//!
//! Each case is three files: `variant_scalar` and `variant_unrolled_x4`
//! compiled from one source (a duplicate pair whose crate names differ in
//! length and whose salted symbol names differ), and `variant_other` from a
//! source computing different work.

use super::tool::symbol_block;
use super::*;

struct Recorded {
    what:     &'static str,
    flavour:  Flavour,
    /// `bench_entry` as this target's object format spells it.
    entry:    &'static str,
    scalar:   &'static str,
    unrolled: &'static str,
    other:    &'static str,
}

macro_rules! recorded {
    ($target:literal, $tool:literal, $flavour:expr, $entry:literal) => {
        Recorded {
            what:     concat!($target, " / ", $tool),
            flavour:  $flavour,
            entry:    $entry,
            scalar:   include_str!(concat!(
                "fixtures/",
                $target,
                "/",
                $tool,
                "-variant_scalar.txt"
            )),
            unrolled: include_str!(concat!(
                "fixtures/",
                $target,
                "/",
                $tool,
                "-variant_unrolled_x4.txt"
            )),
            other:    include_str!(concat!(
                "fixtures/",
                $target,
                "/",
                $tool,
                "-variant_other.txt"
            )),
        }
    };
}

fn cases() -> [Recorded; 7] {
    [
        recorded!(
            "x86_64-unknown-linux-gnu",
            "llvm",
            Flavour::Llvm,
            "bench_entry"
        ),
        recorded!(
            "x86_64-unknown-linux-gnu",
            "gnu",
            Flavour::Gnu,
            "bench_entry"
        ),
        recorded!(
            "aarch64-unknown-linux-gnu",
            "llvm",
            Flavour::Llvm,
            "bench_entry"
        ),
        recorded!("x86_64-apple-darwin", "llvm", Flavour::Llvm, "_bench_entry"),
        recorded!(
            "aarch64-apple-darwin",
            "llvm",
            Flavour::Llvm,
            "_bench_entry"
        ),
        recorded!(
            "x86_64-pc-windows-msvc",
            "llvm",
            Flavour::Llvm,
            "bench_entry"
        ),
        recorded!("x86_64-pc-windows-msvc", "gnu", Flavour::Gnu, "bench_entry"),
    ]
}

#[test]
fn the_recorded_pair_differs_before_normalisation() {
    // Without this the duplicate test below could pass on two identical files
    // and prove nothing about the normaliser.
    for c in cases() {
        assert_ne!(
            c.scalar, c.unrolled,
            "{}: the recorded pair is byte-identical",
            c.what
        );
    }
}

#[test]
fn identical_work_in_two_crates_is_one_duplicate_on_every_format_and_tool() {
    for c in cases() {
        let paths = vec!["scalar".to_string(), "unrolled".to_string(), "other".to_string()];
        let text: Vec<Option<String>> = [c.scalar, c.unrolled, c.other]
            .iter()
            .map(|raw| non_empty(normalize_disasm(raw, c.flavour)))
            .collect();
        let entry: Vec<Option<String>> = [c.scalar, c.unrolled, c.other]
            .iter()
            .map(|raw| symbol_block(raw, c.entry).map(|b| normalize_disasm(&b, c.flavour)))
            .collect();
        assert!(text.iter().all(Option::is_some), "{}: nothing read", c.what);
        let report = build_report(&paths, &entry, &text);
        assert_eq!(
            report.dupes,
            vec![("scalar".to_string(), "unrolled".to_string())],
            "{}: expected exactly the identical pair\n--- scalar\n{}\n--- unrolled\n{}",
            c.what,
            text[0].as_deref().unwrap_or(""),
            text[1].as_deref().unwrap_or("")
        );
        assert!(report.unreadable.is_empty(), "{}", c.what);
    }
}

#[test]
fn bench_entry_is_found_under_the_formats_own_spelling() {
    for c in cases() {
        let block = symbol_block(c.scalar, c.entry)
            .unwrap_or_else(|| panic!("{}: no {} label", c.what, c.entry));
        assert!(block.lines().count() > 2, "{}: {block}", c.what);
        // The wrong spelling for the format finds nothing.
        let wrong = match c.entry.strip_prefix('_') {
            Some(bare) => bare.to_string(),
            None => format!("_{}", c.entry),
        };
        assert_eq!(
            symbol_block(c.scalar, &wrong),
            None,
            "{}: {wrong} matched",
            c.what
        );
        // The entry's block ends at the entry: `do_work`'s body is not in it.
        let normalized = normalize_disasm(&block, c.flavour);
        assert!(
            normalized.lines().count() < normalize_disasm(c.scalar, c.flavour).lines().count(),
            "{}: the block ran past bench_entry",
            c.what
        );
        assert_eq!(
            normalized,
            normalize_disasm(&symbol_block(c.unrolled, c.entry).unwrap(), c.flavour),
            "{}: one source, two crates, two different entries",
            c.what
        );
    }
}
