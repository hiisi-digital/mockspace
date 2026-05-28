#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

//! Mockspace built-in lints as a viola plugin cdylib.
//!
//! First slice of the lint catalogue port (workspace task #610). Exports
//! one provider per built-in lint via the host-owned-buffer
//! `viola.lint.evaluate.v2` vtable from `viola-plugin-abi`. The first
//! port is `no-todo`; later rounds add the remaining built-ins as more
//! providers in this crate's descriptor.
//!
//! # Wire shape
//!
//! Each lint is one `ProviderEntry` in the exported descriptor. The
//! entry's id is the per-lint id (`mockspace-builtin.lint.<name>.v2`);
//! the vtable pointer addresses a `LintEvaluateVtable` static. The
//! evaluator reads source bytes from the NAM v1.x file slice, scans each
//! file, and writes `Diagnostic` records directly into the host-provided
//! output buffer. The plugin keeps no state across calls, so the host
//! may run invocations in parallel with separate buffers.

#[cfg(test)]
extern crate std;

// Force libc into the link so libSystem (darwin) / libc (linux) resolves the
// `memcpy` / `memcmp` intrinsics core emits and `dyld_stub_binder`. The crate
// calls no libc symbols; the `extern crate` is what makes the linker pull the
// system library under `-nodefaultlibs`.
#[cfg(all(unix, not(test)))]
extern crate libc as _;

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// no_std cdylib without panic-unwind still references the EH personality
// fn through the link table; provide an empty stub so the linker resolves.
// rust_eh_personality must exist in every no_std cdylib that does not
// unwind; the symbol name is fixed by the Rust ABI.
#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn rust_eh_personality() {}

use core::ffi::c_void;

use hilavitkutin_extensions::{ProviderExport, ProviderId};
use hilavitkutin_extensions_macros::export_extension;
use viola_plugin_abi::{
    AbiStatus, BytesRef, Diagnostic, DiagnosticSeverity, LintEvaluateVtable,
    NamPayload, SourceLocation, SourceRange, nam_file_entries,
};

// ---------------------------------------------------------------------------
// no-todo
// ---------------------------------------------------------------------------

/// Provider id for the no-todo lint.
///
/// Per task #610 R4 (per-lint id namespaced by pack origin): the
/// mockspace built-in pool uses the `mockspace-builtin.lint.<name>.v2`
/// shape. `.v2` tracks the host-owned-buffer vtable (Decision 1 / Option
/// B of the locked cdylib-port decisions).
pub const PROVIDER_NO_TODO: ProviderId =
    ProviderId::from_name("mockspace-builtin.lint.no-todo.v2");

/// The marker token the lint flags. Built from separate byte literals so
/// the four-letter token never appears contiguously in this source (the
/// lint scans shipped source, including this crate).
const NEEDLE: &[u8] = &[b'T', b'O', b'D', b'O'];

const PLUGIN_ID: &[u8] = b"mockspace-builtin-lints";
const RULE_ID: &[u8] = b"no-todo";
const MESSAGE: &[u8] = b"todo marker found in shipped source";

/// Evaluator for the no-todo lint.
///
/// Walks the NAM v1.x file entries, scans each file's source for the
/// word-bounded marker token, and writes one [`Diagnostic`] per match
/// into the host-owned `out_entries` buffer. Returns
/// [`AbiStatus::InvalidArg`] when `out_entries` or `out_len` is null, or
/// the NAM payload is not a v1.x carrier (matching the v2 fixture
/// contract; the host always passes a non-null buffer pointer).
///
/// Overflow contract (the v2 host-owned-buffer rule): write up to
/// `out_capacity` entries, set `*out_len` to the count the lint would
/// have emitted, and return [`AbiStatus::Internal`] when that count
/// exceeds `out_capacity`; otherwise return [`AbiStatus::Ok`]. The host
/// reads `*out_len > out_capacity` as the truncation signal.
///
/// SAFETY: the host upholds the `LintEvaluateVtable` contract. `nam` is a
/// valid v1.x payload or null; `out_entries` addresses `out_capacity`
/// writable `Diagnostic` slots; `out_len` is a valid writable pointer.
unsafe extern "C" fn no_todo_evaluate(
    _host_ctx: *mut c_void,
    nam: *const NamPayload,
    _lint_config_bytes: *const u8,
    _lint_config_len: arvo::USize,
    out_entries: *mut Diagnostic,
    out_capacity: arvo::USize,
    out_len: *mut arvo::USize,
) -> AbiStatus {
    if out_entries.is_null() || out_len.is_null() {
        return AbiStatus::InvalidArg;
    }
    // SAFETY: host upholds nam validity; the accessor returns None for a
    // non-v1.x carrier, treated as InvalidArg.
    let entries = match unsafe { nam_file_entries(nam) } {
        Some(slice) => slice,
        None => return AbiStatus::InvalidArg,
    };

    let capacity = out_capacity.0;
    let mut written: usize = 0;
    let mut would_emit: usize = 0;

    for entry in entries {
        if entry.source.is_empty() {
            continue;
        }
        // SAFETY: NamFileEntry.source addresses host-owned bytes valid for
        // the call; its byte length is entry.source.len.
        let source: &[u8] = unsafe {
            core::slice::from_raw_parts(entry.source.data, entry.source.len.0)
        };

        for_each_match(source, |line, column| {
            if would_emit < capacity {
                let diag = make_diagnostic(entry.path, line, column);
                // SAFETY: written == would_emit here and would_emit <
                // capacity, so the slot is inside the host buffer; the
                // host owns it for the call's duration.
                unsafe {
                    out_entries.add(written).write(diag);
                }
                written += 1;
            }
            would_emit += 1;
        });
    }

    // SAFETY: out_len is non-null (checked above), host-owned.
    unsafe {
        *out_len = arvo::USize(would_emit);
    }

    if would_emit > capacity {
        AbiStatus::Internal
    } else {
        AbiStatus::Ok
    }
}

// ---------------------------------------------------------------------------
// scanning core (pure, panic-free, unit-tested)
// ---------------------------------------------------------------------------

