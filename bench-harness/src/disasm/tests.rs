//! Unit and integration tests for the disassembly duplicate check.
//!
//! Split from `disasm.rs` per the workspace's file-size discipline;
//! this module has private access to everything in `super` exactly as
//! it would inline, via `#[path]`.
//!
//! [`normalize_disasm`] / [`strip_symbol_annotation`] / [`same_work`] /
//! [`duplicate_pairs`] are pure and tested with synthetic strings, no
//! subprocess involved. The remaining tests compile real cdylibs at
//! test time via `rustc` directly (no cargo, no crate deps) and run
//! the actual `objdump`/`otool` extraction against them.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

// ── pure logic: normalize_disasm / strip_symbol_annotation ──

#[test]
fn normalize_disasm_strips_address_and_raw_opcode_columns() {
    let text = "\n\
        libx.dylib:\tfile format mach-o arm64\n\
        \n\
        Disassembly of section __TEXT,__text:\n\
        \n\
        0000000000000338 <_bench_entry>:\n\
        \x20\x20\x20\x20338: b40001c0     \tcbz\tx0, 0x370\n\
        \x20\x20\x20\x2033c: d1000408     \tsub\tx8, x0, #0x1\n";
    let normalized = normalize_disasm(text);
    // The header line survives (it carries no address to strip past its
    // own tab) but its kept half, "file format mach-o arm64", is
    // identical for every dylib of the same architecture, so it never
    // causes two variants to compare unequal.
    assert_eq!(normalized, "file format mach-o arm64\ncbz\tx0, 0x370\nsub\tx8, x0, #0x1");
}

#[test]
fn normalize_disasm_drops_otool_section_header() {
    let text = "libx.dylib:\n(__TEXT,__text) section\n_bench_entry:\n0000000000000338\tret\n";
    let normalized = normalize_disasm(text);
    assert_eq!(normalized, "ret");
}

#[test]
fn normalize_disasm_strips_crate_salted_symbol_annotation() {
    // Two variants computing identical work in separate crates get a
    // different mangled name for the same private helper (the crate's
    // own metadata hash is embedded in it); the raw call target address
    // is unaffected. This is the false-negative defect two's fix
    // corrects: without stripping the annotation, these two lines
    // would never compare equal for any pair of distinct crates.
    let a = "     8c0: 97fffff0     \tbl\t0x880 <__RINvCsfGFIfYg1MGU_10variant_a27do_workKj40_EB2_>\n";
    let b = "     8c0: 97fffff0     \tbl\t0x880 <__RINvCsdNdfRXoBgyl_10variant_b27do_workKj40_EB2_>\n";
    assert_eq!(normalize_disasm(a), normalize_disasm(b));
    assert_eq!(normalize_disasm(a), "bl\t0x880");
}

#[test]
fn normalize_disasm_keeps_a_genuinely_different_instruction_different() {
    let xor_mul = "     8b8: ca0b0108     \teor\tx8, x8, x11\n";
    let add_mul = "     8b8: 8b090108     \tadd\tx8, x8, x9\n";
    assert_ne!(normalize_disasm(xor_mul), normalize_disasm(add_mul));
}

// ── pure logic: same_work / duplicate_pairs ──

fn some(s: &str) -> Option<String> {
    Some(s.to_string())
}

#[test]
fn same_entry_and_same_text_is_a_duplicate() {
    assert!(same_work(&some("a"), &some("a"), &some("x"), &some("x")));
}

#[test]
fn differing_entry_is_never_a_duplicate_even_if_text_matches() {
    // A differing bench_entry proves differing `.text` on its own; this
    // also guards against a hypothetical bug where the two extraction
    // paths disagree despite covering the same bytes.
    assert!(!same_work(&some("a"), &some("b"), &some("x"), &some("x")));
}

#[test]
fn matching_entry_with_differing_text_is_not_a_duplicate() {
    // This is defect two itself: an identical dispatcher shell around
    // per-size functions that were not inlined must not be reported as
    // a duplicate when the functions actually differ.
    assert!(!same_work(&some("same"), &some("same"), &some("alpha"), &some("beta")));
}

#[test]
fn missing_text_is_never_a_duplicate() {
    assert!(!same_work(&some("same"), &some("same"), &some("alpha"), &None));
    assert!(!same_work(&some("same"), &some("same"), &None, &None));
}

#[test]
fn duplicate_pairs_reports_every_matching_pair_once() {
    let paths = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let entry = vec![some("x"), some("x"), some("y")];
    let text = vec![some("t"), some("t"), some("t")];
    let dupes = duplicate_pairs(&paths, &entry, &text);
    assert_eq!(dupes, vec![("a".to_string(), "b".to_string())]);
}

