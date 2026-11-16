//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Disassembly normalisation: turning raw disassembler output into text that
//! compares equal for two variants computing the same work, and only for
//! those.
//!
//! [`normalize_disasm`] is the entry point every extraction path in
//! `disasm.rs` runs its output through. Everything else here is a pass it
//! composes. Each line comes out as `mnemonic operands`, one space between
//! the two, operands joined by bare commas, whatever spacing the tool used.
//!
//! What is read, per flavour, and where it was checked against real output:
//!
//! - `llvm-objdump` on linked `no_std` libraries of ELF x86_64 and aarch64,
//!   Mach-O x86_64 and arm64, and PE x86_64, recorded in `disasm/fixtures/`:
//!   branch and call targets, RIP-relative displacements, the `add` after an
//!   `adrp`, and salted names, all moving with the layout. An `adrp` page that
//!   itself moves, and otool's decimal page, are held by synthetic lines only.
//! - GNU `objdump` on linked ELF x86_64 and PE x86_64, recorded the same way;
//!   ELF x86_64 `std` libraries live on the host as well.
//! - `otool -tv` and `dumpbin /DISASM`: not reachable from Linux. Their rules
//!   follow the tools' documented line shapes, are held by synthetic lines,
//!   and are proven by the live tests on a macOS or Windows bench machine,
//!   which read the host's libraries with every tool installed there.

use super::tool::Flavour;

/// Normalise disassembly for comparison: drop headers, labels, the address
/// column, raw bytes and comments; fold every branch, call, `adrp` and
/// RIP-relative target to `ADDR`; drop every `<symbol+offset>` annotation.
///
/// Addresses and names are folded for the same reason: unrelated code
/// elsewhere in the binary (a longer crate name, a different dependency
/// graph, anything that shifts where things land in `.text`) moves every
/// address after it, and the compiler salts a private helper's mangled name
/// with the crate's own metadata hash. Neither is a property of the work a
/// variant computes, and left in place either makes two variants computing
/// identical work in separate crates compare as different.
pub(super) fn normalize_disasm(text: &str, flavour: Flavour) -> String {
    let mut lines: Vec<String> = Vec::new();
    // Destination register of the most recently seen `adrp`, so the very next
    // instruction's low-12-bits companion immediate can be recognised as the
    // other half of that same address.
    let mut adrp_reg: Option<String> = None;
    for line in text.lines() {
        if flavour == Flavour::Dumpbin && line.trim() == "Summary" {
            break;
        }
        let Some(insn) = instruction_text(line) else { continue };
        let insn = strip_comment(insn, flavour);
        let insn = fold_annotated_targets(insn);
        let insn = canonical_spacing(&insn);
        if insn.is_empty() {
            continue;
        }
        let mut normalized = fold_rip_relative(&normalize_addresses(&insn));
        if flavour == Flavour::Dumpbin {
            normalized = fold_dumpbin_operands(&normalized);
        }
        if let Some(folded) = fold_adrp_target(&normalized) {
            normalized = folded;
        }
        if let Some(reg) = &adrp_reg
            && let Some(folded) = fold_adrp_companion_immediate(&normalized, reg)
        {
            normalized = folded;
        }
        adrp_reg = adrp_destination(&normalized);
        lines.push(normalized);
    }
    lines.join("\n")
}

/// The instruction part of one output line, or `None` where the line is a
/// header, a label, a section title or blank. The leading address column is
/// taken off: `addr:` (objdump, dumpbin) or a bare address of eight or more
/// hex digits followed by whitespace (otool).
pub(super) fn instruction_text(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.ends_with(':')
        || trimmed.contains("file format")
        || trimmed.starts_with("Disassembly of section")
        || trimmed.starts_with('(')
        || trimmed.starts_with("Dump of file")
        || trimmed.starts_with("File Type:")
    {
        return None;
    }
    let (head, rest) = trimmed
        .split_once(char::is_whitespace)
        .unwrap_or((trimmed, ""));
    let is_hex = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit());
    if let Some(addr) = head.strip_suffix(':')
        && is_hex(addr)
    {
        return Some(rest.trim());
    }
    if head.len() >= 8 && is_hex(head) {
        return Some(rest.trim());
    }
    Some(trimmed)
}

/// Drop a trailing comment: x86 `# ...` (a `#` after whitespace and before a
/// space, another `#` or the end, which no ARM64 `#imm` is), ARM64 `// ...`
/// on ELF, and `; ...`, which LLVM writes on Mach-O arm64 (`; =0`) and otool
/// writes everywhere. A `;` with no whitespace before it (`rep;movsb`) is part
/// of the instruction, except in otool's output.
pub(super) fn strip_comment(insn: &str, flavour: Flavour) -> &str {
    let bytes = insn.as_bytes();
    for i in 0 .. bytes.len() {
        let after_space = i > 0 && bytes[i - 1].is_ascii_whitespace();
        let next = bytes.get(i + 1).copied();
        let cut = match bytes[i] {
            b'#' => after_space && matches!(next, None | Some(b' ' | b'\t' | b'#')),
            b'/' => after_space && next == Some(b'/'),
            b';' => after_space || flavour == Flavour::Otool,
            _ => false,
        };
        if cut {
            return insn[.. i].trim_end();
        }
    }
    insn
}