/// Invoke `emit(line, column)` for each word-bounded occurrence of the
/// marker in `source`. Line is 1-based, column 0-based, per the NAM
/// position convention. Word-bounded means the byte before the match and
/// the byte after it are not word bytes (mirrors the `\bTODO\b` regex the
/// in-process lint uses), so `TODOS` and `XTODO` do not match.
fn for_each_match(source: &[u8], mut emit: impl FnMut(u32, u32)) {
    let n = NEEDLE.len();
    let mut i: usize = 0;
    while i + n <= source.len() {
        if &source[i..i + n] == NEEDLE
            && left_boundary(source, i)
            && right_boundary(source, i + n)
        {
            let (line, column) = byte_offset_to_line_col(source, i);
            emit(line, column);
            i += n;
        } else {
            i += 1;
        }
    }
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn left_boundary(source: &[u8], start: usize) -> bool {
    start == 0 || !is_word_byte(source[start - 1])
}

fn right_boundary(source: &[u8], end: usize) -> bool {
    end >= source.len() || !is_word_byte(source[end])
}

/// Convert a byte offset to a 1-based line and 0-based column.
fn byte_offset_to_line_col(source: &[u8], offset: usize) -> (u32, u32) {
    let mut line: u32 = 1;
    let mut column: u32 = 0;
    let mut i: usize = 0;
    while i < offset && i < source.len() {
        if source[i] == b'\n' {
            line += 1;
            column = 0;
        } else {
            column += 1;
        }
        i += 1;
    }
    (line, column)
}

// NOTE on `path` ownership: the diagnostic's `path` aliases the NAM
// entry's `path` BytesRef, which points at HOST-owned memory valid only
// for this call (see viola-plugin-abi nam.rs). This differs from the
// general Diagnostic contract, where BytesRef slots point at PLUGIN-owned
// memory stable until shutdown. The host therefore must copy `path` bytes
// during the call if it retains the diagnostic past the call's return. The
// current host reads only `severity` by value, so this is sound today; it
// is the constraint every cdylib port that emits NAM-sourced paths shares.
fn make_diagnostic(path: BytesRef, line: u32, column: u32) -> Diagnostic {
    Diagnostic {
        plugin_id: bytes_ref_static(PLUGIN_ID),
        rule_id: bytes_ref_static(RULE_ID),
        severity: DiagnosticSeverity::Warn,
        message: bytes_ref_static(MESSAGE),
        path,
        range: SourceRange {
            start: SourceLocation { line, column },
            end: SourceLocation {
                line,
                column: column + NEEDLE.len() as u32,
            },
        },
        suggestion: BytesRef::EMPTY,
        metadata_schema: ProviderId(0),
        metadata_ptr: core::ptr::null(),
        metadata_len: arvo::USize(0),
    }
}

const fn bytes_ref_static(b: &'static [u8]) -> BytesRef {
    BytesRef {
        data: b.as_ptr(),
        len: arvo::USize(b.len()),
    }
}

// ---------------------------------------------------------------------------
// file-size
// ---------------------------------------------------------------------------

/// Provider id for the file-size lint (the `file-metric` primitive,
/// line-count metrics only).
pub const PROVIDER_FILE_SIZE: ProviderId =
    ProviderId::from_name("mockspace-builtin.lint.file-size.v2");

const FILE_SIZE_RULE_ID: &[u8] = b"file-size";
const FILE_SIZE_MESSAGE: &[u8] = b"file line count exceeds the configured threshold";

/// Line-count metric discriminants, the pure-source-bytes subset of
/// `mockspace_rs::builtins::file_metric::Metric`. The item-count variants
/// need a parsed AST and ship with the bucket-2 ports; the host must not
/// route them here, and an out-of-range discriminant is rejected.
pub const METRIC_LINE_COUNT: u32 = 0;
pub const METRIC_NON_BLANK_LINE_COUNT: u32 = 1;
pub const METRIC_NON_BLANK_NON_COMMENT_LINE_COUNT: u32 = 2;

/// Fixed-layout config the host passes via `lint_config_bytes`.
///
/// This is the FIXED-config arm of the config-bytes contract: the host
/// encodes the per-lint TOML into this `#[repr(C)]` struct and the cdylib
/// reads it back by a pointer cast when `config_len` equals its size. A
/// no_std cdylib cannot parse TOML, so config crosses pre-encoded.
/// Variable-length configs (e.g. a token list) use a length-prefixed blob
/// instead, decoded by hand; that arm lands with the token-scan port.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct FileSizeConfig {
    /// One of the `METRIC_*` line-count discriminants.
    pub metric: u32,
    /// Non-zero fires when count >= threshold; zero when count > threshold.
    pub inclusive: u32,
    pub threshold: arvo::USize,
}

/// Evaluator for the file-size lint. Counts lines per the configured
/// metric and emits one diagnostic per file whose count crosses the
/// threshold.
///
/// Config handling: empty config (`config_len == 0`) is a no-op (the lint
/// is present but not configured). A `config_len` that is neither 0 nor
/// `size_of::<FileSizeConfig>()`, a null config pointer alongside a
/// non-zero length, or an out-of-range metric discriminant all return
/// [`AbiStatus::InvalidArg`]. The overflow + null-arg contract is
/// identical to [`no_todo_evaluate`].
///
/// SAFETY: the host upholds the `LintEvaluateVtable` contract.
unsafe extern "C" fn file_size_evaluate(
    _host_ctx: *mut c_void,
    nam: *const NamPayload,
    lint_config_bytes: *const u8,
    lint_config_len: arvo::USize,
    out_entries: *mut Diagnostic,
    out_capacity: arvo::USize,
    out_len: *mut arvo::USize,
) -> AbiStatus {
    if out_entries.is_null() || out_len.is_null() {
        return AbiStatus::InvalidArg;
    }
    if lint_config_len.0 == 0 {
        // present but not configured: no findings.
        // SAFETY: out_len is non-null (checked above).
        unsafe {
            *out_len = arvo::USize(0);
        }
        return AbiStatus::Ok;
    }
    if lint_config_bytes.is_null()
        || lint_config_len.0 != core::mem::size_of::<FileSizeConfig>()
    {
        return AbiStatus::InvalidArg;
    }
    // SAFETY: config_bytes is non-null and exactly FileSizeConfig-sized;
    // the host encoded a FileSizeConfig. read_unaligned makes no alignment
    // assumption about the host buffer.
    let config = unsafe { (lint_config_bytes as *const FileSizeConfig).read_unaligned() };
    if config.metric > METRIC_NON_BLANK_NON_COMMENT_LINE_COUNT {
        return AbiStatus::InvalidArg;
    }

    // SAFETY: host upholds nam validity; None for a non-v1.x carrier.
    let entries = match unsafe { nam_file_entries(nam) } {
        Some(slice) => slice,
        None => return AbiStatus::InvalidArg,
    };

    let capacity = out_capacity.0;
    let mut written: usize = 0;
    let mut would_emit: usize = 0;

    for entry in entries {
        let count = if entry.source.is_empty() {
            0
        } else {
            // SAFETY: a non-empty NamFileEntry.source addresses host-owned
            // bytes valid for the call; its byte length is entry.source.len.
            let source: &[u8] = unsafe {
                core::slice::from_raw_parts(entry.source.data, entry.source.len.0)
            };
            line_metric(source, config.metric)
        };
        let triggered = if config.inclusive != 0 {
            count >= config.threshold.0
        } else {
            count > config.threshold.0
        };
        if !triggered {
            continue;
        }
        if would_emit < capacity {
            // path aliases host-owned, call-scoped NAM memory; see the note
            // on make_diagnostic.
            let diag = make_file_diagnostic(entry.path);
            // SAFETY: written == would_emit < capacity, so the slot is
            // inside the host buffer.
            unsafe {
                out_entries.add(written).write(diag);
            }
            written += 1;
        }
        would_emit += 1;
    }

    // SAFETY: out_len is non-null (checked above).
    unsafe {
        *out_len = arvo::USize(would_emit);
    }
    if would_emit > capacity {
        AbiStatus::Internal
    } else {
        AbiStatus::Ok
    }
}

