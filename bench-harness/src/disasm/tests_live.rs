//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The duplicate check against real libraries compiled at test time with
//! `rustc` directly (no cargo, no crate deps), read by every disassembler the
//! host has. These run on every platform the benches run on and are what
//! proves a platform there; `tests_recorded.rs` holds the shapes of the
//! platforms this host is not.
//!
//! A test skips, saying so, only where the host lacks `rustc` or the tool it
//! is about. A `rustc` that runs and fails to compile the probe panics with
//! its stderr, so a broken fixture cannot read as a green skip.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::tool::classify_version;
use super::*;

fn scratch_dir(name: &str) -> PathBuf {
    let dir = mockspace_manifest::scratch::held().join(name);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Compile `source` as a cdylib named `crate_name`, for `target` where one is
/// given and for the host otherwise, or as an object with `obj`. The library
/// is named as the target names it: the host's prefix and suffix for a host
/// build.
///
/// `None` only when `rustc` cannot be spawned. The source is always
/// `probe.rs`, so `file!()` in any panic location does not vary with the
/// crate name.
fn build(dir: &Path, crate_name: &str, source: &str, target: Option<&str>) -> Option<PathBuf> {
    let src_path = dir.join("probe.rs");
    std::fs::write(&src_path, source).expect("write probe source");
    let out_path = match target {
        None => {
            dir.join(format!(
                "{}{crate_name}{}",
                std::env::consts::DLL_PREFIX,
                std::env::consts::DLL_SUFFIX
            ))
        },
        Some(t) => dir.join(format!("{crate_name}.{t}.o")),
    };
    let mut cmd = Command::new("rustc");
    cmd.args(["--crate-type", "cdylib", "--crate-name", crate_name, "-O", "--edition", "2021"]);
    if let Some(t) = target {
        cmd.args(["--target", t, "--emit=obj"]);
    }
    let output = match cmd.arg("-o").arg(&out_path).arg(&src_path).output() {
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

/// Every disassembler this host has, each under its own flavour: the one
/// detection picks, and also a GNU `objdump` and an `llvm-objdump` where both
/// are installed, so both flavours are proven where both exist.
fn tools() -> Vec<Disassembler> {
    let mut found: Vec<Disassembler> = Disassembler::detected().into_iter().cloned().collect();
    for program in ["objdump", "llvm-objdump"] {
        let Ok(out) = Command::new(program).arg("--version").output() else { continue };
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        if let Some(flavour) = classify_version(&text)
            && !found.iter().any(|t| t.flavour == flavour)
        {
            found.push(Disassembler {
                program: program.into(),
                flavour,
            });
        }
    }
    if found.is_empty() {
        eprintln!("skipping: this host has no disassembler");
    }
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
    let Some(lib) = build(&dir, "probe_entry", &dispatcher_source(ADD_MUL_WORK), None) else {
        eprintln!("skipping: rustc unavailable");
        return;
    };
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
    let (Some(a), Some(b)) = (
        build(&dir, "variant_scalar", &source, None),
        build(&dir, "variant_unrolled_x4", &source, None),
    ) else {
        eprintln!("skipping: rustc unavailable");
        return;
    };
    let paths = strings(&[a, b]);
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
    let (Some(a), Some(b)) = (
        build(
            &dir,
            "probe_different_a",
            &dispatcher_source(ADD_MUL_WORK),
            None,
        ),
        build(
            &dir,
            "probe_different_b",
            &dispatcher_source(XOR_MUL_WORK),
            None,
        ),
    ) else {
        eprintln!("skipping: rustc unavailable");
        return;
    };
    let paths = strings(&[a, b]);
    for tool in tools() {
        let (entry, text) = read(&tool, &paths);
        assert!(text.iter().all(Option::is_some), "{:?}", tool.flavour);
        // Precondition: the dispatcher shells are identical, so only the
        // whole-code comparison can tell these apart.
        assert_eq!(
            entry[0], entry[1],
            "{:?}: the entries already differ",
            tool.flavour
        );
        assert!(
            entry[0].is_some(),
            "{:?}: bench_entry was not read",
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

/// Objects for every other target whose standard library this toolchain has
/// installed, read by `llvm-objdump`: the Mach-O and COFF shapes, live, where
/// the host can build them. A target without its standard library is
/// skipped by name; `tests_recorded.rs` holds those shapes regardless.
#[test]
fn objects_of_other_targets_are_read_and_compared_live() {
    let Some(tool) = tools().into_iter().find(|t| t.flavour == Flavour::Llvm) else {
        eprintln!("skipping: no llvm-objdump on this host");
        return;
    };
    let dir = scratch_dir("disasm_cross");
    for (target, entry_spelling) in [
        ("x86_64-apple-darwin", "_bench_entry"),
        ("aarch64-apple-darwin", "_bench_entry"),
        ("x86_64-pc-windows-msvc", "bench_entry"),
        ("aarch64-pc-windows-msvc", "bench_entry"),
        ("x86_64-unknown-linux-gnu", "bench_entry"),
        ("aarch64-unknown-linux-gnu", "bench_entry"),
    ] {
        let probe = Command::new("rustc")
            .args(["--print", "target-libdir", "--target", target])
            .output();
        let installed = probe
            .ok()
            .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
            .is_some_and(|libdir| {
                std::fs::read_dir(libdir).is_ok_and(|mut d| {
                    d.any(|e| {
                        e.is_ok_and(|e| e.file_name().to_string_lossy().starts_with("libstd-"))
                    })
                })
            });
        if !installed {
            eprintln!("skipping {target}: its standard library is not installed");
            continue;
        }
        let same = dispatcher_source(ADD_MUL_WORK);
        let objs = [
            build(&dir, "variant_scalar", &same, Some(target)).unwrap(),
            build(&dir, "variant_unrolled_x4", &same, Some(target)).unwrap(),
            build(
                &dir,
                "variant_other",
                &dispatcher_source(XOR_MUL_WORK),
                Some(target),
            )
            .unwrap(),
        ];
        let format = format::ObjectFormat::of_file(&objs[0]).unwrap();
        assert_eq!(format.spell("bench_entry"), entry_spelling, "{target}");
        let paths = strings(&objs);
        let (entry, text) = read(&tool, &paths);
        assert!(
            entry.iter().chain(&text).all(Option::is_some),
            "{target}: not read"
        );
        assert_eq!(
            build_report(&paths, &entry, &text).dupes,
            vec![(paths[0].clone(), paths[1].clone())],
            "{target}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
