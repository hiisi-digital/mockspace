//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Unit and integration tests for the disassembly duplicate check.
//!
//! Split from `disasm.rs` per the workspace's file-size discipline;
//! this module has private access to everything in `super` exactly as
//! it would inline, via `#[path]`.
//!
//! [`symbol_candidates`] / [`normalize_disasm`] / [`normalize_addresses`] /
//! [`same_work`] / [`duplicate_pairs`] / [`build_report`] are pure and
//! tested with synthetic strings and precomputed `Option<String>`
//! values, no subprocess involved (`strip_symbol_annotation` /
//! `adrp_destination` / `fold_adrp_companion_immediate` are exercised
//! only indirectly, through `normalize_disasm`). The remaining tests
//! compile real cdylibs at test time via `rustc` directly (no cargo,
//! no crate deps) and run the actual `objdump`/`otool` extraction
//! against them. [`build_cdylib`] skips (rather than fails) only when
//! `rustc` cannot be spawned at all; a `rustc` that runs and fails to
//! compile the probe source panics with its stderr, so a broken
//! fixture cannot read as a green skip.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

// ── pure logic: symbol_candidates ──

#[test]
fn symbol_candidates_tries_bare_and_underscore_prefixed() {
    // The underscore-prefix guard shipped with no direct test of its
    // own; this pins it on every platform, independent of whether the
    // local `objdump` accepts
    // `--disassemble-symbols=` at all (see the real-dylib test below,
    // which is not universally portable across objdump flavours).
    assert_eq!(symbol_candidates("bench_entry"), [
        "bench_entry".to_string(),
        "_bench_entry".to_string()
    ]);
}

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
    // causes two variants to compare unequal. `0x370` is a branch
    // target, folded to `ADDR`; `#0x1` is a `#`-prefixed immediate and
    // survives untouched.
    assert_eq!(
        normalized,
        "file format mach-o arm64\ncbz\tx0, ADDR\nsub\tx8, x0, #0x1"
    );
}

#[test]
fn normalize_disasm_drops_otool_section_header() {
    let text = "libx.dylib:\n(__TEXT,__text) section\n_bench_entry:\n0000000000000338\tret\n";
    let normalized = normalize_disasm(text);
    assert_eq!(normalized, "ret");
}

#[test]
fn normalize_disasm_folds_mangled_name_and_shifted_target_together() {
    // Two variants computing identical work in separate crates get a
    // different mangled name for the same private helper (the crate's
    // own metadata hash is embedded in it), AND a shifted call-target
    // address (crate names of different lengths, or any other unrelated
    // difference, move where `do_work` lands in `.text`). Real variant
    // names are not conveniently the same length (`variant_scalar`
    // against `variant_unrolled_x4`), so the target addresses below are
    // deliberately different, not just the mangled names: without
    // folding both, these two lines would never compare equal for any
    // real pair of distinct crates.
    let a =
        "     8c0: 97fffff0     \tbl\t0x880 <__RINvCsfGFIfYg1MGU_10variant_a27do_workKj40_EB2_>\n";
    let b = "     8c0: 97fffff0     \tbl\t0x890 <__RINvCsdNdfRXoBgyl_10variant_unrolled_x427do_workKj40_EB2_>\n";
    assert_eq!(normalize_disasm(a), normalize_disasm(b));
    assert_eq!(normalize_disasm(a), "bl\tADDR");
}

#[test]
fn normalize_addresses_folds_a_branch_target_that_shifted_by_an_unrelated_layout_change() {
    // Reproduces the reviewer's finding directly: two builds differing
    // only in something unrelated to the branch itself (here, a longer
    // crate name pushing everything after it forward) shift every
    // subsequent `.text` address by a constant. `b.ne 0xb68` against
    // `b.ne 0xb78` is exactly that shape.
    assert_eq!(
        normalize_addresses("b.ne\t0xb68"),
        normalize_addresses("b.ne\t0xb78")
    );
    assert_eq!(normalize_addresses("b.ne\t0xb68"), "b.ne\tADDR");
}

#[test]
fn normalize_addresses_leaves_hash_prefixed_immediates_alone() {
    // `#0x1f` (31) and `#0x25` (37) are semantic: they are the actual
    // multiplier constants two different algorithms chose, not layout.
    // Folding them would erase exactly the distinction the "different
    // work is not a duplicate" guarantee depends on.
    assert_eq!(normalize_addresses("mov\tx9, #0x1f"), "mov\tx9, #0x1f");
    assert_ne!(
        normalize_addresses("mov\tx9, #0x1f"),
        normalize_addresses("mov\tx9, #0x25")
    );
}