// ── CheckReport: "could not answer" must be distinguishable from
// "answered, no duplicates". Both currently produce an empty `dupes`;
// the whole point of `CheckReport` is that `unreadable` is what tells
// the two apart, so these assert on it directly rather than assuming
// an empty duplicate list means a clean pass.

#[test]
fn a_genuine_clean_pass_has_no_unreadable_variants() {
    let paths = vec!["a".to_string(), "b".to_string()];
    let entry = vec![some("x"), some("y")];
    let text = vec![some("alpha"), some("beta")];
    let report = build_report(&paths, &entry, &text);
    assert!(report.dupes.is_empty());
    assert!(
        report.unreadable.is_empty(),
        "a run where every variant was disassembled and none matched must not carry any \
         unreadable paths"
    );
}

#[test]
fn total_extraction_failure_is_not_a_clean_pass() {
    // This is defect one's failure shape one level up: if `.text`
    // extraction fails for every variant, `same_work` is false for
    // every pair by construction (see `missing_text_is_never_a_duplicate`
    // above), so `dupes` alone is indistinguishable from a genuine
    // no-duplicates run. `unreadable` is what a caller must check.
    let paths = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let entry = vec![None, None, None];
    let text = vec![None, None, None];
    let report = build_report(&paths, &entry, &text);
    assert!(report.dupes.is_empty());
    assert_eq!(
        report.unreadable, paths,
        "every variant's `.text` section failed to extract; all of them should be reported \
         as unreadable, not silently dropped"
    );
}

#[test]
fn partial_extraction_failure_still_reports_dupes_found_among_the_rest() {
    let paths = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let entry = vec![some("x"), some("x"), None];
    let text = vec![some("t"), some("t"), None];
    let report = build_report(&paths, &entry, &text);
    assert_eq!(report.dupes, vec![("a".to_string(), "b".to_string())]);
    assert_eq!(report.unreadable, vec!["c".to_string()]);
}

