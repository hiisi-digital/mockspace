//! Automatic disassembly diffing for variant dylibs.
//!
//! Before timing, compare each variant dylib's compiled machine code.
//! If two variants produce identical code, [`check_duplicates`] warns:
//! they will benchmark identically, wasting time.
//!
//! Uses `objdump` (works on Linux + macOS with `llvm-objdump`) with a
//! fallback to `otool -tv` on macOS for older toolchains.
//!
//! The comparison itself has two layers. `bench_entry` (the FFI
//! dispatcher `#[bench_variant]` generates) is cheap to disassemble in
//! isolation and a *different* `bench_entry` proves the variants
//! differ, since the entry is part of the whole `.text` section. It
//! does not prove the converse: the user's per-size function is
//! called through `bench_entry` rather than always inlined into it
//! (a large function, multiple codegen units, or a non-LTO profile
//! can all leave it as a separate symbol), so two variants computing
//! entirely different work can still export a byte-identical
//! dispatcher shell. The authoritative check disassembles the whole
//! `.text` section, which sees the user's function wherever the
//! optimizer put it.

use std::process::Command;

/// The spellings a symbol's C name might carry across platforms.
/// Mach-O prefixes every exported symbol with an underscore
/// (`bench_entry` becomes `_bench_entry`); ELF does not. Try the bare
/// name first (correct on ELF, and objdump accepts it unprefixed
/// there), then the underscore-prefixed form, rather than assuming
/// either spelling.
fn symbol_candidates(symbol: &str) -> [String; 2] {
    [symbol.to_string(), format!("_{symbol}")]
}

/// Disassemble one named symbol from a dylib via `objdump
/// --disassemble-symbols`.
///
/// A candidate that objdump does not find still exits `0`; it only
/// warns on stderr, which this function does not read. So a candidate
/// only counts as found when its own label (`<candidate>:`) actually
/// appears in stdout, not merely when the process exits successfully.
fn objdump_symbol(dylib_path: &str, symbol: &str) -> Option<String> {
    for candidate in symbol_candidates(symbol) {
        let Ok(result) =
            Command::new("objdump")
                .args(["-d", &format!("--disassemble-symbols={candidate}"), dylib_path])
                .output()
        else {
            return None;
        };
        if !result.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&result.stdout).to_string();
        if text.contains(&format!("<{candidate}>")) {
            return Some(normalize_disasm(&text));
        }
    }
    None
}

/// Disassemble the whole file via `objdump -d`, no symbol filter.
fn objdump_all(dylib_path: &str) -> Option<String> {
    let result = Command::new("objdump").args(["-d", dylib_path]).output().ok()?;
    if !result.status.success() {
        return None;
    }
    Some(normalize_disasm(&String::from_utf8_lossy(&result.stdout)))
}

/// Disassemble the whole file via `otool -tv` (macOS fallback).
#[cfg(target_os = "macos")]
fn otool_all(dylib_path: &str) -> Option<String> {
    let result = Command::new("otool").args(["-tv", dylib_path]).output().ok()?;
    if !result.status.success() {
        return None;
    }
    Some(normalize_disasm(&String::from_utf8_lossy(&result.stdout)))
}

/// Normalise disassembly for comparison: strip addresses, raw opcode
/// bytes, and section/label decoration, keeping only mnemonics and
/// operands. Also drops a trailing `<symbol+offset>` annotation
/// objdump/otool append to a jump or call target: that name is the
/// Rust-mangled name of whatever the compiler placed at that address,
/// salted with the crate's own metadata hash, so two variants that
/// compute byte-identical work in separate crates would otherwise
/// compare as different purely because their private helper's mangled
/// name differs.
fn normalize_disasm(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.ends_with(':')
            || trimmed.starts_with("Disassembly")
            || trimmed.starts_with('(')
        {
            continue;
        }
        let kept = if let Some(pos) = trimmed.find('\t') {
            &trimmed[pos + 1 ..]
        } else {
            trimmed
        };
        lines.push(strip_symbol_annotation(kept).to_string());
    }
    lines.join("\n")
}

/// Drop a trailing ` <symbol+offset>` annotation, if the line carries
/// one. See [`normalize_disasm`] for why.
fn strip_symbol_annotation(line: &str) -> &str {
    if line.ends_with('>')
        && let Some(pos) = line.rfind(" <")
    {
        return &line[.. pos];
    }
    line
}