fn make_file_diagnostic(path: BytesRef) -> Diagnostic {
    Diagnostic {
        plugin_id: bytes_ref_static(PLUGIN_ID),
        rule_id: bytes_ref_static(FILE_SIZE_RULE_ID),
        severity: DiagnosticSeverity::Warn,
        message: bytes_ref_static(FILE_SIZE_MESSAGE),
        path,
        // whole-file finding: a zero-width point at the file start.
        range: SourceRange {
            start: SourceLocation { line: 1, column: 0 },
            end: SourceLocation { line: 1, column: 0 },
        },
        suggestion: BytesRef::EMPTY,
        metadata_schema: ProviderId(0),
        metadata_ptr: core::ptr::null(),
        metadata_len: arvo::USize(0),
    }
}

/// Count lines per the metric discriminant. Line splitting matches
/// `str::lines()`: lines split on `\n`, a trailing `\r` is trimmed, and a
/// final trailing newline does not add an empty line.
fn line_metric(source: &[u8], metric: u32) -> usize {
    let mut count: usize = 0;
    for line in Lines::new(source) {
        let trimmed = line.trim_ascii();
        let included = match metric {
            METRIC_NON_BLANK_LINE_COUNT => !trimmed.is_empty(),
            METRIC_NON_BLANK_NON_COMMENT_LINE_COUNT => {
                !trimmed.is_empty() && !trimmed.starts_with(b"//")
            }
            // METRIC_LINE_COUNT and any already-validated discriminant.
            _ => true,
        };
        if included {
            count += 1;
        }
    }
    count
}

/// Iterator over the lines of a byte slice with `str::lines()` semantics.
struct Lines<'a> {
    rest: &'a [u8],
}

impl<'a> Lines<'a> {
    fn new(source: &'a [u8]) -> Self {
        Self { rest: source }
    }
}

