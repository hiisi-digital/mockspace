//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The duplicate decision and the symbol lookup over real recorded output of
//! linked libraries of each object format and tool flavour
//! (`disasm/fixtures/`, see its README), so every format's shape is held on
//! every host.
//!
//! Each case is four libraries: `variant_scalar` and `variant_unrolled_x4`
//! linked from one source (identical work under two crate names; the PE pair
//! also at two image bases), `variant_other` computing different work, and
//! `variant_shifted`, the first with one more exported function placed before
//! the rest, so its `bench_entry` is the same code at other addresses, with
//! other branch, call and data targets.

use super::normalize::instruction_text;
use super::pe_exports::export_block;
use super::tool::symbol_block;
use super::*;

struct Library {
    whole:   &'static str,
    /// GNU `objdump -p` of a PE image, where the entry is found by export.
    exports: Option<&'static str>,
}

struct Recorded {
    what:     &'static str,
    flavour:  Flavour,
    /// `bench_entry` as the disassembler labels it in this format.
    entry:    &'static str,
    scalar:   Library,
    unrolled: Library,
    other:    Library,
    shifted:  Library,
}

macro_rules! library {
    ($target:literal, $tool:literal, $crate_name:literal, none) => {
        Library {
            whole:   include_str!(concat!(
                "fixtures/",
                $target,
                "/",
                $tool,
                "-",
                $crate_name,
                ".txt"
            )),
            exports: None,
        }
    };
    ($target:literal, $tool:literal, $crate_name:literal, exports) => {
        Library {
            whole:   include_str!(concat!(
                "fixtures/",
                $target,
                "/",
                $tool,
                "-",
                $crate_name,
                ".txt"
            )),
            exports: Some(include_str!(concat!(
                "fixtures/",
                $target,
                "/",
                $tool,
                "-",
                $crate_name,
                ".exports.txt"
            ))),
        }
    };
}

macro_rules! recorded {
    ($target:literal, $tool:literal, $flavour:expr, $entry:literal, $exports:ident) => {
        Recorded {
            what:     concat!($target, " / ", $tool),
            flavour:  $flavour,
            entry:    $entry,
            scalar:   library!($target, $tool, "variant_scalar", $exports),
            unrolled: library!($target, $tool, "variant_unrolled_x4", $exports),
            other:    library!($target, $tool, "variant_other", $exports),
            shifted:  library!($target, $tool, "variant_shifted", $exports),
        }
    };
}

fn cases() -> [Recorded; 7] {
    [
        recorded!(
            "x86_64-unknown-linux-gnu",
            "llvm",
            Flavour::Llvm,
            "bench_entry",
            none
        ),
        recorded!(
            "x86_64-unknown-linux-gnu",
            "gnu",
            Flavour::Gnu,
            "bench_entry",
            none
        ),
        recorded!(
            "aarch64-unknown-linux-gnu",
            "llvm",
            Flavour::Llvm,
            "bench_entry",
            none
        ),
        recorded!(
            "x86_64-apple-darwin",
            "llvm",
            Flavour::Llvm,
            "_bench_entry",
            none
        ),
        recorded!(
            "aarch64-apple-darwin",
            "llvm",
            Flavour::Llvm,
            "_bench_entry",
            none
        ),
        recorded!(
            "x86_64-pc-windows-msvc",
            "llvm",
            Flavour::Llvm,
            "bench_entry",
            none
        ),
        recorded!(
            "x86_64-pc-windows-msvc",
            "gnu",
            Flavour::Gnu,
            "bench_entry",
            exports
        ),
    ]
}

/// The raw `bench_entry` of a library, as `Disassembler::symbol` cuts it.
fn raw_entry(c: &Recorded, lib: &Library) -> Option<String> {
    match lib.exports {
        Some(p) => export_block(lib.whole, p, c.entry),
        None => symbol_block(lib.whole, c.entry),
    }
}