/// Extract machine code for the `bench_entry` symbol from a dylib.
fn extract_bench_entry(dylib_path: &str) -> Option<String> {
    if let Some(asm) = objdump_symbol(dylib_path, "bench_entry") {
        return Some(asm);
    }

    #[cfg(target_os = "macos")]
    {
        let result = Command::new("otool").args(["-tv", dylib_path]).output().ok()?;

        if result.status.success() {
            let text = String::from_utf8_lossy(&result.stdout).to_string();
            return extract_symbol_range(&text, "_bench_entry");
        }
    }

    None
}

/// Extract lines between a symbol label and the next symbol.
#[cfg(target_os = "macos")]
fn extract_symbol_range(text: &str, symbol: &str) -> Option<String> {
    let mut capture = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.contains(symbol) && line.contains(':') {
            capture = true;
            continue;
        }
        if capture {
            // Stop at next symbol (line ending with ':' that isn't an address)
            if line.contains(':') && !line.starts_with(|c: char| c.is_ascii_hexdigit()) {
                break;
            }
            lines.push(line.to_string());
        }
    }
    if lines.is_empty() { None } else { Some(lines.join("\n")) }
}

/// Disassemble the whole `.text` section of a dylib: every function,
/// not just `bench_entry`. See the module docs for why this, rather
/// than `bench_entry` alone, is the authoritative comparison basis.
fn extract_text_section(dylib_path: &str) -> Option<String> {
    if let Some(asm) = objdump_all(dylib_path) {
        return Some(asm);
    }

    #[cfg(target_os = "macos")]
    {
        otool_all(dylib_path)
    }

    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Whether two variants' disassembly describes the same compiled
/// work.
///
/// `entry_a` / `entry_b` are `bench_entry`'s own disassembly: a cheap
/// negative when both are available. Differing entries prove differing
/// `.text` (the entry is part of it), so the pair is not a duplicate
/// regardless of `text_a` / `text_b`, without needing them to have
/// been extractable at all.
///
/// When the entries match, or either is unavailable, the whole `.text`
/// section is the authoritative comparison: it sees the user's
/// per-size function wherever the optimizer placed it, inlined into
/// the dispatcher or standing alone as its own symbol, so two variants
/// computing different work cannot compare equal here even when their
/// dispatcher shells are byte-identical. `None` on either side (the
/// dylib could not be disassembled at all) never counts as a match:
/// absence of evidence is not evidence of duplication.
fn same_work(
    entry_a: &Option<String>,
    entry_b: &Option<String>,
    text_a: &Option<String>,
    text_b: &Option<String>,
) -> bool {
    if let (Some(a), Some(b)) = (entry_a, entry_b)
        && a != b
    {
        return false;
    }
    matches!((text_a, text_b), (Some(a), Some(b)) if a == b)
}

/// Pairwise duplicate decision over precomputed disassembly, factored
/// out of [`check_duplicates`] so it is testable without a dylib on
/// disk. Returns the paths of every duplicate pair.
fn duplicate_pairs(
    variant_paths: &[String],
    entry_asm: &[Option<String>],
    text_section: &[Option<String>],
) -> Vec<(String, String)> {
    let mut dupes = Vec::new();
    for i in 0 .. variant_paths.len() {
        for j in (i + 1) .. variant_paths.len() {
            if same_work(&entry_asm[i], &entry_asm[j], &text_section[i], &text_section[j]) {
                dupes.push((variant_paths[i].clone(), variant_paths[j].clone()));
            }
        }
    }
    dupes
}

/// Compare `bench_entry` disassembly and the whole `.text` section
/// across all variant dylibs. Reports duplicates to stderr.
pub fn check_duplicates(variant_paths: &[String]) {
    if variant_paths.len() < 2 {
        return;
    }

    let entry_asm: Vec<Option<String>> =
        variant_paths.iter().map(|p| extract_bench_entry(p)).collect();
    let text_section: Vec<Option<String>> =
        variant_paths.iter().map(|p| extract_text_section(p)).collect();

    let dupes = duplicate_pairs(variant_paths, &entry_asm, &text_section);

    if !dupes.is_empty() {
        eprintln!(
            "  WARNING: {} variant pair(s) have identical machine code:",
            dupes.len()
        );
        for (a, b) in &dupes {
            let a_short = a.rsplit('/').nth(1).unwrap_or(a);
            let b_short = b.rsplit('/').nth(1).unwrap_or(b);
            eprintln!("    {} == {}", a_short, b_short);
        }
    }
}

#[cfg(test)]
#[path = "disasm/tests.rs"]
mod tests;