impl<'a> Iterator for Lines<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        if self.rest.is_empty() {
            return None;
        }
        match self.rest.iter().position(|&b| b == b'\n') {
            Some(idx) => {
                let mut line = &self.rest[..idx];
                if line.last() == Some(&b'\r') {
                    line = &line[..line.len() - 1];
                }
                self.rest = &self.rest[idx + 1..];
                Some(line)
            }
            None => {
                let line = self.rest;
                self.rest = &[];
                Some(line)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// token-scan (no-std lint)
// ---------------------------------------------------------------------------

/// Provider id for the no-std lint (the `token-scan` primitive).
pub const PROVIDER_NO_STD: ProviderId =
    ProviderId::from_name("mockspace-builtin.lint.no-std.v2");

const NO_STD_RULE_ID: &[u8] = b"no-std";
const TOKEN_SCAN_MESSAGE: &[u8] = b"forbidden token matched outside strings and comments";

/// Upper bound on the token count a single token-scan config may carry.
/// The largest catalogue preset lists eight tokens; 32 leaves headroom
/// without an allocation.
const MAX_TOKENS: usize = 32;

/// Decoded strip flags from the variable-config blob.
#[derive(Copy, Clone)]
struct TokenScanFlags {
    word_boundary: bool,
    strip_strings: bool,
    strip_comments: bool,
    strip_doc_comments: bool,
}

/// Evaluator for the no-std lint. Thin wrapper that bakes the rule id and
/// delegates to the shared token-scan core; future token-scan lints add
/// their own wrapper + provider and reuse the core.
///
/// SAFETY: the host upholds the `LintEvaluateVtable` contract.
unsafe extern "C" fn no_std_evaluate(
    _host_ctx: *mut c_void,
    nam: *const NamPayload,
    lint_config_bytes: *const u8,
    lint_config_len: arvo::USize,
    out_entries: *mut Diagnostic,
    out_capacity: arvo::USize,
    out_len: *mut arvo::USize,
) -> AbiStatus {
    // SAFETY: forwards the host-upheld pointers unchanged.
    unsafe {
        token_scan_core(
            nam,
            lint_config_bytes,
            lint_config_len,
            out_entries,
            out_capacity,
            out_len,
            NO_STD_RULE_ID,
        )
    }
}

/// Shared token-scan logic. Decodes the variable-config blob (the VARIABLE
/// arm of the config-bytes contract), scans each NAM file for each literal
/// token outside strings and comments (per the strip flags), and emits one
/// diagnostic per match carrying `rule_id`.
///
/// Config blob (little-endian): `[word_boundary: u8][strip_strings: u8]
/// [strip_comments: u8][strip_doc_comments: u8][token_count: u32]` then
/// `token_count` entries of `[len: u32][bytes...]`. Empty config
/// (`config_len == 0`) is a no-op (lint present but not configured).
/// Malformed config (short header, `token_count > MAX_TOKENS`, a token
/// length running past the buffer) returns `AbiStatus::InvalidArg`. The
/// overflow + null-arg contract matches the other evaluators.
///
/// SAFETY: the host upholds the `LintEvaluateVtable` contract; `rule_id`
/// is a static byte slice.
unsafe fn token_scan_core(
    nam: *const NamPayload,
    lint_config_bytes: *const u8,
    lint_config_len: arvo::USize,
    out_entries: *mut Diagnostic,
    out_capacity: arvo::USize,
    out_len: *mut arvo::USize,
    rule_id: &'static [u8],
) -> AbiStatus {
    if out_entries.is_null() || out_len.is_null() {
        return AbiStatus::InvalidArg;
    }
    if lint_config_len.0 == 0 {
        // SAFETY: out_len non-null.
        unsafe {
            *out_len = arvo::USize(0);
        }
        return AbiStatus::Ok;
    }
    if lint_config_bytes.is_null() {
        return AbiStatus::InvalidArg;
    }
    // SAFETY: config_bytes non-null with config_len bytes, host-owned for
    // the call.
    let config: &[u8] =
        unsafe { core::slice::from_raw_parts(lint_config_bytes, lint_config_len.0) };

    let (flags, tokens, token_count) = match decode_token_config(config) {
        Some(decoded) => decoded,
        None => return AbiStatus::InvalidArg,
    };

    let entries = match unsafe { nam_file_entries(nam) } {
        Some(slice) => slice,
        None => return AbiStatus::InvalidArg,
    };

    let capacity = out_capacity.0;
    let mut written: usize = 0;
    let mut would_emit: usize = 0;

    for entry in entries {
        if entry.source.is_empty() {
            continue;
        }
        // SAFETY: non-empty source addresses host-owned bytes valid for the
        // call; its length is entry.source.len.
        let source: &[u8] = unsafe {
            core::slice::from_raw_parts(entry.source.data, entry.source.len.0)
        };
        for &(off, len) in &tokens[..token_count] {
            let token = &config[off..off + len];
            scan_one_token(source, token, flags, |match_off| {
                if would_emit < capacity {
                    let (line, column) = byte_offset_to_line_col(source, match_off);
                    // path aliases host-owned, call-scoped NAM memory; see
                    // the note on make_diagnostic.
                    let diag =
                        make_token_diagnostic(entry.path, rule_id, line, column, len as u32);
                    // SAFETY: written == would_emit < capacity -> in-bounds.
                    unsafe {
                        out_entries.add(written).write(diag);
                    }
                    written += 1;
                }
                would_emit += 1;
            });
        }
    }

    // SAFETY: out_len non-null.
    unsafe {
        *out_len = arvo::USize(would_emit);
    }
    if would_emit > capacity {
        AbiStatus::Internal
    } else {
        AbiStatus::Ok
    }
}

/// Decode the config blob into flags plus a fixed table of `(offset, len)`
/// token spans into `config`. Returns `None` on any malformation.
fn decode_token_config(
    config: &[u8],
) -> Option<(TokenScanFlags, [(usize, usize); MAX_TOKENS], usize)> {
    if config.len() < 8 {
        return None;
    }
    let flags = TokenScanFlags {
        word_boundary: config[0] != 0,
        strip_strings: config[1] != 0,
        strip_comments: config[2] != 0,
        strip_doc_comments: config[3] != 0,
    };
    let token_count = u32::from_le_bytes([config[4], config[5], config[6], config[7]]) as usize;
    if token_count > MAX_TOKENS {
        return None;
    }
    let mut tokens: [(usize, usize); MAX_TOKENS] = [(0, 0); MAX_TOKENS];
    let mut p = 8;
    for slot in tokens.iter_mut().take(token_count) {
        if p + 4 > config.len() {
            return None;
        }
        let len =
            u32::from_le_bytes([config[p], config[p + 1], config[p + 2], config[p + 3]]) as usize;
        p += 4;
        if len == 0 || p + len > config.len() {
            return None;
        }
        // The whole-span-skip strip is faithful only for tokens with no
        // comment / string delimiter byte (`/` or `"`); a token containing
        // one could match across a span boundary. Enforce the invariant
        // rather than assume it: reject such a config.
        let tok = &config[p..p + len];
        if tok.contains(&b'/') || tok.contains(&b'"') {
            return None;
        }
        *slot = (p, len);
        p += len;
    }
    Some((flags, tokens, token_count))
}

/// Scan `source` for `token`, skipping strings and comments per `flags`,
/// invoking `emit(byte_offset)` for each non-overlapping match. Mirrors the
/// in-process `scan_token` over a stripped view: a token can only match in
/// a live run between strip-spans, so whole spans are skipped (the
/// catalogue's tokens never contain comment or string delimiter bytes, so a
/// byte-equality match cannot straddle a span boundary).
fn scan_one_token(source: &[u8], token: &[u8], flags: TokenScanFlags, mut emit: impl FnMut(usize)) {
    let tlen = token.len();
    if tlen == 0 {
        return;
    }
    let mut i: usize = 0;
    while i + tlen <= source.len() {
        if let Some(end) = strip_span_at(source, i, flags) {
            i = end;
            continue;
        }
        if &source[i..i + tlen] == token && token_boundary_ok(source, i, tlen, flags.word_boundary)
        {
            emit(i);
            i += tlen;
        } else {
            i += 1;
        }
    }
}

/// If a stripped span (comment or string, per `flags`) starts at `i`, return
/// its end offset; otherwise `None`. Ports the span detection of
/// `mockspace_rs::strip`. A non-stripped comment / string (its flag off)
/// returns `None`, so its bytes stay live and matchable.
fn strip_span_at(source: &[u8], i: usize, flags: TokenScanFlags) -> Option<usize> {
    let len = source.len();
    let b = source[i];
    // line comment, possibly a doc comment.
    if b == b'/' && i + 1 < len && source[i + 1] == b'/' {
        let is_doc = i + 2 < len && (source[i + 2] == b'/' || source[i + 2] == b'!');
        let strip_this =
            (is_doc && flags.strip_doc_comments) || (!is_doc && flags.strip_comments);
        if strip_this {
            return Some(find_line_end(source, i));
        }
        return None;
    }
    // block comment, possibly a doc block.
    if b == b'/' && i + 1 < len && source[i + 1] == b'*' {
        let is_doc = i + 2 < len && (source[i + 2] == b'*' || source[i + 2] == b'!');
        let strip_this =
            (is_doc && flags.strip_doc_comments) || (!is_doc && flags.strip_comments);
        if strip_this {
            return Some(find_block_comment_end(source, i));
        }
        return None;
    }
    // raw string: r#"..."# or br#"..."#. The whole literal is skipped
    // (slightly wider than the in-process strip, which keeps the sigils
    // live; no catalogue token contains a raw-string sigil).
    if flags.strip_strings && (b == b'r' || (b == b'b' && i + 1 < len && source[i + 1] == b'r')) {
        if let Some(end) = match_raw_string_end(source, i) {
            return Some(end);
        }
    }
    // plain string literal.
    if flags.strip_strings && b == b'"' {
        return Some(find_string_end(source, i));
    }
    None
}

fn find_line_end(source: &[u8], from: usize) -> usize {
    let mut i = from;
    while i < source.len() && source[i] != b'\n' {
        i += 1;
    }
    if i < source.len() {
        i + 1
    } else {
        i
    }
}

fn find_block_comment_end(source: &[u8], from: usize) -> usize {
    let mut i = from + 2;
    let mut depth: usize = 1;
    while i + 1 < source.len() && depth > 0 {
        if source[i] == b'/' && source[i + 1] == b'*' {
            depth += 1;
            i += 2;
        } else if source[i] == b'*' && source[i + 1] == b'/' {
            depth -= 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    if depth == 0 { i } else { source.len() }
}

fn find_string_end(source: &[u8], from: usize) -> usize {
    let mut i = from + 1;
    while i < source.len() {
        match source[i] {
            b'\\' if i + 1 < source.len() => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    source.len()
}

/// Match a raw string starting at `from`; return its end offset, or `None`
/// if `from` does not open a raw string.
fn match_raw_string_end(source: &[u8], from: usize) -> Option<usize> {
    let len = source.len();
    let mut i = from;
    if source[i] == b'b' {
        i += 1;
    }
    if i >= len || source[i] != b'r' {
        return None;
    }
    i += 1;
    let mut hashes: usize = 0;
    while i < len && source[i] == b'#' {
        hashes += 1;
        i += 1;
    }
    if i >= len || source[i] != b'"' {
        return None;
    }
    let mut j = i + 1;
    while j < len {
        if source[j] == b'"' {
            let mut closing = 0;
            let mut k = j + 1;
            while k < len && source[k] == b'#' && closing < hashes {
                closing += 1;
                k += 1;
            }
            if closing == hashes {
                return Some(k);
            }
        }
        j += 1;
    }
    Some(len)
}

/// Word-boundary check mirroring the in-process `scan_token`: a boundary is
/// required only on the side whose adjacent token byte is itself a word
/// byte, and only when `word_boundary` is set.
fn token_boundary_ok(source: &[u8], start: usize, tlen: usize, word_boundary: bool) -> bool {
    if !word_boundary {
        return true;
    }
    let token = &source[start..start + tlen];
    let lhs_ok = if is_word_byte(token[0]) {
        start == 0 || !is_word_byte(source[start - 1])
    } else {
        true
    };
    let after = start + tlen;
    let rhs_ok = if is_word_byte(token[tlen - 1]) {
        after >= source.len() || !is_word_byte(source[after])
    } else {
        true
    };
    lhs_ok && rhs_ok
}

fn make_token_diagnostic(
    path: BytesRef,
    rule_id: &'static [u8],
    line: u32,
    column: u32,
    length: u32,
) -> Diagnostic {
    Diagnostic {
        plugin_id: bytes_ref_static(PLUGIN_ID),
        rule_id: bytes_ref_static(rule_id),
        severity: DiagnosticSeverity::Warn,
        message: bytes_ref_static(TOKEN_SCAN_MESSAGE),
        path,
        // tokens are single-line, so the span stays on one line; this
        // matches scan_token reporting length = token byte length.
        range: SourceRange {
            start: SourceLocation { line, column },
            end: SourceLocation { line, column: column + length },
        },
        suggestion: BytesRef::EMPTY,
        metadata_schema: ProviderId(0),
        metadata_ptr: core::ptr::null(),
        metadata_len: arvo::USize(0),
    }
}

// ---------------------------------------------------------------------------
// descriptor export
// ---------------------------------------------------------------------------

static NO_TODO_VTABLE: LintEvaluateVtable = LintEvaluateVtable {
    evaluate: no_todo_evaluate,
};

/// Provider-export marker for the no-todo lint. The `#[export_extension]`
/// macro reads `ID` and `VTABLE_PTR` into the descriptor's provider table.
pub struct NoTodoProvider;

impl ProviderExport for NoTodoProvider {
    const ID: ProviderId = PROVIDER_NO_TODO;
    const VTABLE_PTR: *const c_void =
        &NO_TODO_VTABLE as *const LintEvaluateVtable as *const c_void;
}

static FILE_SIZE_VTABLE: LintEvaluateVtable = LintEvaluateVtable {
    evaluate: file_size_evaluate,
};

/// Provider-export marker for the file-size lint.
pub struct FileSizeProvider;

impl ProviderExport for FileSizeProvider {
    const ID: ProviderId = PROVIDER_FILE_SIZE;
    const VTABLE_PTR: *const c_void =
        &FILE_SIZE_VTABLE as *const LintEvaluateVtable as *const c_void;
}

static NO_STD_VTABLE: LintEvaluateVtable = LintEvaluateVtable {
    evaluate: no_std_evaluate,
};

/// Provider-export marker for the no-std lint.
pub struct NoStdProvider;

impl ProviderExport for NoStdProvider {
    const ID: ProviderId = PROVIDER_NO_STD;
    const VTABLE_PTR: *const c_void =
        &NO_STD_VTABLE as *const LintEvaluateVtable as *const c_void;
}

#[export_extension(
    name = "mockspace-builtin-lints",
    version = "0.0.0",
    providers = [NoTodoProvider, FileSizeProvider, NoStdProvider],
)]
#[allow(dead_code)]
pub struct MockspaceBuiltinLints;

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::MaybeUninit;
    use std::vec::Vec;

    fn collect(source: &[u8]) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        for_each_match(source, |l, c| out.push((l, c)));
        out
    }

    #[test]
    fn single_match_reports_line_and_column() {
        // marker at byte 3 on line 1, column 3.
        let src = b"// \x54\x4f\x44\x4f: fix\nfn x() {}";
        assert_eq!(collect(src), std::vec![(1, 3)]);
    }

    #[test]
    fn word_boundary_excludes_embedded_marker() {
        // trailing word byte (S) and leading word byte (X / _) both fail
        // the boundary, so none of these three match.
        let src = b"\x54\x4f\x44\x4fS X\x54\x4f\x44\x4f _\x54\x4f\x44\x4f";
        assert!(collect(src).is_empty());
    }

    #[test]
    fn case_sensitive_lowercase_does_not_match() {
        let src = b"todo lower nope";
        assert!(collect(src).is_empty());
    }

    #[test]
    fn multiple_matches_track_line_and_column() {
        // line 1 col 2, line 2 col 2.
        let src = b"x \x54\x4f\x44\x4f y\nz \x54\x4f\x44\x4f w";
        assert_eq!(collect(src), std::vec![(1, 2), (2, 2)]);
    }

    fn entry(path: &[u8], source: &[u8]) -> viola_plugin_abi::NamFileEntry {
        viola_plugin_abi::NamFileEntry {
            path: bytes_ref_static_runtime(path),
            language: arvo::USize(0),
            source: bytes_ref_static_runtime(source),
            nodes: BytesRef::EMPTY,
        }
    }

    fn bytes_ref_static_runtime(b: &[u8]) -> BytesRef {
        BytesRef { data: b.as_ptr(), len: arvo::USize(b.len()) }
    }

    fn payload(entries: &[viola_plugin_abi::NamFileEntry]) -> NamPayload {
        NamPayload {
            version: viola_plugin_abi::NamVersion::V1_0_0,
            data: entries.as_ptr() as *const c_void,
            len: arvo::USize(
                core::mem::size_of::<viola_plugin_abi::NamFileEntry>()
                    * entries.len(),
            ),
        }
    }

    #[test]
    fn evaluate_writes_diagnostics_into_host_buffer() {
        let src = b"x \x54\x4f\x44\x4f y\nz \x54\x4f\x44\x4f w";
        let entries = [entry(b"a.rs", src)];
        let nam = payload(&entries);

        let mut buf: [MaybeUninit<Diagnostic>; 8] =
            [const { MaybeUninit::uninit() }; 8];
        let mut out_len = arvo::USize(0);
        // SAFETY: nam is a valid v1.0.0 payload; buf has 8 writable slots;
        // out_len is a valid pointer.
        let status = unsafe {
            no_todo_evaluate(
                core::ptr::null_mut(),
                &nam,
                core::ptr::null(),
                arvo::USize(0),
                buf.as_mut_ptr() as *mut Diagnostic,
                arvo::USize(8),
                &mut out_len,
            )
        };
        assert!(matches!(status, AbiStatus::Ok));
        assert_eq!(out_len.0, 2);
        // SAFETY: out_len reports 2 written entries; slots 0 and 1 are init.
        let d0 = unsafe { buf[0].assume_init() };
        let d1 = unsafe { buf[1].assume_init() };
        assert_eq!(d0.range.start.line, 1);
        assert_eq!(d0.range.start.column, 2);
        assert_eq!(d1.range.start.line, 2);
        assert_eq!(d1.range.start.column, 2);
        assert_eq!(d0.severity, DiagnosticSeverity::Warn);
    }

    #[test]
    fn evaluate_reports_overflow_with_would_have_count() {
        let src = b"x \x54\x4f\x44\x4f y\nz \x54\x4f\x44\x4f w";
        let entries = [entry(b"a.rs", src)];
        let nam = payload(&entries);

        let mut buf: [MaybeUninit<Diagnostic>; 1] =
            [const { MaybeUninit::uninit() }; 1];
        let mut out_len = arvo::USize(0);
        // SAFETY: capacity 1 is honoured; only slot 0 is written.
        let status = unsafe {
            no_todo_evaluate(
                core::ptr::null_mut(),
                &nam,
                core::ptr::null(),
                arvo::USize(0),
                buf.as_mut_ptr() as *mut Diagnostic,
                arvo::USize(1),
                &mut out_len,
            )
        };
        assert!(matches!(status, AbiStatus::Internal));
        // would-have-emitted count is the full match total, not the
        // truncated written count.
        assert_eq!(out_len.0, 2);
    }

    #[test]
    fn evaluate_skips_empty_source_and_walks_multiple_entries() {
        // first entry has empty source (skipped); second has one match.
        // exercises the `entry.source.is_empty()` skip and the multi-entry
        // loop, and that the emitted path tracks the matching entry.
        let entries = [
            entry(b"empty.rs", b""),
            entry(b"a.rs", b"x \x54\x4f\x44\x4f y"),
        ];
        let nam = payload(&entries);
        let mut buf: [MaybeUninit<Diagnostic>; 4] =
            [const { MaybeUninit::uninit() }; 4];
        let mut out_len = arvo::USize(0);
        // SAFETY: nam is a valid v1.0.0 payload; buf has 4 writable slots.
        let status = unsafe {
            no_todo_evaluate(
                core::ptr::null_mut(),
                &nam,
                core::ptr::null(),
                arvo::USize(0),
                buf.as_mut_ptr() as *mut Diagnostic,
                arvo::USize(4),
                &mut out_len,
            )
        };
        assert!(matches!(status, AbiStatus::Ok));
        assert_eq!(out_len.0, 1);
        // SAFETY: one entry written; slot 0 is initialised.
        let d0 = unsafe { buf[0].assume_init() };
        // the diagnostic path aliases the matching entry's path bytes.
        // SAFETY: d0.path points at the `b"a.rs"` slice live for this test.
        let path = unsafe {
            core::slice::from_raw_parts(d0.path.data, d0.path.len.0)
        };
        assert_eq!(path, b"a.rs");
    }

    #[test]
    fn evaluate_rejects_null_out_len() {
        let entries = [entry(b"a.rs", b"x \x54\x4f\x44\x4f y")];
        let nam = payload(&entries);
        let mut buf: [MaybeUninit<Diagnostic>; 4] =
            [const { MaybeUninit::uninit() }; 4];
        // SAFETY: out_len null is the case under test; the fn returns before
        // any write.
        let status = unsafe {
            no_todo_evaluate(
                core::ptr::null_mut(),
                &nam,
                core::ptr::null(),
                arvo::USize(0),
                buf.as_mut_ptr() as *mut Diagnostic,
                arvo::USize(4),
                core::ptr::null_mut(),
            )
        };
        assert!(matches!(status, AbiStatus::InvalidArg));
    }

    // -- file-size --

    #[test]
    fn line_metric_matches_str_lines() {
        // parity with str::lines() for the plain line-count metric.
        for s in [
            "",
            "a",
            "a\n",
            "a\nb",
            "a\nb\n",
            "a\n\nb\n",
            "\n",
            "a\r\nb\r\n",
        ] {
            assert_eq!(
                line_metric(s.as_bytes(), METRIC_LINE_COUNT),
                s.lines().count(),
                "line count mismatch for {s:?}",
            );
        }
    }

    #[test]
    fn line_metric_non_blank_and_non_comment() {
        let src = b"code\n   \n// comment\n  // indented comment\nmore code\n";
        // lines: "code", "   "(blank), "// comment", "  // indented comment", "more code"
        assert_eq!(line_metric(src, METRIC_LINE_COUNT), 5);
        // non-blank drops the whitespace-only line.
        assert_eq!(line_metric(src, METRIC_NON_BLANK_LINE_COUNT), 4);
        // non-blank-non-comment also drops the two comment lines.
        assert_eq!(line_metric(src, METRIC_NON_BLANK_NON_COMMENT_LINE_COUNT), 2);
    }

    fn run_file_size(
        entries: &[viola_plugin_abi::NamFileEntry],
        cfg: Option<FileSizeConfig>,
        cap: usize,
    ) -> (AbiStatus, usize) {
        let nam = payload(entries);
        let mut buf: [MaybeUninit<Diagnostic>; 8] =
            [const { MaybeUninit::uninit() }; 8];
        let mut out_len = arvo::USize(0);
        let (cptr, clen): (*const u8, arvo::USize) = match &cfg {
            Some(c) => (
                c as *const FileSizeConfig as *const u8,
                arvo::USize(core::mem::size_of::<FileSizeConfig>()),
            ),
            None => (core::ptr::null(), arvo::USize(0)),
        };
        // SAFETY: nam valid; buf has 8 slots (cap <= 8); out_len valid; cfg
        // pointer (when Some) addresses a live FileSizeConfig for the call.
        let status = unsafe {
            file_size_evaluate(
                core::ptr::null_mut(),
                &nam,
                cptr,
                clen,
                buf.as_mut_ptr() as *mut Diagnostic,
                arvo::USize(cap),
                &mut out_len,
            )
        };
        (status, out_len.0)
    }

    #[test]
    fn file_size_threshold_inclusive_and_exclusive() {
        // 3-line file.
        let entries = [entry(b"a.rs", b"one\ntwo\nthree")];
        // exclusive, threshold 3: count 3 is NOT > 3 -> no fire.
        let cfg = FileSizeConfig { metric: METRIC_LINE_COUNT, inclusive: 0, threshold: arvo::USize(3) };
        let (s, n) = run_file_size(&entries, Some(cfg), 8);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 0);
        // inclusive, threshold 3: count 3 IS >= 3 -> fire.
        let cfg = FileSizeConfig { metric: METRIC_LINE_COUNT, inclusive: 1, threshold: arvo::USize(3) };
        let (s, n) = run_file_size(&entries, Some(cfg), 8);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 1);
        // exclusive, threshold 2: count 3 > 2 -> fire.
        let cfg = FileSizeConfig { metric: METRIC_LINE_COUNT, inclusive: 0, threshold: arvo::USize(2) };
        let (s, n) = run_file_size(&entries, Some(cfg), 8);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 1);
    }

    #[test]
    fn file_size_empty_config_is_noop() {
        let entries = [entry(b"a.rs", b"one\ntwo\nthree\nfour\nfive")];
        let (s, n) = run_file_size(&entries, None, 8);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 0);
    }

    #[test]
    fn file_size_rejects_unknown_metric() {
        let entries = [entry(b"a.rs", b"x\ny")];
        // metric 3 = PubItemCount (AST) -> out of range for this provider.
        let cfg = FileSizeConfig { metric: 3, inclusive: 1, threshold: arvo::USize(1) };
        let (s, _) = run_file_size(&entries, Some(cfg), 8);
        assert!(matches!(s, AbiStatus::InvalidArg));
    }

    #[test]
    fn file_size_rejects_wrong_config_len() {
        let entries = [entry(b"a.rs", b"x\ny")];
        let nam = payload(&entries);
        let mut buf: [MaybeUninit<Diagnostic>; 4] =
            [const { MaybeUninit::uninit() }; 4];
        let mut out_len = arvo::USize(0);
        let stray: [u8; 3] = [1, 2, 3];
        // SAFETY: non-zero, non-FileSizeConfig-sized config -> InvalidArg
        // before any deref of the bytes as a config.
        let status = unsafe {
            file_size_evaluate(
                core::ptr::null_mut(),
                &nam,
                stray.as_ptr(),
                arvo::USize(3),
                buf.as_mut_ptr() as *mut Diagnostic,
                arvo::USize(4),
                &mut out_len,
            )
        };
        assert!(matches!(status, AbiStatus::InvalidArg));
    }

    #[test]
    fn file_size_config_layout_is_pinned() {
        use core::mem::{offset_of, size_of};
        // FileSizeConfig is the #[repr(C)] wire contract between the host
        // encoder and this decoder. Pin the field offsets: the runtime
        // size_of guard catches a length mismatch, but a reorder or padding
        // drift that preserves size (e.g. swapping the two u32 fields) would
        // otherwise read garbage silently.
        assert_eq!(offset_of!(FileSizeConfig, metric), 0);
        assert_eq!(offset_of!(FileSizeConfig, inclusive), 4);
        assert_eq!(offset_of!(FileSizeConfig, threshold), 8);
        assert_eq!(size_of::<FileSizeConfig>(), 8 + size_of::<arvo::USize>());
    }

    #[test]
    fn file_size_counts_per_file_across_entries() {
        // first file 2 lines (under), second 4 lines (over threshold 3 excl).
        let entries = [
            entry(b"small.rs", b"a\nb"),
            entry(b"big.rs", b"a\nb\nc\nd"),
        ];
        let cfg = FileSizeConfig { metric: METRIC_LINE_COUNT, inclusive: 0, threshold: arvo::USize(3) };
        let (s, n) = run_file_size(&entries, Some(cfg), 8);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 1);
    }

    // -- token-scan (no-std) --

    fn token_blob(
        word_boundary: bool,
        strip: (bool, bool, bool),
        tokens: &[&[u8]],
    ) -> Vec<u8> {
        let mut b = Vec::new();
        b.push(word_boundary as u8);
        b.push(strip.0 as u8); // strings
        b.push(strip.1 as u8); // comments
        b.push(strip.2 as u8); // doc comments
        b.extend_from_slice(&(tokens.len() as u32).to_le_bytes());
        for t in tokens {
            b.extend_from_slice(&(t.len() as u32).to_le_bytes());
            b.extend_from_slice(t);
        }
        b
    }

    fn run_token_scan(
        entries: &[viola_plugin_abi::NamFileEntry],
        blob: &[u8],
        cap: usize,
    ) -> (AbiStatus, usize, Vec<Diagnostic>) {
        let nam = payload(entries);
        let mut buf: [MaybeUninit<Diagnostic>; 16] =
            [const { MaybeUninit::uninit() }; 16];
        let mut out_len = arvo::USize(0);
        // SAFETY: nam valid; buf has 16 slots (cap <= 16); out_len valid;
        // blob addresses live config bytes for the call.
        let status = unsafe {
            no_std_evaluate(
                core::ptr::null_mut(),
                &nam,
                blob.as_ptr(),
                arvo::USize(blob.len()),
                buf.as_mut_ptr() as *mut Diagnostic,
                arvo::USize(cap),
                &mut out_len,
            )
        };
        let written = out_len.0.min(cap);
        let mut diags = Vec::new();
        for k in 0..written {
            // SAFETY: slots 0..written were initialised by the evaluator.
            diags.push(unsafe { buf[k].assume_init() });
        }
        (status, out_len.0, diags)
    }

    #[test]
    fn token_scan_matches_live_code_with_line_col() {
        let entries = [entry(b"a.rs", b"  use std::io;\n")];
        let blob = token_blob(false, (true, true, true), &[b"use std::"]);
        let (s, n, diags) = run_token_scan(&entries, &blob, 16);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 1);
        assert_eq!(diags[0].range.start.line, 1);
        assert_eq!(diags[0].range.start.column, 2);
        // span covers the matched token: "use std::" is 9 bytes.
        assert_eq!(diags[0].range.end.column, 2 + 9);
        // SAFETY: rule_id points at the static b"no-std".
        let rid = unsafe {
            core::slice::from_raw_parts(diags[0].rule_id.data, diags[0].rule_id.len.0)
        };
        assert_eq!(rid, b"no-std");
    }

    #[test]
    fn token_scan_skips_strings_and_comments() {
        // token appears in a string, a line comment, and a block comment
        // (all stripped) and once in live code; only the live one fires.
        let src = b"let s = \"use std::io\";\n// use std::fmt\n/* use std::x */\nuse std::real;\n";
        let entries = [entry(b"a.rs", src)];
        let blob = token_blob(false, (true, true, true), &[b"use std::"]);
        let (s, n, _) = run_token_scan(&entries, &blob, 16);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 1);
    }

    #[test]
    fn token_scan_skips_raw_string() {
        let src = b"let s = r#\"use std::io\"#;\nuse std::real;\n";
        let entries = [entry(b"a.rs", src)];
        let blob = token_blob(false, (true, true, true), &[b"use std::"]);
        let (_, n, _) = run_token_scan(&entries, &blob, 16);
        assert_eq!(n, 1);
    }

    #[test]
    fn token_scan_strip_flag_off_matches_in_comment() {
        // strip_comments off: the token in the comment now fires too.
        let src = b"// use std::fmt\nuse std::real;\n";
        let entries = [entry(b"a.rs", src)];
        let blob = token_blob(false, (true, false, false), &[b"use std::"]);
        let (_, n, _) = run_token_scan(&entries, &blob, 16);
        assert_eq!(n, 2);
    }

    #[test]
    fn token_scan_multiple_tokens() {
        let src = b"use std::io;\nextern crate std;\n";
        let entries = [entry(b"a.rs", src)];
        let blob = token_blob(false, (true, true, true), &[b"use std::", b"extern crate std"]);
        let (_, n, _) = run_token_scan(&entries, &blob, 16);
        assert_eq!(n, 2);
    }

    #[test]
    fn token_scan_word_boundary() {
        // word_boundary on: "std" matches as a whole word but not inside
        // "stdio".
        let src = b"a std b\nstdio\n";
        let entries = [entry(b"a.rs", src)];
        let blob = token_blob(true, (true, true, true), &[b"std"]);
        let (_, n, _) = run_token_scan(&entries, &blob, 16);
        assert_eq!(n, 1);
    }

    #[test]
    fn token_scan_empty_config_is_noop() {
        let entries = [entry(b"a.rs", b"use std::io;\n")];
        let (s, n, _) = run_token_scan(&entries, &[], 16);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 0);
    }

    #[test]
    fn token_scan_rejects_malformed_config() {
        let entries = [entry(b"a.rs", b"use std::io;\n")];
        // short header (< 8 bytes).
        let (s1, _, _) = run_token_scan(&entries, &[1, 1, 1], 16);
        assert!(matches!(s1, AbiStatus::InvalidArg));
        // token_count beyond MAX_TOKENS.
        let mut huge = std::vec![0u8, 1, 1, 1];
        huge.extend_from_slice(&(u32::MAX).to_le_bytes());
        let (s2, _, _) = run_token_scan(&entries, &huge, 16);
        assert!(matches!(s2, AbiStatus::InvalidArg));
        // token length runs past the buffer.
        let mut overrun = std::vec![0u8, 1, 1, 1];
        overrun.extend_from_slice(&(1u32).to_le_bytes()); // count 1
        overrun.extend_from_slice(&(99u32).to_le_bytes()); // len 99, no bytes
        let (s3, _, _) = run_token_scan(&entries, &overrun, 16);
        assert!(matches!(s3, AbiStatus::InvalidArg));
    }

    #[test]
    fn token_scan_rejects_delimiter_token() {
        // a token containing a comment/string delimiter would break the
        // whole-span-skip invariant; the decoder rejects it.
        let entries = [entry(b"a.rs", b"a/b\n")];
        let slash = token_blob(false, (true, true, true), &[b"a/b"]);
        let (s1, _, _) = run_token_scan(&entries, &slash, 16);
        assert!(matches!(s1, AbiStatus::InvalidArg));
        let quote = token_blob(false, (true, true, true), &[b"a\"b"]);
        let (s2, _, _) = run_token_scan(&entries, &quote, 16);
        assert!(matches!(s2, AbiStatus::InvalidArg));
    }

    #[test]
    fn token_scan_overflow_reports_would_have_count() {
        let src = b"use std::a;\nuse std::b;\nuse std::c;\n";
        let entries = [entry(b"a.rs", src)];
        let blob = token_blob(false, (true, true, true), &[b"use std::"]);
        // capacity 1, three matches.
        let (s, n, diags) = run_token_scan(&entries, &blob, 1);
        assert!(matches!(s, AbiStatus::Internal));
        assert_eq!(n, 3);
        assert_eq!(diags.len(), 1);
    }
}