/// Drop every ` <symbol+offset>` annotation, and fold the address it
/// annotates (`0x11860 <f>` from LLVM, bare `11860 <f>` from GNU) to `ADDR`.
pub(super) fn fold_annotated_targets(insn: &str) -> String {
    let mut out = String::with_capacity(insn.len());
    let mut rest = insn;
    while let Some(open) = rest.find(" <") {
        let Some(close) = rest[open ..].find('>') else { break };
        let before = &rest[.. open];
        let token_start = before.rfind([' ', '\t', ',']).map_or(0, |p| p + 1);
        let token = &before[token_start ..];
        let digits = token.strip_prefix("0x").unwrap_or(token);
        if token_start > 0 && !digits.is_empty() && digits.chars().all(|c| c.is_ascii_hexdigit()) {
            out.push_str(&before[.. token_start]);
            out.push_str("ADDR");
        } else {
            out.push_str(before);
        }
        rest = &rest[open + close + 1 ..];
    }
    out.push_str(rest);
    out
}

/// `mnemonic operands`, one space between, runs of whitespace collapsed, no
/// space after a comma: LLVM writes `cmpq\t$0x40, %rdx`, GNU `cmp    $0x40,%rdx`.
pub(super) fn canonical_spacing(insn: &str) -> String {
    let collapsed = insn.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.replace(", ", ",")
}

/// Replace every free-standing hexadecimal literal in `insn` with `ADDR`: a
/// branch, call or `adrp` target address.
///
/// A hex literal counts as an address, and is folded, unless:
///
/// - it is immediately preceded by `#` (ARM64 immediate) or `$` (x86 AT&T
///   immediate): a semantic constant the algorithm chose (a multiplier, a
///   dispatch size, a seed), and folding it would let two variants computing
///   different work compare equal. The `adrp`/`add` page-offset idiom is the
///   one `#` immediate that is an address; [`fold_adrp_companion_immediate`]
///   handles it with the context this pass lacks.
/// - it sits inside `[...]`, an ARM64 or Intel-syntax memory operand, whose
///   offset selects a field or an element.
/// - it is immediately followed by `(`, an AT&T memory operand's displacement
///   (`0x10(%rax)`); the RIP-relative one is folded by [`fold_rip_relative`].
pub(super) fn normalize_addresses(insn: &str) -> String {
    let bytes = insn.as_bytes();
    let mut out = String::with_capacity(insn.len());
    let mut bracket_depth: i32 = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => {
                bracket_depth += 1;
                out.push('[');
                i += 1;
            },
            b']' => {
                bracket_depth -= 1;
                out.push(']');
                i += 1;
            },
            b'0' if bracket_depth == 0
                && i + 1 < bytes.len()
                && bytes[i + 1] == b'x'
                && (i == 0 || !matches!(bytes[i - 1], b'#' | b'$')) =>
            {
                let start = i;
                i += 2;
                while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'(' {
                    out.push_str(&insn[start .. i]);
                } else {
                    out.push_str("ADDR");
                }
            },
            other => {
                out.push(other as char);
                i += 1;
            },
        }
    }
    out
}

/// Fold a RIP-relative displacement, which is layout (the distance from the
/// instruction to the data it reaches) rather than a field offset: AT&T
/// `0x3af2d(%rip)` and `-0x3af2d(%rip)`, Intel `[rip+0x3af2d]` and
/// `[rip - 0x3af2d]`, and dumpbin `[rip+1234h]`. The sign is layout as much as
/// the magnitude (the data sits before the instruction or after it), so it
/// folds with it.
pub(super) fn fold_rip_relative(insn: &str) -> String {
    let mut out = String::with_capacity(insn.len());
    let mut rest = insn;
    while let Some(at) = rest.find("(%rip)") {
        let head = &rest[.. at];
        let start = head
            .rfind(|c: char| !(c.is_ascii_hexdigit() || c == 'x'))
            .map_or(0, |p| p + 1);
        if head[start ..].starts_with("0x") {
            let start = match head[.. start].ends_with(['-', '+']) {
                true => start - 1,
                false => start,
            };
            out.push_str(&head[.. start]);
            out.push_str("ADDR");
        } else {
            out.push_str(head);
        }
        out.push_str("(%rip)");
        rest = &rest[at + "(%rip)".len() ..];
    }
    out.push_str(rest);
    let mut folded = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open ..].find(']') else { break };
        let inner = &rest[open + 1 .. open + close];
        folded.push_str(&rest[..= open]);
        let tokens: Vec<&str> = inner
            .split(['+', '-', ' '])
            .filter(|t| !t.is_empty())
            .collect();
        if tokens.contains(&"rip") {
            folded.push_str(if tokens.len() > 1 { "rip+ADDR" } else { "rip" });
        } else {
            folded.push_str(inner);
        }
        folded.push(']');
        rest = &rest[open + close + 1 ..];
    }
    folded.push_str(rest);
    folded
}