#[test]
fn normalize_addresses_leaves_dollar_prefixed_immediates_alone() {
    // x86 AT&T syntax uses `$` rather than `#` for an immediate.
    assert_eq!(normalize_addresses("mov\t$0x40,%eax"), "mov\t$0x40,%eax");
}

#[test]
fn normalize_addresses_leaves_bracketed_memory_operands_alone() {
    // A memory-operand offset is semantic (which field of the struct is
    // being read), not a branch target, whether or not it happens to be
    // `#`-prefixed; the bracket-depth guard protects it either way.
    assert_eq!(
        normalize_addresses("str\tx2, [sp, #0x8]"),
        "str\tx2, [sp, #0x8]"
    );
    assert_eq!(
        normalize_addresses("ldr\tx0, [x1, 0x10]"),
        "ldr\tx0, [x1, 0x10]"
    );
}

#[test]
fn normalize_addresses_leaves_x86_att_displacement_alone() {
    // x86 AT&T memory operands are not bracketed (`0x10(%rax)`, not
    // `[rax+0x10]`), so the bracket-depth guard above cannot see this
    // one; without its own rule, `0x10` folds to `ADDR(%rax)` and two
    // variants reading different struct fields, array elements, or
    // stack slots compare equal, a false duplicate rather than a missed
    // one. This machine only runs ARM64 objdump/otool, so this is a
    // synthetic-input test of reasoned-correct logic, not a
    // real-dylib-verified one; see the doc comment on
    // `normalize_addresses`.
    assert_eq!(
        normalize_addresses("movq\t0x10(%rax),%rbx"),
        "movq\t0x10(%rax),%rbx"
    );
    // Different displacements must still compare different: this is a
    // struct-field or array-index selection, not layout noise.
    assert_ne!(
        normalize_addresses("movq\t0x10(%rax),%rbx"),
        normalize_addresses("movq\t0x18(%rax),%rbx")
    );
    // A genuine call/branch target elsewhere on the same line still
    // folds; only the parenthesised displacement is protected.
    assert_eq!(
        normalize_addresses("callq\t0x4010a0 <do_work>"),
        "callq\tADDR <do_work>"
    );
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
    // An identical dispatcher shell around per-size functions that were
    // not inlined (so `bench_entry` itself carries no trace of what
    // they compute) must not be reported as a duplicate when the
    // functions actually differ.
    assert!(!same_work(
        &some("same"),
        &some("same"),
        &some("alpha"),
        &some("beta")
    ));
}