fn entry(c: &Recorded, lib: &Library) -> Option<String> {
    non_empty(normalize_disasm(&raw_entry(c, lib)?, c.flavour))
}

fn text(c: &Recorded, lib: &Library) -> Option<String> {
    non_empty(normalize_disasm(lib.whole, c.flavour))
}

/// The instruction lines of a dump, addresses included: everything but its
/// header, section titles and labels.
fn instruction_lines(whole: &str) -> Vec<&str> {
    whole
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty()
                && !t.contains("file format")
                && !t.starts_with("Disassembly of section")
                && !t.ends_with(':')
        })
        .collect()
}

#[test]
fn every_recorded_pair_differs_in_its_instruction_lines_before_normalisation() {
    // Without this the tests below could pass on pairs identical in every
    // line the normaliser folds, and prove nothing about it.
    for c in cases() {
        assert_ne!(
            instruction_lines(c.scalar.whole),
            instruction_lines(c.unrolled.whole),
            "{}: the identical-work pair has the same instruction lines",
            c.what
        );
        let (a, b) = (
            raw_entry(&c, &c.scalar).unwrap(),
            raw_entry(&c, &c.shifted).unwrap(),
        );
        let strip = |s: &str| -> Vec<String> {
            s.lines()
                .filter_map(instruction_text)
                .map(str::to_string)
                .collect()
        };
        assert_ne!(
            strip(&a),
            strip(&b),
            "{}: the shifted entry has the same instructions with the address column off",
            c.what
        );
    }
}

#[test]
fn identical_work_in_two_libraries_is_one_duplicate_on_every_format_and_tool() {
    for c in cases() {
        let paths = vec!["scalar".to_string(), "unrolled".to_string(), "other".to_string()];
        let libs = [&c.scalar, &c.unrolled, &c.other];
        let texts: Vec<Option<String>> = libs.iter().map(|l| text(&c, l)).collect();
        let entries: Vec<Option<String>> = libs.iter().map(|l| entry(&c, l)).collect();
        assert!(
            texts.iter().chain(&entries).all(Option::is_some),
            "{}: not read",
            c.what
        );
        let report = build_report(&paths, &entries, &texts);
        assert_eq!(
            report.dupes,
            vec![("scalar".to_string(), "unrolled".to_string())],
            "{}: expected exactly the identical pair\n--- scalar\n{}\n--- unrolled\n{}",
            c.what,
            texts[0].as_deref().unwrap_or(""),
            texts[1].as_deref().unwrap_or("")
        );
    }
}

#[test]
fn the_same_entry_at_other_addresses_normalises_to_the_same_text() {
    for c in cases() {
        let (a, b) = (entry(&c, &c.scalar), entry(&c, &c.shifted));
        assert!(a.is_some(), "{}: no entry", c.what);
        assert_eq!(a, b, "{}: one entry, shifted, compares different", c.what);
        // The library with the extra function is no duplicate of the other.
        assert_ne!(text(&c, &c.scalar), text(&c, &c.shifted), "{}", c.what);
    }
}

#[test]
fn bench_entry_is_found_under_the_formats_own_spelling_and_no_other() {
    for c in cases() {
        let block = raw_entry(&c, &c.scalar).unwrap_or_else(|| panic!("{}: no entry", c.what));
        let wrong = match c.entry.strip_prefix('_') {
            Some(bare) => bare.to_string(),
            None => format!("_{}", c.entry),
        };
        let lib = &c.scalar;
        let found = match lib.exports {
            Some(p) => export_block(lib.whole, p, &wrong),
            None => symbol_block(lib.whole, &wrong),
        };
        assert_eq!(found, None, "{}: {wrong} matched", c.what);
        // The entry's block ends at the entry: `do_work`'s body is not in it.
        let lines = normalize_disasm(&block, c.flavour).lines().count();
        assert!(
            (2 .. 30).contains(&lines),
            "{}: {lines} lines is not one small function:\n{block}",
            c.what
        );
    }
}
