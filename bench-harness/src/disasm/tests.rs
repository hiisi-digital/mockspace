//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Pure-logic tests for the disassembly duplicate check: normalisation, the
//! duplicate decision and the report, over synthetic strings shaped as each
//! tool writes them. Real output is in `tests_recorded.rs`, real tools on the
//! host in `tests_live.rs`.

use super::normalize::{
    canonical_spacing,
    fold_annotated_targets,
    fold_dumpbin_operands,
    fold_rip_relative,
    instruction_text,
    normalize_addresses,
    strip_comment,
};
use super::*;

const LLVM: Flavour = Flavour::Llvm;
const GNU: Flavour = Flavour::Gnu;

// ── headers, labels and the address column ──

#[test]
fn every_tools_header_and_label_lines_are_dropped() {
    for line in [
        "libvariant_scalar.so:\tfile format elf64-x86-64",
        // GNU pads with spaces, not a tab, so the file name is not after a tab
        // and has to be recognised as a header in its own right.
        "libvariant_scalar.so:     file format elf64-x86-64",
        "Disassembly of section .text:",
        "Disassembly of section .text._RNvCs5ICXHhZ1ia_14variant_scalar7do_work:",
        "Disassembly of section __TEXT,__text:",
        "0000000000011ca0 <bench_entry>:",
        "0000000000000068 <_bench_entry>:",
        "_bench_entry:",
        "libx.dylib:",
        "(__TEXT,__text) section",
        "Dump of file alpha.dll",
        "File Type: DLL",
        "",
    ] {
        assert_eq!(instruction_text(line), None, "{line:?}");
    }
}

#[test]
fn every_tools_address_column_comes_off() {
    for (line, insn) in [
        ("   11ca0:      \tcmpq\t$0x40, %rdx", "cmpq\t$0x40, %rdx"),
        ("   11ca0:\tcmp    $0x40,%rdx", "cmp    $0x40,%rdx"),
        ("       0:      \tldrb\tw8, [x0]", "ldrb\tw8, [x0]"),
        ("0000000000000338\tret", "ret"),
        (
            "  0000000180001000: cmp         rdx,40h",
            "cmp         rdx,40h",
        ),
    ] {
        assert_eq!(instruction_text(line), Some(insn), "{line:?}");
    }
    // A short all-hex mnemonic is not an address.
    assert_eq!(instruction_text("add\tx8, x9, x8"), Some("add\tx8, x9, x8"));
}

#[test]
fn a_gnu_file_header_does_not_make_two_libraries_differ() {
    // The Linux duplicate test failed on exactly this: GNU's header line has no
    // tab, so the old normaliser kept it whole, file name included.
    let a = "\nlibvariant_scalar.so:     file format elf64-x86-64\n\n   0:\tret\n";
    let b = "\nlibvariant_unrolled_x4.so:     file format elf64-x86-64\n\n   0:\tret\n";
    assert_eq!(normalize_disasm(a, GNU), normalize_disasm(b, GNU));
    assert_eq!(normalize_disasm(a, GNU), "ret");
}

// ── comments, annotations, spacing ──

#[test]
fn every_tools_comment_comes_off_and_no_immediate_does() {
    for (insn, kept) in [
        (
            "movq\t0x3af2d(%rip), %rax     # 0x4c6a8 <writev+0x4c6a8>",
            "movq\t0x3af2d(%rip), %rax",
        ),
        (
            "mov    0x3af2d(%rip),%rax        # 4c6a8 <__gmon_start__>",
            "mov    0x3af2d(%rip),%rax",
        ),
        ("mov\tx0, #0x0                ; =0", "mov\tx0, #0x0"),
        ("mov\tx0, #0x10 // =16", "mov\tx0, #0x10"),
        (
            "leaq\tL_x(%rip), %rax ## literal pool",
            "leaq\tL_x(%rip), %rax",
        ),
        ("ldr\tx8, [x8, #0x10]", "ldr\tx8, [x8, #0x10]"),
        ("cmp\tx2, #0x40", "cmp\tx2, #0x40"),
        ("rep;movsb", "rep;movsb"),
    ] {
        assert_eq!(strip_comment(insn, LLVM), kept, "{insn:?}");
    }
    assert_eq!(
        strip_comment("adrp\tx8, 1 ; 0x1000", Flavour::Otool),
        "adrp\tx8, 1"
    );
}

#[test]
fn an_annotated_target_folds_whether_or_not_it_is_written_with_0x() {
    let llvm = "callq\t0x11860 <_RNvCs5ICXHhZ1ia_14variant_scalar7do_work>";
    let gnu = "call   11860 <_RNvCs5ICXHhZ1ia_14variant_scalar7do_work>";
    assert_eq!(fold_annotated_targets(llvm), "callq\tADDR");
    assert_eq!(fold_annotated_targets(gnu), "call   ADDR");
    // The mnemonic itself is never folded, even when it reads as hex.
    assert_eq!(fold_annotated_targets("add <x>"), "add");
}