/// `dumpbin` writes a target as a bare full-width address
/// (`0000000180001010`, sometimes `h`-suffixed) and a call to a named
/// function as its mangled name, salted with the crate's hash. Folded: a
/// long hex token as the operand of a branch or call (`j*`, `call`, `loop*`)
/// and as the whole of a `[...]` operand (an absolute address) to `ADDR`, and a
/// token spelled as a Rust (`_R`, `_ZN`) or MSVC (`?`) mangled name to `SYM`.
/// Every other hex token is an immediate or a displacement the algorithm
/// chose, `mov rax,0CBF29CE484222325h` or `[rax+12345678h]`, and is kept.
pub(super) fn fold_dumpbin_operands(insn: &str) -> String {
    let Some((mnemonic, operands)) = insn.split_once(' ') else {
        return insn.to_string();
    };
    let branch =
        mnemonic.starts_with('j') || mnemonic.starts_with("call") || mnemonic.starts_with("loop");
    let long_hex = |token: &str| {
        let digits = token.strip_suffix('h').unwrap_or(token);
        digits.len() >= 8 && digits.chars().all(|c| c.is_ascii_hexdigit())
    };
    let mut out = format!("{mnemonic} ");
    let mut token = String::new();
    let mut depth = 0;
    let mut lone = true;
    let flush = |token: &mut String, out: &mut String, closing: bool, depth: i32, lone: bool| {
        let address = if depth > 0 { closing && lone } else { branch };
        if address && long_hex(token) {
            out.push_str("ADDR");
        } else if ["_R", "_ZN", "__R", "__ZN", "?"]
            .iter()
            .any(|p| token.starts_with(p))
        {
            out.push_str("SYM");
        } else {
            out.push_str(token);
        }
        token.clear();
    };
    for c in operands.chars() {
        match c {
            '[' => {
                flush(&mut token, &mut out, false, depth, lone);
                depth += 1;
                lone = true;
                out.push(c);
            },
            ']' => {
                flush(&mut token, &mut out, true, depth, lone);
                depth -= 1;
                out.push(c);
            },
            ',' | '+' | '-' | ' ' | '*' => {
                if depth > 0 {
                    lone = false;
                }
                flush(&mut token, &mut out, false, depth, lone);
                out.push(c);
            },
            _ => token.push(c),
        }
    }
    flush(&mut token, &mut out, false, depth, lone);
    out
}

/// An `adrp`'s page operand is always an address, whatever form the tool
/// prints it in: `0x1000` (LLVM, already folded), or a bare page number
/// (`adrp x8, 1` from otool). `adrp reg,PAGE` comes out as `adrp reg,ADDR`.
pub(super) fn fold_adrp_target(insn: &str) -> Option<String> {
    let operands = insn.strip_prefix("adrp ")?;
    let (reg, _) = operands.split_once(',')?;
    Some(format!("adrp {reg},ADDR"))
}

/// If `insn` (already normalised) is an `adrp`, its destination register.
pub(super) fn adrp_destination(insn: &str) -> Option<String> {
    let operands = insn.strip_prefix("adrp ")?;
    Some(operands.split(',').next()?.to_string())
}

/// ARM64 forms a 64-bit address in two instructions: `adrp reg, PAGE` loads
/// the page, then `add reg, reg, #lo12` adds the low 12 bits. The companion's
/// immediate is `#`-prefixed, so [`normalize_addresses`] keeps it as a would-be
/// constant, but it is the low half of the same address and shifts with it
/// whenever anything unrelated moves (observed: two dylibs from identical
/// source but for crate-name length differed only here in `bench_entry`).
///
/// Recognised narrowly, so a genuine immediate on a register an `adrp` last
/// wrote is not folded: `insn` must be `add`, with destination and first
/// source both `adrp_reg` exactly.
///
/// Known gaps, left on purpose because closing them risks the dangerous
/// direction (two variants touching different data comparing equal), where
/// leaving them only risks a genuine duplicate going unreported: an `add`
/// into a different register than its source, and `adrp` followed by a
/// `ldr`/`str` taking the offset inside `[...]`. Both occur in `core`'s panic
/// formatting machinery, which is why the test fixtures' dispatcher returns a
/// bare value from its unmatched arm instead of panicking.
pub(super) fn fold_adrp_companion_immediate(insn: &str, adrp_reg: &str) -> Option<String> {
    let operands = insn.strip_prefix("add ")?;
    let mut parts = operands.splitn(3, ',');
    let dest = parts.next()?;
    let src = parts.next()?;
    let imm = parts.next()?;
    if dest != adrp_reg || src != adrp_reg || !imm.starts_with('#') {
        return None;
    }
    Some(format!("add {dest},{src},#ADDR"))
}