// ── real dylibs, compiled at test time via rustc directly (no cargo,
// no crate deps), so these run without a published mockspace-bench-core.
// Skips (rather than fails) when rustc or objdump/otool are missing,
// so a stripped-down environment degrades instead of flaking. Both
// tools are present on every machine that can build this crate at all
// (rustc always; objdump ships with the platform's C toolchain), so
// the skip path is not expected to trigger in normal use.

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("mockspace_bench_disasm_test_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[cfg(target_os = "macos")]
const DYLIB_EXT: &str = "dylib";
#[cfg(not(target_os = "macos"))]
const DYLIB_EXT: &str = "so";

/// Compile `source` as a cdylib named `crate_name` via `rustc`
/// directly. Returns `None` (the caller should skip, not fail) if
/// `rustc` cannot be run.
fn build_cdylib(dir: &Path, crate_name: &str, source: &str) -> Option<PathBuf> {
    let src_path = dir.join(format!("{crate_name}.rs"));
    std::fs::write(&src_path, source).expect("write probe source");
    let out_path = dir.join(format!("lib{crate_name}.{DYLIB_EXT}"));
    let status = Command::new("rustc")
        .args(["--crate-type", "cdylib", "--crate-name", crate_name, "-O", "--edition", "2021"])
        .arg("-o")
        .arg(&out_path)
        .arg(&src_path)
        .status()
        .ok()?;
    if status.success() { Some(out_path) } else { None }
}

/// A `#[inline(never)]` per-size function so the compiler cannot
/// fold it into `bench_entry`, matching the real macro's shape when
/// the optimizer declines to inline (large function, multiple
/// codegen units, no LTO): `bench_entry` stays a thin dispatcher
/// whose own disassembly is the same shell regardless of what the
/// per-size function computes.
fn dispatcher_source(work_body: &str) -> String {
    format!(
        "#[inline(never)]\n\
         fn do_work(input: &[u8; 64], output: &mut u64) {{\n\
         {work_body}\n\
         }}\n\
         \n\
         #[unsafe(no_mangle)]\n\
         pub unsafe extern \"C\" fn bench_entry(\n\
         \x20\x20\x20\x20input_ptr: *const u8,\n\
         \x20\x20\x20\x20output_ptr: *mut u8,\n\
         \x20\x20\x20\x20n: usize,\n\
         ) -> u64 {{\n\
         \x20\x20\x20\x20match n {{\n\
         \x20\x20\x20\x2064 => {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20let input = unsafe {{ &*(input_ptr as *const [u8; 64]) }};\n\
         \x20\x20\x20\x20\x20\x20\x20\x20let output = unsafe {{ &mut *(output_ptr as *mut u64) }};\n\
         \x20\x20\x20\x20\x20\x20\x20\x20do_work(input, output);\n\
         \x20\x20\x20\x20\x20\x20\x20\x200\n\
         \x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20other => panic!(\"bad n={{other}}\"),\n\
         \x20\x20\x20\x20}}\n\
         }}\n"
    )
}

const ADD_MUL_WORK: &str = "\x20\x20\x20\x20let mut acc: u64 = 0;\n\
    \x20\x20\x20\x20for &b in input.iter() {\n\
    \x20\x20\x20\x20\x20\x20\x20\x20acc = acc.wrapping_add(b as u64).wrapping_mul(31);\n\
    \x20\x20\x20\x20}\n\
    \x20\x20\x20\x20*output = acc;";

const XOR_MUL_WORK: &str = "\x20\x20\x20\x20let mut acc: u64 = 0xcbf29ce484222325;\n\
    \x20\x20\x20\x20for &b in input.iter() {\n\
    \x20\x20\x20\x20\x20\x20\x20\x20acc ^= b as u64;\n\
    \x20\x20\x20\x20\x20\x20\x20\x20acc = acc.wrapping_mul(0x100000001b3);\n\
    \x20\x20\x20\x20}\n\
    \x20\x20\x20\x20*output = acc;";

#[test]
fn objdump_finds_bench_entry_under_this_platforms_symbol_prefix() {
    // Regression test for defect one: on Mach-O the exported symbol is
    // `_bench_entry`, and a bare `--disassemble-symbols=bench_entry`
    // used to find nothing (objdump exits 0 with only a stderr
    // warning). Calls `objdump_symbol` directly rather than through
    // `extract_bench_entry`, so a silent fall-through to the otool
    // fallback cannot mask a regression here.
    let dir = scratch_dir("defect_one");
    let Some(dylib) = build_cdylib(&dir, "probe_defect_one", &dispatcher_source(ADD_MUL_WORK))
    else {
        eprintln!("skipping: rustc unavailable");
        return;
    };
    let Some(asm) = objdump_symbol(dylib.to_str().unwrap(), "bench_entry") else {
        panic!("objdump_symbol found nothing for a freshly built dylib's bench_entry");
    };
    assert!(
        !asm.trim().is_empty(),
        "bench_entry disassembly should not be empty"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn identical_work_in_separate_crates_is_flagged_duplicate() {
    let dir = scratch_dir("identical");
    let source = dispatcher_source(ADD_MUL_WORK);
    let (Some(a), Some(b)) = (
        build_cdylib(&dir, "probe_identical_a", &source),
        build_cdylib(&dir, "probe_identical_b", &source),
    ) else {
        eprintln!("skipping: rustc unavailable");
        return;
    };
    let paths = vec![a.to_str().unwrap().to_string(), b.to_str().unwrap().to_string()];
    let entry_asm: Vec<Option<String>> =
        paths.iter().map(|p| extract_bench_entry(p)).collect();
    let text_section: Vec<Option<String>> =
        paths.iter().map(|p| extract_text_section(p)).collect();
    assert!(
        text_section.iter().all(Option::is_some),
        "expected extract_text_section to succeed on both freshly built dylibs"
    );
    let dupes = duplicate_pairs(&paths, &entry_asm, &text_section);
    assert_eq!(
        dupes.len(),
        1,
        "identical work in two crates should be flagged as one duplicate pair"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn different_work_is_not_flagged_duplicate() {
    // `do_work` is `#[inline(never)]` in both variants, matching the
    // bug defect two names: the real work sits in a symbol
    // `bench_entry` merely calls, not in `bench_entry` itself. The
    // variants must not be reported as duplicates, regardless of
    // whether `bench_entry`'s own shell happens to land at the same
    // address in both (unrelated compiler layout decisions can move
    // it either way); what must hold is that `do_work` computing
    // different things is never lost.
    let dir = scratch_dir("different");
    let (Some(a), Some(b)) = (
        build_cdylib(&dir, "probe_different_a", &dispatcher_source(ADD_MUL_WORK)),
        build_cdylib(&dir, "probe_different_b", &dispatcher_source(XOR_MUL_WORK)),
    ) else {
        eprintln!("skipping: rustc unavailable");
        return;
    };
    let paths = vec![a.to_str().unwrap().to_string(), b.to_str().unwrap().to_string()];
    let entry_asm: Vec<Option<String>> =
        paths.iter().map(|p| extract_bench_entry(p)).collect();
    let text_section: Vec<Option<String>> =
        paths.iter().map(|p| extract_text_section(p)).collect();
    assert!(
        text_section.iter().all(Option::is_some),
        "expected extract_text_section to succeed on both freshly built dylibs"
    );
    let dupes = duplicate_pairs(&paths, &entry_asm, &text_section);
    assert!(
        dupes.is_empty(),
        "different `do_work` bodies must not be flagged as duplicates"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