#[test]
fn llvm_and_gnu_spacing_come_out_the_same() {
    assert_eq!(canonical_spacing("cmpq\t$0x40, %rdx"), "cmpq $0x40,%rdx");
    assert_eq!(canonical_spacing("cmp    $0x40,%rdx"), "cmp $0x40,%rdx");
    assert_eq!(
        canonical_spacing("add\tx8, x9, x8, lsl #5"),
        "add x8,x9,x8,lsl #5"
    );
}

#[test]
fn normalize_disasm_folds_mangled_name_and_shifted_target_together() {
    // Identical work in two crates: a different salted name for the same
    // private helper, and a call target shifted by an unrelated layout change.
    let a = "     8c0: \tbl\t0x880 <__RINvCsfGFIfYg1MGU_10variant_a27do_workKj40_EB2_>\n";
    let b = "     8c0: \tbl\t0x890 <__RINvCsdNdfRXoBgyl_10variant_unrolled_x427do_workKj40_EB2_>\n";
    assert_eq!(normalize_disasm(a, LLVM), normalize_disasm(b, LLVM));
    assert_eq!(normalize_disasm(a, LLVM), "bl ADDR");
}

// ── addresses ──

#[test]
fn normalize_addresses_folds_a_shifted_branch_target() {
    assert_eq!(
        normalize_addresses("b.ne 0xb68"),
        normalize_addresses("b.ne 0xb78")
    );
    assert_eq!(normalize_addresses("b.ne 0xb68"), "b.ne ADDR");
}

#[test]
fn immediates_and_memory_offsets_are_never_folded() {
    // A `#` or `$` immediate is the algorithm's own constant; a bracketed or
    // AT&T displacement selects a field. Folding any of them would let two
    // variants doing different work compare equal.
    for kept in [
        "mov x9,#0x1f",
        "mov $0x40,%eax",
        "str x2,[sp,#0x8]",
        "ldr x0,[x1,0x10]",
        "movq 0x10(%rax),%rbx",
    ] {
        assert_eq!(normalize_addresses(kept), kept);
        assert_eq!(fold_rip_relative(kept), kept);
    }
    assert_ne!(
        normalize_addresses("mov x9,#0x1f"),
        normalize_addresses("mov x9,#0x25")
    );
    assert_ne!(
        normalize_addresses("movq 0x10(%rax),%rbx"),
        normalize_addresses("movq 0x18(%rax),%rbx")
    );
}

#[test]
fn a_rip_relative_displacement_is_layout_and_folds() {
    // The distance from the instruction to its data moves with everything
    // between them, so two crates with identical work differ here.
    let a = normalize_disasm("   1a0:      \tmovq\t0x3af2d(%rip), %rax", LLVM);
    let b = normalize_disasm("   1a0:      \tmovq\t0x3aef6(%rip), %rax", LLVM);
    assert_eq!(a, b);
    assert_eq!(a, "movq ADDR(%rip),%rax");
    assert_eq!(
        normalize_disasm("   1a0:\tmov    0x3af2d(%rip),%rax        # 4c6a8 <x>", GNU),
        "mov ADDR(%rip),%rax"
    );
    assert_eq!(
        fold_rip_relative("lea rax,[rip + 0x3af2d]"),
        "lea rax,[rip + ADDR]"
    );
    assert_eq!(
        fold_rip_relative("lea rax,[rip+0x10]"),
        "lea rax,[rip+ADDR]"
    );
    assert_eq!(
        fold_rip_relative("lea rax,[rax+0x10]"),
        "lea rax,[rax+0x10]"
    );
}

#[test]
fn an_adrp_page_folds_in_every_form_and_its_companion_add_with_it() {
    let llvm = "adrp\tx8, 0x1000 <foo>\nadd\tx8, x8, #0x123\n";
    let otool = "0000000000003f80\tadrp\tx8, 1 ; 0x1000\n0000000000003f84\tadd\tx8, x8, #0x456\n";
    assert_eq!(
        normalize_disasm(llvm, LLVM),
        "adrp x8,ADDR\nadd x8,x8,#ADDR"
    );
    assert_eq!(
        normalize_disasm(otool, Flavour::Otool),
        "adrp x8,ADDR\nadd x8,x8,#ADDR"
    );
    // An `add` into a different register keeps its immediate.
    assert_eq!(
        normalize_disasm("adrp\tx8, 0x1000\nadd\tx9, x8, #0x123\n", LLVM),
        "adrp x8,ADDR\nadd x9,x8,#0x123"
    );
}