#[test]
fn missing_text_is_never_a_duplicate() {
    assert!(!same_work(
        &some("same"),
        &some("same"),
        &some("alpha"),
        &None
    ));
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
    // An extraction that silently finds nothing must not look like an
    // extraction that ran and found nothing wrong: if `.text` extraction
    // fails for every variant, `same_work` is false for every pair by
    // construction (see `missing_text_is_never_a_duplicate` above), so
    // `dupes` alone is indistinguishable from a genuine no-duplicates
    // run. `unreadable` is what a caller must check.
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
    let dir = std::env::temp_dir().join(format!(
        "mockspace_bench_disasm_test_{}_{}",
        std::process::id(),
        name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[cfg(target_os = "macos")]
const DYLIB_EXT: &str = "dylib";
#[cfg(not(target_os = "macos"))]
const DYLIB_EXT: &str = "so";

/// Compile `source` as a cdylib named `crate_name` via `rustc`
/// directly.
///
/// Returns `None` (the caller should skip, not fail) only when `rustc`
/// itself cannot be spawned at all. A `rustc` that runs and fails to
/// compile the probe source is a broken fixture, not an unavailable
/// tool, and panics with the compiler's stderr rather than returning
/// `None`: collapsing the two into one `Option` would let a broken
/// test fixture masquerade as an environment-availability skip, and
/// every test built on this helper would silently pass having tested
/// nothing.
///
/// The source file is always written to the fixed name `probe.rs`
/// inside `dir`, never `{crate_name}.rs`. `panic!()` embeds `file!()`,
/// the literal path passed to rustc, in its `Location` data; a source
/// path whose length tracks `crate_name`'s length would make the
/// panic branch's compiled size depend on the crate name too, which
/// is exactly the kind of test-harness-introduced noise this module
/// exists to be insensitive to in the dylibs under test, not to add on
/// top in the fixture that builds them. `--crate-name` still varies
/// per call and is what the mangled symbols and metadata hash key off.
fn build_cdylib(dir: &Path, crate_name: &str, source: &str) -> Option<PathBuf> {
    let src_path = dir.join("probe.rs");
    std::fs::write(&src_path, source).expect("write probe source");
    let out_path = dir.join(format!("lib{crate_name}.{DYLIB_EXT}"));
    let output = match Command::new("rustc")
        .args(["--crate-type", "cdylib", "--crate-name", crate_name, "-O", "--edition", "2021"])
        .arg("-o")
        .arg(&out_path)
        .arg(&src_path)
        .output()
    {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => panic!("failed to spawn rustc to build probe crate `{crate_name}`: {e}"),
    };
    if !output.status.success() {
        panic!(
            "rustc failed to compile probe crate `{crate_name}`:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Some(out_path)
}

/// A `#[inline(never)]` per-size function so the compiler cannot fold
/// it into `bench_entry`, matching the real macro's shape when the
/// optimizer declines to inline (large function, multiple codegen
/// units, no LTO): `bench_entry` stays a thin dispatcher whose own
/// disassembly is the same shell regardless of what the per-size
/// function computes.
///
/// The unmatched-`n` arm returns `0` rather than the real macro's
/// `panic!(...)`. `n` is always `64` in every test built on this
/// fixture, so the arm never runs; a `panic!` with format
/// interpolation pulls in `core`'s Arguments-building and formatting
/// machinery regardless of whether it runs, which (observed directly)
/// adds roughly 50,000 lines of unrelated disassembly to every dylib
/// and, deep inside it, at least one more instance of the
/// `adrp`+`add`-into-a-different-register address idiom that
/// `fold_adrp_companion_immediate` does not fold (see its doc comment).
/// None of that machinery is what these tests exist to exercise.
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
         \x20\x20\x20\x20_ => 0,\n\
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

/// Whether the local `objdump` looks LLVM-flavored (Apple's toolchain,
/// or `llvm-objdump`), which is what accepts `--disassemble-symbols=`.
/// GNU binutils spells the same option `--disassemble=`, so
/// `objdump_symbol` legitimately returns `None` there: the whole-`.text`
/// path (`objdump_all`, no symbol filter at all) is unaffected and
/// still the authoritative comparison, so this is not a correctness
/// gap in `check_duplicates`, only a reason the entry-only fast path
/// never fires on such a system. Not verified against a real GNU
/// binutils install from this environment; this guard exists so that
/// gap degrades this test into a skip instead of a false failure,
/// rather than silently assuming every `objdump` on every platform
/// accepts the same flag spelling.
fn objdump_is_llvm_flavored() -> bool {
    let Ok(output) = Command::new("objdump").arg("--version").output() else {
        return false;
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    text.contains("LLVM")
}

#[test]
fn objdump_finds_bench_entry_under_this_platforms_symbol_prefix() {
    // On Mach-O the exported symbol is `_bench_entry`, and a bare
    // `--disassemble-symbols=bench_entry` used to find nothing (objdump
    // exits 0 with only a stderr warning). Calls `objdump_symbol`
    // directly rather than through `extract_bench_entry`, so a silent
    // fall-through to the otool fallback cannot mask a regression here.
    let dir = scratch_dir("underscore_prefix");
    let Some(dylib) = build_cdylib(
        &dir,
        "probe_underscore_prefix",
        &dispatcher_source(ADD_MUL_WORK),
    ) else {
        eprintln!("skipping: rustc unavailable");
        return;
    };
    let Some(asm) = objdump_symbol(dylib.to_str().unwrap(), "bench_entry") else {
        if !objdump_is_llvm_flavored() {
            eprintln!(
                "skipping: this objdump does not identify as LLVM-flavored, so \
                 --disassemble-symbols= may not be the flag it accepts (GNU binutils \
                 spells it --disassemble=); see objdump_is_llvm_flavored"
            );
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
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
    // The two crate names are deliberately different lengths, the
    // shape a real pair of variant names actually takes ("same length"
    // would be the case that happens to work even without folding the
    // addresses a longer name shifts everything after it by).
    let dir = scratch_dir("identical");
    let source = dispatcher_source(ADD_MUL_WORK);
    let (Some(a), Some(b)) = (
        build_cdylib(&dir, "variant_scalar", &source),
        build_cdylib(&dir, "variant_unrolled_x4", &source),
    ) else {
        eprintln!("skipping: rustc unavailable");
        return;
    };
    let paths = vec![a.to_str().unwrap().to_string(), b.to_str().unwrap().to_string()];
    let entry_asm: Vec<Option<String>> = paths.iter().map(|p| extract_bench_entry(p)).collect();
    let text_section: Vec<Option<String>> = paths.iter().map(|p| extract_text_section(p)).collect();
    assert!(
        text_section.iter().all(Option::is_some),
        "expected extract_text_section to succeed on both freshly built dylibs"
    );
    let dupes = duplicate_pairs(&paths, &entry_asm, &text_section);
    assert_eq!(
        dupes.len(),
        1,
        "identical work in two crates of different name length should be flagged as one \
         duplicate pair"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn different_work_is_not_flagged_duplicate() {
    // `do_work` is `#[inline(never)]` in both variants: the real work
    // sits in a symbol `bench_entry` merely calls, not in `bench_entry`
    // itself. The precondition assertion below is what makes this
    // fixture actually exercise that shape rather than passing for the
    // unrelated reason that `bench_entry`'s own disassembly already
    // differed; `matching_entry_with_differing_text_is_not_a_duplicate`
    // above is the deterministic version of this same guarantee.
    let dir = scratch_dir("different");
    let (Some(a), Some(b)) = (
        build_cdylib(&dir, "probe_different_a", &dispatcher_source(ADD_MUL_WORK)),
        build_cdylib(&dir, "probe_different_b", &dispatcher_source(XOR_MUL_WORK)),
    ) else {
        eprintln!("skipping: rustc unavailable");
        return;
    };
    let paths = vec![a.to_str().unwrap().to_string(), b.to_str().unwrap().to_string()];
    let entry_asm: Vec<Option<String>> = paths.iter().map(|p| extract_bench_entry(p)).collect();
    let text_section: Vec<Option<String>> = paths.iter().map(|p| extract_text_section(p)).collect();
    assert!(
        text_section.iter().all(Option::is_some),
        "expected extract_text_section to succeed on both freshly built dylibs"
    );
    assert_eq!(
        entry_asm[0], entry_asm[1],
        "both variants dispatch a single size the same way; bench_entry's own disassembly \
         should be identical so this fixture actually exercises the whole-.text fallback \
         rather than passing because bench_entry alone already differed"
    );
    let dupes = duplicate_pairs(&paths, &entry_asm, &text_section);
    assert!(
        dupes.is_empty(),
        "different `do_work` bodies must not be flagged as duplicates"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ── the names the warnings print ──

/// The duplicate warning exists so an operator can see which two variants
/// collided. `rsplit('/').nth(1)` is the second component from the right, which
/// on the path shape `config::resolve_variant_path` builds
/// (`<benches>/variants/<name>/target/release/lib<name>.dylib`) is `release` for
/// every variant, so the line read `release == release` and named nobody.
#[test]
fn a_variant_is_named_by_its_own_name_not_by_its_build_directory() {
    let a = "/b/variants/alpha/target/release/libalpha.dylib";
    let b = "/b/variants/beta/target/release/libbeta.dylib";
    assert_eq!(super::short_name(a), "alpha");
    assert_eq!(super::short_name(b), "beta");
    assert_ne!(
        super::short_name(a),
        super::short_name(b),
        "two variants must not share one label"
    );
}

/// The other path shape the manifest accepts is a spelled-out relative path,
/// and a bare file name is what a hand-written manifest reaches for. Built from
/// the host's own library prefix and suffix, because that is what every path
/// the harness is handed carries; a `.so` on macOS is not a case that arises.
#[test]
fn short_name_handles_every_path_shape_the_manifest_accepts() {
    let p = std::env::consts::DLL_PREFIX;
    let s = std::env::consts::DLL_SUFFIX;
    assert_eq!(
        super::short_name(&format!("variants/x/target/release/{p}x{s}")),
        "x",
        "the host's prefix and suffix come off"
    );
    assert_eq!(
        super::short_name(&format!("{p}x{s}")),
        "x",
        "bare file name"
    );
    assert_eq!(super::short_name("/a/b/plain"), "plain", "no extension");
    assert_eq!(super::short_name(""), "", "empty is empty, not a panic");
    // A name that merely starts with the prefix letters is not prefixed: only a
    // real `lib` prefix comes off, and what is left must not be empty.
    assert_eq!(
        super::short_name(&format!("{p}{s}")),
        "",
        "prefix and suffix only"
    );
}
