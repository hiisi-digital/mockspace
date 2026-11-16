//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Tests of what the disassembler is asked and how its answer is cut: the
//! object format and symbol spelling read off a file's bytes, one symbol's
//! block out of a whole dump, and the flavour a `--version` text names.

use super::*;

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
    const B: &[&str] = &["bench_entry"];
    const U: &[&str] = &["_bench_entry"];
    // A 32-bit PE image tries the export's bare name first, then the
    // underscored one: no real i686 DLL has been read, only these bytes.
    const BU: &[&str] = &["bench_entry", "_bench_entry"];
    let cases: [(Vec<u8>, Option<&[&str]>); 11] = [
        (b"\x7fELF\x02\x01\x01".to_vec(), Some(B)),
        (vec![0xCF, 0xFA, 0xED, 0xFE, 7, 0, 0, 1], Some(U)),
        (vec![0xCE, 0xFA, 0xED, 0xFE, 7, 0, 0, 0], Some(U)),
        (vec![0xFE, 0xED, 0xFA, 0xCF, 0, 0, 0, 7], Some(U)),
        (vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 2], Some(U)),
        (pe_image(0x8664), Some(B)),
        (pe_image(0xAA64), Some(B)),
        (pe_image(0x014C), Some(BU)),
        (vec![0x64, 0x86, 7, 0], Some(B)),
        (vec![0x4C, 0x01, 7, 0], Some(U)),
        (b"#!/bin/sh\n".to_vec(), None),
    ];
    for (bytes, spelled) in cases {
        let format = ObjectFormat::of_bytes(&bytes);
        assert_eq!(
            format.map(|f| f.spellings("bench_entry")),
            spelled.map(|s| s.iter().map(|x| x.to_string()).collect::<Vec<_>>()),
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

// ── a PE image's function by its export, for GNU objdump and dumpbin ──

/// `dumpbin /HEADERS /EXPORTS` of the recorded
/// `x86_64-pc-windows-msvc/variant_scalar.dll`, written by hand in dumpbin's
/// documented layout from that DLL's export table: no real dumpbin output has
/// been read here (ticket
/// `prove_the_otool_and_dumpbin_disassembly_paths_on_real_hosts`).
const DUMPBIN_EXPORTS: &str = "
Dump of file variant_scalar.dll

PE signature found

File Type: DLL

OPTIONAL HEADER VALUES
             20B magic # (PE32+)
            1000 entry point (0000000180001000)
       180000000 image base (0000000180000000 to 0000000180004FFF)

  Section contains the following exports for variant_scalar.dll

    00000000 characteristics
           0 time date stamp
        0.00 version
           1 ordinal base
           2 number of functions
           2 number of names

    ordinal hint RVA      name

          1    0 00001090 bench_entry
          2    1 000010C0 bench_name

  Summary

        1000 .rdata
        1000 .text
";

#[test]
fn dumpbin_and_gnu_read_one_export_table_and_cut_the_same_function() {
    use super::pe_exports::Exports;
    let fixtures = "fixtures/x86_64-pc-windows-msvc";
    let gnu_p = include_str!("fixtures/x86_64-pc-windows-msvc/gnu-variant_scalar.exports.txt");
    let whole = include_str!("fixtures/x86_64-pc-windows-msvc/llvm-variant_scalar.txt");
    let from_gnu = Exports::from_gnu(gnu_p).expect("objdump -p reads");
    let from_dumpbin = Exports::from_dumpbin(DUMPBIN_EXPORTS).expect("dumpbin reads");
    assert_eq!(
        from_dumpbin, from_gnu,
        "{fixtures}: two readers of one table"
    );
    let block = from_dumpbin
        .block(whole, "bench_entry")
        .expect("bench_entry");
    assert_eq!(block, from_gnu.block(whole, "bench_entry").unwrap());
    // From the export to the next one: `bench_entry` and its padding, and no
    // line of `bench_name`.
    assert!(
        block
            .lines()
            .next()
            .unwrap()
            .trim_start()
            .starts_with("180001090:"),
        "{block}"
    );
    assert!(
        block
            .lines()
            .all(|l| !l.trim_start().starts_with("1800010c0:")),
        "{block}"
    );
    assert!(block.contains("retq"), "{block}");
    assert_eq!(from_dumpbin.block(whole, "_bench_entry"), None);

    // dumpbin's own disassembly lines, `<address>: <instruction>`, cut the same.
    let dumpbin_whole = "  0000000180001000: movzx       eax,byte ptr [rcx]\n\
                         \x20 0000000180001090: sub         rsp,28h\n\
                         \x20 0000000180001094: cmp         r8,8\n\
                         \x20 00000001800010BF: int         3\n\
                         \x20 00000001800010C0: lea         rax,[00000001800020A0h]\n";
    let cut = from_dumpbin.block(dumpbin_whole, "bench_entry").unwrap();
    assert_eq!(cut.lines().count(), 3, "{cut}");
    assert_eq!(
        normalize_disasm(&cut, Flavour::Dumpbin),
        "sub rsp,28h\ncmp r8,8\nint 3"
    );
}