#[test]
fn dumpbin_full_width_addresses_and_mangled_names_fold() {
    assert_eq!(fold_dumpbin_operands("jne 0000000180001010"), "jne ADDR");
    assert_eq!(
        fold_dumpbin_operands("call _RNvCs5ICXHhZ1ia_14variant_scalar7do_work"),
        "call SYM"
    );
    assert_eq!(
        fold_dumpbin_operands("lea rcx,[00000001800020A0h]"),
        "lea rcx,[ADDR]"
    );
    assert_eq!(fold_dumpbin_operands("cmp rdx,40h"), "cmp rdx,40h");
    assert_eq!(fold_dumpbin_operands("ret"), "ret");
    let text = "  0000000180001000: cmp         rdx,40h\n  0000000180001004: jne         \
                0000000180001010\n\n  Summary\n\n        1000 .data\n";
    assert_eq!(
        normalize_disasm(text, Flavour::Dumpbin),
        "cmp rdx,40h\njne ADDR"
    );
}

#[test]
fn normalize_disasm_keeps_a_genuinely_different_instruction_different() {
    let xor_mul = "     8b8: \teor\tx8, x8, x11\n";
    let add_mul = "     8b8: \tadd\tx8, x8, x9\n";
    assert_ne!(
        normalize_disasm(xor_mul, LLVM),
        normalize_disasm(add_mul, LLVM)
    );
}

#[test]
fn output_with_no_instructions_is_no_evidence() {
    // A tool that runs and prints only headers must not hand back an empty
    // string two variants would then share.
    let headers = "\nlibx.so:     file format elf64-x86-64\n\nDisassembly of section .text:\n";
    assert_eq!(non_empty(normalize_disasm(headers, GNU)), None);
    assert_eq!(non_empty("ret".to_string()), Some("ret".to_string()));
}

// ── same_work / duplicate_pairs ──

fn some(s: &str) -> Option<String> {
    Some(s.to_string())
}

#[test]
fn same_entry_and_same_text_is_a_duplicate() {
    assert!(same_work(&some("a"), &some("a"), &some("x"), &some("x")));
}

#[test]
fn differing_entry_is_never_a_duplicate_even_if_text_matches() {
    assert!(!same_work(&some("a"), &some("b"), &some("x"), &some("x")));
}

