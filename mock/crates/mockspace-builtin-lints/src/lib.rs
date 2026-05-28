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

#[export_extension(
    name = "mockspace-builtin-lints",
    version = "0.0.0",
    providers = [NoTodoProvider, FileSizeProvider],
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
}
