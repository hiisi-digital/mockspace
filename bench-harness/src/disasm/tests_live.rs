//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The duplicate check against real libraries compiled at test time with
//! `rustc` directly (no cargo, no crate deps), read by every disassembler the
//! host has: the one detection picks, GNU and LLVM `objdump` where installed,
//! `otool` on macOS and `dumpbin` on Windows. These run on every platform the
//! benches run on and are what proves a platform there; `tests_recorded.rs`
//! holds the shapes of the platforms this host is not.
//!
//! Nothing here skips. A host with no `rustc` or no disassembler fails, since
//! the duplicate check cannot run there either; a `rustc` that fails to
//! compile the probe panics with its stderr.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::tool::classify_version;
use super::*;

fn scratch_dir(name: &str) -> PathBuf {
    let dir = mockspace_manifest::scratch::held().join(name);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Compile `source` as a cdylib named `crate_name` with `rustc` and `args`,
/// into `file` under `dir`. The source is always `probe.rs`, so `file!()` in
/// any panic location does not vary with the crate name.
fn rustc(dir: &Path, crate_name: &str, source: &str, file: &str, args: &[&str]) -> PathBuf {
    let src_path = dir.join("probe.rs");
    std::fs::write(&src_path, source).expect("write probe source");
    let out_path = dir.join(file);
    let output = Command::new("rustc")
        .args(["--crate-type", "cdylib", "--crate-name", crate_name, "-O", "--edition", "2021"])
        .args(args)
        .arg("-o")
        .arg(&out_path)
        .arg(&src_path)
        .output()
        .unwrap_or_else(|e| panic!("rustc could not be run to build `{crate_name}`: {e}"));
    assert!(
        output.status.success(),
        "rustc failed to build `{crate_name}`:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    out_path
}

/// A host cdylib, named as the host names one.
fn build(dir: &Path, crate_name: &str, source: &str) -> PathBuf {
    let file = format!(
        "{}{crate_name}{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    rustc(dir, crate_name, source, &file, &[])
}

/// A `#[inline(never)]` per-size function, so `bench_entry` stays a thin
/// dispatcher whose own disassembly is the same shell whatever the per-size
/// function computes, as the real macro's is when the optimizer declines to
/// inline. The unmatched arm returns `0` rather than panicking: a formatted
/// `panic!` pulls `core`'s formatting machinery into every library, none of
/// which these tests exist to exercise.
fn dispatcher_source(work_body: &str) -> String {
    format!(
        "#[inline(never)]\n\
         fn do_work(input: &[u8; 64], output: &mut u64) {{\n{work_body}\n}}\n\n\
         #[unsafe(no_mangle)]\n\
         pub unsafe extern \"C\" fn bench_entry(input_ptr: *const u8, output_ptr: *mut u8, n: usize) -> u64 {{\n\
         \x20   match n {{\n\
         \x20       64 => {{\n\
         \x20           let input = unsafe {{ &*(input_ptr as *const [u8; 64]) }};\n\
         \x20           let output = unsafe {{ &mut *(output_ptr as *mut u64) }};\n\
         \x20           do_work(input, output);\n\
         \x20           0\n\
         \x20       }}\n\
         \x20       _ => 0,\n\
         \x20   }}\n\
         }}\n"
    )
}

const ADD_MUL_WORK: &str = "    let mut acc: u64 = 0;\n    for &b in input.iter() {\n        acc = \
                            acc.wrapping_add(b as u64).wrapping_mul(31);\n    }\n    *output = acc;";

const XOR_MUL_WORK: &str = "    let mut acc: u64 = 0xcbf29ce484222325;\n    for &b in input.iter() {\n        \
                            acc ^= b as u64;\n        acc = acc.wrapping_mul(0x100000001b3);\n    }\n    *output = acc;";

/// Whether `program arg` can be spawned at all.
fn spawns(program: &str, arg: &str) -> bool {
    Command::new(program).arg(arg).output().is_ok()
}

/// Every disassembler this host has, each under its own flavour: the one
/// detection picks, a GNU `objdump` and an `llvm-objdump` where installed,
/// `otool` on macOS and `dumpbin` on Windows, so every bench machine proves
/// every flavour it has. Fails on a host with none.
fn tools() -> Vec<Disassembler> {
    let mut found: Vec<Disassembler> = Disassembler::detected().into_iter().cloned().collect();
    let mut add = |program: &str, flavour: Flavour| {
        if !found.iter().any(|t| t.flavour == flavour) {
            found.push(Disassembler {
                program: program.into(),
                flavour,
            });
        }
    };
    for program in ["objdump", "llvm-objdump"] {
        let Ok(out) = Command::new(program).arg("--version").output() else { continue };
        if let Some(flavour) = classify_version(&String::from_utf8_lossy(&out.stdout)) {
            add(program, flavour);
        }
    }
    if cfg!(target_os = "macos") && spawns("otool", "-h") {
        add("otool", Flavour::Otool);
    }
    if cfg!(windows) && spawns("dumpbin", "/?") {
        add("dumpbin", Flavour::Dumpbin);
    }
    assert!(
        !found.is_empty(),
        "this host has no disassembler, so the duplicate check cannot run on it"
    );
    found
}

fn read(tool: &Disassembler, paths: &[String]) -> (Vec<Option<String>>, Vec<Option<String>>) {
    (
        paths.iter().map(|p| extract_bench_entry(tool, p)).collect(),
        paths
            .iter()
            .map(|p| extract_text_section(tool, p))
            .collect(),
    )
}

fn strings(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| p.to_str().unwrap().to_string())
        .collect()
}

#[test]
fn bench_entry_is_found_under_this_platforms_symbol_spelling() {
    let dir = scratch_dir("disasm_entry");
    let lib = build(&dir, "probe_entry", &dispatcher_source(ADD_MUL_WORK));
    for tool in tools() {
        let asm = tool
            .symbol(&lib, "bench_entry")
            .unwrap_or_else(|e| panic!("{:?}: {e}", tool.flavour));
        let normalized = normalize_disasm(&asm, tool.flavour);
        assert!(normalized.lines().count() >= 2, "{:?}: {asm}", tool.flavour);
        assert!(
            tool.symbol(&lib, "bench_entry_absent").is_err(),
            "{:?}",
            tool.flavour
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn identical_work_in_separate_crates_is_flagged_duplicate() {
    // Crate names of different lengths, the shape a real pair of variant
    // names takes: equal lengths would pass without the address folding.
    let dir = scratch_dir("disasm_identical");
    let source = dispatcher_source(ADD_MUL_WORK);
    let paths = strings(&[
        build(&dir, "variant_scalar", &source),
        build(&dir, "variant_unrolled_x4", &source),
    ]);
    for tool in tools() {
        let (entry, text) = read(&tool, &paths);
        assert!(
            text.iter().all(Option::is_some),
            "{:?}: a library was not read",
            tool.flavour
        );
        assert_eq!(
            duplicate_pairs(&paths, &entry, &text).len(),
            1,
            "{:?}: identical work in two crates is one duplicate pair",
            tool.flavour
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn different_work_is_not_flagged_duplicate() {
    let dir = scratch_dir("disasm_different");
    let paths = strings(&[
        build(&dir, "probe_different_a", &dispatcher_source(ADD_MUL_WORK)),
        build(&dir, "probe_different_b", &dispatcher_source(XOR_MUL_WORK)),
    ]);
    for tool in tools() {
        let (entry, text) = read(&tool, &paths);
        assert!(text.iter().all(Option::is_some), "{:?}", tool.flavour);
        // Precondition: the dispatcher shells are identical, so only the
        // whole-code comparison can tell these apart.
        assert!(
            entry[0].is_some(),
            "{:?}: bench_entry was not read",
            tool.flavour
        );
        assert_eq!(
            entry[0], entry[1],
            "{:?}: the entries already differ",
            tool.flavour
        );
        assert!(
            duplicate_pairs(&paths, &entry, &text).is_empty(),
            "{:?}",
            tool.flavour
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The recorded fixtures' libraries, linked live for every target by
/// `rust-lld` and read by `llvm-objdump`: the same decisions
/// `tests_recorded.rs` holds, made over whatever LLVM this host has. Needs the
/// rustup standard library of each target, which no bench machine carries.
#[test]
#[ignore = "install_the_rustup_targets_the_cross_linked_disasm_test_need: needs five rustup targets"]
fn linked_libraries_of_every_target_are_read_and_compared_live() {
    let tool = tools()
        .into_iter()
        .find(|t| t.flavour == Flavour::Llvm)
        .expect("llvm-objdump");
    let dir = scratch_dir("disasm_cross");
    let same = include_str!("fixtures/probe.rs");
    let other = include_str!("fixtures/probe_other.rs");
    let shifted = include_str!("fixtures/probe_shifted.rs");
    for (target, linker) in [
        (
            "x86_64-unknown-linux-gnu",
            &["-C", "linker-flavor=ld.lld"][..],
        ),
        (
            "aarch64-unknown-linux-gnu",
            &["-C", "linker-flavor=ld.lld", "-C", "link-arg=--no-relax"][..],
        ),
        ("x86_64-apple-darwin", &["-C", "linker-flavor=ld64.lld"][..]),
        (
            "aarch64-apple-darwin",
            &["-C", "linker-flavor=ld64.lld", "-C", "link-arg=-ignore_optimization_hints"][..],
        ),
        (
            "x86_64-pc-windows-msvc",
            &["-C", "link-arg=/NODEFAULTLIB", "-C", "link-arg=/NOENTRY"][..],
        ),
    ] {
        let mut args = vec!["--target", target, "-C", "panic=abort", "-C", "linker=rust-lld"];
        args.extend_from_slice(linker);
        let lib = |name: &str, source: &str| {
            rustc(&dir, name, source, &format!("{name}.{target}.lib"), &args)
        };
        let paths = strings(&[
            lib("variant_scalar", same),
            lib("variant_unrolled_x4", same),
            lib("variant_other", other),
            lib("variant_shifted", shifted),
        ]);
        let (entry, text) = read(&tool, &paths);
        assert!(
            entry.iter().chain(&text).all(Option::is_some),
            "{target}: not read"
        );
        assert_eq!(
            build_report(&paths[.. 3], &entry[.. 3], &text[.. 3]).dupes,
            vec![(paths[0].clone(), paths[1].clone())],
            "{target}"
        );
        assert_eq!(
            entry[0], entry[3],
            "{target}: the shifted entry compares different"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