#[test]
fn matching_entry_with_differing_text_is_not_a_duplicate() {
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

// ── CheckReport: "could not answer" is distinguishable from "no duplicates" ──

#[test]
fn a_genuine_clean_pass_has_no_unreadable_variants() {
    let paths = vec!["a".to_string(), "b".to_string()];
    let report = build_report(&paths, &[some("x"), some("y")], &[
        some("alpha"),
        some("beta"),
    ]);
    assert!(report.dupes.is_empty());
    assert!(report.unreadable.is_empty());
}

#[test]
fn total_extraction_failure_is_not_a_clean_pass() {
    let paths = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let report = build_report(&paths, &[None, None, None], &[None, None, None]);
    assert!(report.dupes.is_empty());
    assert_eq!(report.unreadable, paths);
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

// ── the names the warnings print ──

/// The duplicate warning exists so an operator can see which two variants
/// collided, and the name is read off the library's own suffix: a `.dylib`
/// path names its arm on a Linux host as well, which is where this used to
/// print `alpha.dylib`.
#[test]
fn a_variant_is_named_by_its_own_name_not_by_its_build_directory() {
    for (a, b) in [
        (
            "/b/variants/alpha/target/release/libalpha.dylib",
            "/b/variants/beta/target/release/libbeta.dylib",
        ),
        (
            "/b/variants/alpha/target/release/libalpha.so",
            "/b/variants/beta/target/release/libbeta.so",
        ),
        (
            "C:\\b\\variants\\alpha\\target\\release\\alpha.dll",
            "C:\\b\\variants\\beta\\target\\release\\beta.dll",
        ),
    ] {
        assert_eq!(short_name(a), "alpha");
        assert_eq!(short_name(b), "beta");
    }
}

#[test]
fn short_name_handles_every_path_shape_the_manifest_accepts() {
    let p = std::env::consts::DLL_PREFIX;
    let s = std::env::consts::DLL_SUFFIX;
    assert_eq!(
        short_name(&format!("variants/x/target/release/{p}x{s}")),
        "x"
    );
    assert_eq!(short_name(&format!("{p}x{s}")), "x");
    assert_eq!(short_name("/a/b/plain"), "plain");
    assert_eq!(short_name(""), "");
}

// ── object formats and symbol spelling ──

/// A PE image's first bytes: `MZ`, `e_lfanew` at 0x3c, then `PE\0\0` and
/// the machine.
fn pe_image(machine: u16) -> Vec<u8> {
    let mut b = vec![0u8; 0x48];
    b[0 .. 2].copy_from_slice(b"MZ");
    b[0x3C .. 0x40].copy_from_slice(&0x40u32.to_le_bytes());
    b[0x40 .. 0x44].copy_from_slice(b"PE\0\0");
    b[0x44 .. 0x46].copy_from_slice(&machine.to_le_bytes());
    b
}

#[test]
fn every_format_is_read_off_its_own_leading_bytes_and_spells_a_symbol_its_way() {
    use super::format::ObjectFormat;
    let cases: [(Vec<u8>, Option<&str>); 11] = [
        (b"\x7fELF\x02\x01\x01".to_vec(), Some("bench_entry")),
        (
            vec![0xCF, 0xFA, 0xED, 0xFE, 7, 0, 0, 1],
            Some("_bench_entry"),
        ),
        (
            vec![0xCE, 0xFA, 0xED, 0xFE, 7, 0, 0, 0],
            Some("_bench_entry"),
        ),
        (
            vec![0xFE, 0xED, 0xFA, 0xCF, 0, 0, 0, 7],
            Some("_bench_entry"),
        ),
        (
            vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 2],
            Some("_bench_entry"),
        ),
        (pe_image(0x8664), Some("bench_entry")),
        (pe_image(0xAA64), Some("bench_entry")),
        (pe_image(0x014C), Some("_bench_entry")),
        (vec![0x64, 0x86, 7, 0], Some("bench_entry")),
        (vec![0x4C, 0x01, 7, 0], Some("_bench_entry")),
        (b"#!/bin/sh\n".to_vec(), None),
    ];
    for (bytes, spelled) in cases {
        let format = ObjectFormat::of_bytes(&bytes);
        assert_eq!(
            format.map(|f| f.spell("bench_entry")).as_deref(),
            spelled,
            "{bytes:02x?}"
        );
    }
    // `MZ` with no `PE\0\0` where `e_lfanew` points is no PE image.
    let mut broken = pe_image(0x8664);
    broken[0x40] = b'X';
    assert_eq!(ObjectFormat::of_bytes(&broken), None);
}

// ── one symbol's block, for the tools with no symbol filter ──

#[test]
fn a_symbols_block_runs_from_its_exact_label_to_the_next_label() {
    use super::tool::symbol_block;
    let otool = "libx.dylib:\n(__TEXT,__text) section\n_bench_entry_helper:\n0000000000000300\tnop\n\
                 _bench_entry:\n0000000000000338\tcmp\tx2, #0x40\n000000000000033c\tret\n\
                 _do_work:\n0000000000000340\tret\n";
    assert_eq!(
        symbol_block(otool, "_bench_entry").as_deref(),
        Some("_bench_entry:\n0000000000000338\tcmp\tx2, #0x40\n000000000000033c\tret")
    );
    let dumpbin = "Dump of file alpha.dll\n\nFile Type: DLL\n\n  bench_entry:\n  0000000180001000: cmp \
                   rdx,40h\n  0000000180001004: ret\n\n  do_work:\n  0000000180001010: ret\n";
    assert_eq!(
        symbol_block(dumpbin, "bench_entry").as_deref(),
        Some("  bench_entry:\n  0000000180001000: cmp rdx,40h\n  0000000180001004: ret")
    );
    let objdump = "0000000000011ca0 <bench_entry_helper>:\n   11ca0:\tret\n\n\
                   0000000000011cb0 <bench_entry>:\n   11cb0:\tret\n\nDisassembly of section .fini:\n";
    assert_eq!(
        symbol_block(objdump, "bench_entry").as_deref(),
        Some("0000000000011cb0 <bench_entry>:\n   11cb0:\tret")
    );
    // A label that only starts or ends with the name is another function, and
    // a call annotation naming it is no label.
    assert_eq!(
        symbol_block("   0:\tcall 0x10 <bench_entry>\n", "bench_entry"),
        None
    );
    assert_eq!(symbol_block(otool, "bench_entry"), None);
}

#[test]
fn a_version_text_names_its_flavour() {
    use super::tool::classify_version;
    assert_eq!(
        classify_version("GNU objdump (GNU Binutils for Ubuntu) 2.42"),
        Some(Flavour::Gnu)
    );
    assert_eq!(
        classify_version("Ubuntu LLVM version 18.1.3\n  Optimized build."),
        Some(Flavour::Llvm)
    );
    assert_eq!(
        classify_version("Apple LLVM version 15.0.0 (clang-1500.3.9.4)"),
        Some(Flavour::Llvm)
    );
    assert_eq!(classify_version("objdump: unknown option"), None);
}
