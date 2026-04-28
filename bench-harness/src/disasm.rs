//! Automatic disassembly diffing for variant dylibs.
//!
//! Before timing, extract the `bench_entry` function's machine code
//! from each variant dylib. If two variants produce identical
//! machine code, [`check_duplicates`] warns: they will benchmark
//! identically, wasting time.
//!
//! Uses `objdump` (works on Linux + macOS with `llvm-objdump`) with
//! a fallback to `otool -tv` on macOS for older toolchains.

use std::process::Command;

/// Extract machine code bytes for `bench_entry` from a dylib.
fn extract_bench_entry(dylib_path: &str) -> Option<String> {
    let result = Command::new("objdump")
        .args(["-d", "--disassemble-symbols=bench_entry", dylib_path])
        .output()
        .ok()?;

    if result.status.success() {
        let text = String::from_utf8_lossy(&result.stdout).to_string();
        if text.contains("bench_entry") {
            return Some(normalize_disasm(&text));
        }
    }

    #[cfg(target_os = "macos")]
    {
        let result = Command::new("otool")
            .args(["-tv", dylib_path])
            .output()
            .ok()?;

        if result.status.success() {
            let text = String::from_utf8_lossy(&result.stdout).to_string();
            return extract_symbol_range(&text, "_bench_entry");
        }
    }

    None
}

/// Normalise disassembly for comparison: strip addresses, keep only
/// opcodes and operands.
fn normalize_disasm(text: &str) -> String {
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.ends_with(':') || trimmed.starts_with("Disassembly") {
            continue;
        }
        if let Some(pos) = trimmed.find('\t') {
            lines.push(trimmed[pos + 1..].to_string());
        } else {
            lines.push(trimmed.to_string());
        }
    }
    lines.join("\n")
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
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

/// Compare `bench_entry` disassembly across all variant dylibs.
/// Reports duplicates to stderr.
pub fn check_duplicates(variant_paths: &[String]) {
    // (path, normalized_asm)
    let mut entries: Vec<(String, String)> = Vec::new();

    for path in variant_paths {
        if let Some(asm) = extract_bench_entry(path) {
            entries.push((path.clone(), asm));
        }
    }

    if entries.len() < 2 {
        return;
    }

    let mut dupes = Vec::new();
    for i in 0..entries.len() {
        for j in (i + 1)..entries.len() {
            if entries[i].1 == entries[j].1 {
                dupes.push((entries[i].0.clone(), entries[j].0.clone()));
            }
        }
    }

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
