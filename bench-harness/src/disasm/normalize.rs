//! Disassembly normalisation: turning raw `objdump`/`otool` output into
//! text that compares equal for two variants computing the same work,
//! and only for those. Split from `disasm.rs` per the workspace's
//! file-size discipline.
//!
//! [`normalize_disasm`] is the entry point every extraction path in
//! `disasm.rs` runs its output through. Everything else here is a
//! pass it composes.

/// Normalise disassembly for comparison: strip addresses, raw opcode
/// bytes, and section/label decoration, keeping only mnemonics and
/// operands, with every branch/call/adrp target address folded to a
/// fixed placeholder (see [`normalize_addresses`]) and every trailing
/// `<symbol+offset>` annotation dropped (see
/// [`strip_symbol_annotation`]).
///
/// Both of those exist for the same reason: unrelated code elsewhere
/// in the binary (a longer crate name, a different dependency graph,
/// anything that shifts where things land in `.text`) moves every
/// address after it by a constant amount, and the compiler salts a
/// private, non-inlined helper's mangled name with the crate's own
/// metadata hash. Neither is a property of the work a variant
/// computes; both are layout noise, and left in place either one
/// makes two variants that compute byte-identical work in separate
/// crates compare as different.
pub(super) fn normalize_disasm(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    // Destination register of the most recently seen `adrp`, so the
    // very next instruction's low-12-bits companion immediate can be
    // recognised as the other half of that same address. See
    // `fold_adrp_companion_immediate` for why this needs its own pass.
    let mut adrp_reg: Option<String> = None;
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
        let kept = strip_symbol_annotation(kept);
        let mut normalized = normalize_addresses(kept);
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

/// Drop a trailing ` <symbol+offset>` annotation, if the line carries
/// one. See [`normalize_disasm`] for why.
pub(super) fn strip_symbol_annotation(line: &str) -> &str {
    if line.ends_with('>')
        && let Some(pos) = line.rfind(" <")
    {
        return &line[.. pos];
    }
    line
}

/// Replace every free-standing hexadecimal literal in `operand` with a
/// fixed placeholder: a branch, call, or `adrp` target address. See
/// [`normalize_disasm`] for why these need folding and not just the
/// leading per-line address column.
///
/// A hex literal counts as an address, and gets folded, unless either
/// holds:
///
/// - It is immediately preceded by `#` (ARM64 immediate syntax) or
///   `$` (x86 AT&T immediate syntax). Those are semantic constants the
///   algorithm itself chose (a multiplier, a dispatch size, an XOR
///   seed), not compiled-code layout, and folding them would erase the
///   distinction that makes the whole-`.text` comparison meaningful at
///   all: two variants computing genuinely different work must not
///   compare equal here. (The `adrp`/`add` page-offset idiom is the
///   one shape of `#`-prefixed immediate that is address, not
///   algorithm; [`fold_adrp_companion_immediate`] handles it
///   separately, with the extra context this function does not have.)
/// - It sits inside `[...]` (a memory operand, e.g. `[sp, #0x8]`).
///   The offset that matters there is already `#`-prefixed and caught
///   by the rule above; nothing inside brackets is a branch target.
pub(super) fn normalize_addresses(operand: &str) -> String {
    let bytes = operand.as_bytes();
    let mut out = String::with_capacity(operand.len());
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
                i += 2;
                while i < bytes.len() && (bytes[i] as char).is_ascii_hexdigit() {
                    i += 1;
                }
                out.push_str("ADDR");
            },
            other => {
                out.push(other as char);
                i += 1;
            },
        }
    }
    out
}

/// If `line` (already run through [`normalize_addresses`]) is an
/// `adrp` instruction, its destination register.
pub(super) fn adrp_destination(line: &str) -> Option<String> {
    let (mnemonic, operands) = line.split_once('\t')?;
    if mnemonic != "adrp" {
        return None;
    }
    Some(operands.split(',').next()?.trim().to_string())
}

/// ARM64 forms a 64-bit address in two instructions: `adrp reg, PAGE`
/// loads the page, then a second instruction (canonically `add reg,
/// reg, #lo12`) adds the low 12 bits within that page. The `adrp`
/// operand is already folded to `ADDR` by [`normalize_addresses`], but
/// the companion `add`'s immediate is `#`-prefixed, so that pass
/// leaves it alone as a would-be semantic constant. It is not one: it
/// is the low half of the exact same address, and shifts in lockstep
/// with it whenever anything unrelated moves in the binary (observed
/// directly: two dylibs built from byte-identical source except for
/// crate-name length carried this as the only remaining difference in
/// an otherwise-identical `bench_entry`, which is precisely a
/// genuinely-duplicate pair failing to compare equal).
///
/// Recognised narrowly, to avoid folding a genuine immediate that
/// happens to reuse a register an `adrp` last wrote: `line` must be
/// `add`, and its destination and first source operand must both be
/// `adrp_reg` exactly (`add reg, reg, #imm`), which is the shape an
/// address-formation idiom takes and an unrelated computation on that
/// register essentially never does immediately after that register
/// was just loaded with a page address.
///
/// Known gaps, not fixed here, both left alone on purpose because
/// closing them risks the opposite and more dangerous failure (two
/// variants that access or compute different data comparing as
/// equal), where what they leave behind is only the safe direction (a
/// genuine duplicate occasionally not detected):
///
/// - **`add dest, src, #imm` where `dest` differs from `src`.** Only
///   the `dest == src` accumulate form is recognised. A companion `add`
///   into a fresh register (`add x10, x9, #imm`, `x9` still holding a
///   page base) is just as much an address computation, but is not
///   folded, because the weaker signal (source-only, any destination)
///   is harder to distinguish from a coincidental register reuse this
///   function has no broader context to rule out.
/// - **`adrp`+`ldr`/`str`.** Loading through a page-relative base
///   (rather than materialising the address into a register first)
///   takes the offset inside `[...]`, which [`normalize_addresses`]'s
///   bracket guard deliberately never folds: most bracketed offsets
///   are genuine struct-field or array-index selection.
///
/// Both were observed directly, in the same place: `core`'s panic
/// formatting machinery (pulled in by any `panic!` using format
/// interpolation, regardless of whether the arm ever runs) uses both
/// idioms building its `Arguments`, and an unrelated layout shift
/// elsewhere in the binary (a longer crate name is enough) can move
/// one of those loads across a page boundary. This is why the test
/// fixtures in `disasm/tests.rs` route their dispatcher's unmatched
/// arm through a bare value instead of `panic!(...)`: not to dodge
/// this gap, but because pulling in fifty thousand lines of formatting
/// machinery neither variant's own code touches was never part of
/// what those tests exist to exercise.
pub(super) fn fold_adrp_companion_immediate(line: &str, adrp_reg: &str) -> Option<String> {
    let (mnemonic, operands) = line.split_once('\t')?;
    if mnemonic != "add" {
        return None;
    }
    let mut parts = operands.splitn(3, ',').map(str::trim);
    let dest = parts.next()?;
    let src = parts.next()?;
    let imm = parts.next()?;
    if dest != adrp_reg || src != adrp_reg || !imm.starts_with('#') {
        return None;
    }
    Some(format!("{mnemonic}\t{dest}, {src}, #ADDR"))
}
