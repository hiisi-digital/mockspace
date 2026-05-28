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
    NamFileEntry, NamNode, NamPayload, SourceLocation, SourceRange, nam_file_entries,
    nam_file_nodes, node_kind,
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

/// Metric discriminants matching `mockspace_rs::builtins::file_metric::Metric`
/// in order. The line-count metrics (0..=2) scan source bytes; the item-count
/// metrics (3..=5) walk the NAM v1.1.0 `NamNode` array. An out-of-range
/// discriminant is rejected.
pub const METRIC_LINE_COUNT: u32 = 0;
pub const METRIC_NON_BLANK_LINE_COUNT: u32 = 1;
pub const METRIC_NON_BLANK_NON_COMMENT_LINE_COUNT: u32 = 2;
pub const METRIC_PUB_ITEM_COUNT: u32 = 3;
pub const METRIC_PRIVATE_ITEM_COUNT: u32 = 4;
pub const METRIC_TOTAL_ITEM_COUNT: u32 = 5;

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

/// Evaluator for the file-size lint. Counts lines (metrics 0..=2, over
/// source bytes) or top-level items (metrics 3..=5, over the NAM v1.1.0
/// node array) per the configured metric, and emits one diagnostic per file
/// whose count crosses the threshold.
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
    if config.metric > METRIC_TOTAL_ITEM_COUNT {
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
        let count = if config.metric <= METRIC_NON_BLANK_NON_COMMENT_LINE_COUNT {
            if entry.source.is_empty() {
                0
            } else {
                // SAFETY: a non-empty NamFileEntry.source addresses host-owned
                // bytes valid for the call; its byte length is entry.source.len.
                let source: &[u8] = unsafe {
                    core::slice::from_raw_parts(entry.source.data, entry.source.len.0)
                };
                line_metric(source, config.metric)
            }
        } else {
            // item-count metrics walk the NAM node array.
            item_metric(entry, config.metric)
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

/// Count top-level items per the item-count metric, walking the NAM v1.1.0
/// `NamNode` array. Returns 0 when the entry carries no serialised tree (a
/// v1.0.0 producer, or before the rust-native runner serialises nodes).
///
/// Top-level items are the direct children of the SOURCE_FILE root (a node's
/// `parent` index equals the root's index). The flat array has no sibling
/// pointer, so children are found by scanning for a matching `parent`. `pub`
/// visibility is read from the source byte span of a child VISIBILITY_MODIFIER
/// node, exactly `pub` (matching syn `Visibility::Public`; `pub(crate)` and
/// friends are not public). PrivateItemCount is total minus public, matching
/// `file_metric`'s `!is_pub_item` count.
///
/// The item-kind set ([`is_item_kind`]) APPROXIMATES syn `File::items`: it
/// counts only the item kinds present in the canonical `node_kind` table.
/// Item forms with no table kind yet (`extern crate`, trait alias) are not
/// counted until the table gains them (a viola-side append; see round
/// 202605282400 DOC CL D4). For the realistic max-item-count workloads this
/// metric serves, that gap is immaterial; exact parity arrives with the
/// table additions.
fn item_metric(entry: &NamFileEntry, metric: u32) -> usize {
    let nodes = match nam_file_nodes(entry) {
        Some(n) if !n.is_empty() => n,
        _ => return 0,
    };
    let source: &[u8] = if entry.source.is_empty() {
        &[]
    } else {
        // SAFETY: non-empty source addresses host-owned bytes valid for the call.
        unsafe { core::slice::from_raw_parts(entry.source.data, entry.source.len.0) }
    };
    let root = match nodes.iter().position(|nd| nd.kind.0 == node_kind::SOURCE_FILE.0) {
        Some(i) => i,
        None => return 0,
    };

    let mut total: usize = 0;
    let mut public: usize = 0;
    for (i, nd) in nodes.iter().enumerate() {
        if nd.parent.0 != root || !is_item_kind(nd.kind.0) {
            continue;
        }
        total += 1;
        if is_pub_item_kind(nd.kind.0) && node_is_public(nodes, i, source) {
            public += 1;
        }
    }

    match metric {
        METRIC_PUB_ITEM_COUNT => public,
        METRIC_PRIVATE_ITEM_COUNT => total - public,
        // METRIC_TOTAL_ITEM_COUNT.
        _ => total,
    }
}

/// Top-level item node kinds (the breadth of syn `File::items`).
fn is_item_kind(kind: usize) -> bool {
    kind == node_kind::FUNCTION_ITEM.0
        || kind == node_kind::STRUCT_ITEM.0
        || kind == node_kind::ENUM_ITEM.0
        || kind == node_kind::UNION_ITEM.0
        || kind == node_kind::TRAIT_ITEM.0
        || kind == node_kind::IMPL_ITEM.0
        || kind == node_kind::MOD_ITEM.0
        || kind == node_kind::TYPE_ITEM.0
        || kind == node_kind::CONST_ITEM.0
        || kind == node_kind::STATIC_ITEM.0
        || kind == node_kind::USE_DECLARATION.0
        || kind == node_kind::MACRO_DEFINITION.0
        || kind == node_kind::MACRO_INVOCATION.0
        || kind == node_kind::FOREIGN_MOD_ITEM.0
}

/// Item kinds eligible for the public count (matches `file_metric::is_pub_item`:
/// fn, struct, enum, trait, type, const, static, mod).
fn is_pub_item_kind(kind: usize) -> bool {
    kind == node_kind::FUNCTION_ITEM.0
        || kind == node_kind::STRUCT_ITEM.0
        || kind == node_kind::ENUM_ITEM.0
        || kind == node_kind::TRAIT_ITEM.0
        || kind == node_kind::TYPE_ITEM.0
        || kind == node_kind::CONST_ITEM.0
        || kind == node_kind::STATIC_ITEM.0
        || kind == node_kind::MOD_ITEM.0
}

/// True when `item_idx` has a child VISIBILITY_MODIFIER node whose source text
/// is exactly `pub`. Children are found by parent-index scan.
fn node_is_public(nodes: &[NamNode], item_idx: usize, source: &[u8]) -> bool {
    for nd in nodes {
        if nd.parent.0 == item_idx && nd.kind.0 == node_kind::VISIBILITY_MODIFIER.0 {
            let start = nd.start_byte.0;
            let end = nd.end_byte.0;
            if start <= end && end <= source.len() && &source[start..end] == b"pub" {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// ast-type-position
// ---------------------------------------------------------------------------

/// Provider id for the ast-type-position lint (the `ast-type-position`
/// primitive). Backs the bare-primitive family of catalogue presets
/// (no-public-raw-field, no-bare-numeric, no-bare-option, no-bare-result,
/// no-bare-string, no-vec-in-trait-sig, strategy-marker-required,
/// trait-first-signatures): each ships a forbidden-type list plus a
/// position selection, and this evaluator fires once per forbidden type
/// name found at a selected position when the enclosing item's visibility
/// matches.
pub const PROVIDER_AST_TYPE_POSITION: ProviderId =
    ProviderId::from_name("mockspace-builtin.lint.ast-type-position.v2");

const AST_TYPE_POSITION_RULE_ID: &[u8] = b"ast-type-position";
const AST_TYPE_POSITION_MESSAGE: &[u8] = b"forbidden type in this position";

/// Position-selection bits in the config blob's `positions` byte. They
/// mirror `mockspace_rs::config_types::TypePosition`, narrowed to the
/// position kinds the v1.2.0 `node_kind` table can express via a container
/// node plus its descendant type leaves.
///
/// StructField and EnumVariantField both bottom out at FIELD_DECLARATION
/// in the tree (a struct-style enum-variant field is a FIELD_DECLARATION
/// under an ENUM_VARIANT), so they share the container kind but are
/// distinguished by whether an ENUM_VARIANT sits on the parent chain.
pub const POSITION_STRUCT_FIELD: u8 = 1 << 0;
pub const POSITION_ENUM_VARIANT_FIELD: u8 = 1 << 1;
pub const POSITION_FN_PARAM: u8 = 1 << 2;

// SCOPING NOTE: FnReturn and TypeAliasBody (two further TypePosition
// variants the in-process lint inspects) are NOT expressible against the
// v1.2.0 node_kind table. A return type has no dedicated position-container
// kind (it is a bare TYPE_IDENTIFIER / PRIMITIVE_TYPE child of a function
// signature with no return-position marker), and a type-alias body
// likewise has no body-position kind. Both await a return-type-position
// and a type-alias-body-position kind in a later NAM bump; this slice
// scopes to the three positions kinds 22..=24 can express. The bare
// TYPE_ITEM walk for TypeAliasBody and the FnReturn walk are intentionally
// absent, not forgotten.

/// Visibility filter, mirroring `config_types::Visibility`. `0` is Any
/// (no gate); non-zero is Public (the enclosing item must be `pub`).
const VISIBILITY_ANY: u8 = 0;

/// Decoded ast-type-position config: the visibility gate, the selected
/// position bitset, and a fixed table of `(offset, len)` spans of the
/// forbidden type names into the config blob.
#[derive(Copy, Clone)]
struct AstTypePositionFlags {
    visibility: u8,
    positions: u8,
}

/// Upper bound on the forbidden-type count a single config may carry. The
/// largest bare-primitive preset lists well under this; 32 leaves headroom
/// without an allocation (matching the token-scan bound).
const MAX_FORBIDDEN: usize = 32;

/// Evaluator for the ast-type-position lint. Walks the NAM v1.2.0 node
/// array; for each position-container node selected by the config (a
/// FIELD_DECLARATION, PARAMETER, or ENUM_VARIANT-hosted FIELD_DECLARATION),
/// collects its descendant TYPE_IDENTIFIER / PRIMITIVE_TYPE leaves and
/// compares each leaf's source text against the forbidden-type list. On a
/// match, when the enclosing item passes the visibility gate, emits one
/// diagnostic spanned to the leaf's byte range.
///
/// Config handling matches the other evaluators: empty config
/// (`config_len == 0`) is a no-op; a null config pointer alongside a
/// non-zero length, or a malformed blob, returns [`AbiStatus::InvalidArg`].
/// An entry with no serialised node tree (a pre-v1.1.0 producer) is
/// skipped. The overflow + null-arg contract is identical to
/// [`no_todo_evaluate`].
///
/// SAFETY: the host upholds the `LintEvaluateVtable` contract.
unsafe extern "C" fn ast_type_position_evaluate(
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
        // SAFETY: out_len non-null (checked above).
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
    let (flags, forbidden, forbidden_count) = match decode_ast_type_position_config(config) {
        Some(decoded) => decoded,
        None => return AbiStatus::InvalidArg,
    };

    // SAFETY: host upholds nam validity; None for a non-v1.x carrier.
    let entries = match unsafe { nam_file_entries(nam) } {
        Some(slice) => slice,
        None => return AbiStatus::InvalidArg,
    };

    let capacity = out_capacity.0;
    let mut written: usize = 0;
    let mut would_emit: usize = 0;

    for entry in entries {
        let nodes = match nam_file_nodes(entry) {
            Some(n) if !n.is_empty() => n,
            _ => continue,
        };
        let source: &[u8] = if entry.source.is_empty() {
            &[]
        } else {
            // SAFETY: non-empty source addresses host-owned bytes valid for
            // the call; its length is entry.source.len.
            unsafe { core::slice::from_raw_parts(entry.source.data, entry.source.len.0) }
        };

        // Each type leaf belongs to its nearest enclosing position
        // container; classifying by that nearest container avoids
        // double-counting when containers nest (a FIELD_DECLARATION inside
        // an ENUM_VARIANT). The leaf is emitted once, attributed to the
        // closest container, and only when that container's position bit is
        // selected.
        for (leaf_idx, leaf) in nodes.iter().enumerate() {
            if leaf.kind.0 != node_kind::TYPE_IDENTIFIER.0
                && leaf.kind.0 != node_kind::PRIMITIVE_TYPE.0
            {
                continue;
            }
            let Some(container) = nearest_container(nodes, leaf_idx) else {
                continue;
            };
            let Some(position) =
                position_for_container(nodes, container, nodes[container].kind.0)
            else {
                continue;
            };
            if flags.positions & position == 0 {
                continue;
            }
            if flags.visibility != VISIBILITY_ANY
                && !enclosing_item_is_public(nodes, container, source)
            {
                continue;
            }
            let start = leaf.start_byte.0;
            let end = leaf.end_byte.0;
            if !(start <= end && end <= source.len()) {
                continue;
            }
            let leaf_text = &source[start..end];
            let mut matched = false;
            for &(off, len) in &forbidden[..forbidden_count] {
                if leaf_text == &config[off..off + len] {
                    matched = true;
                    break;
                }
            }
            if !matched {
                continue;
            }
            if would_emit < capacity {
                let (line, column) = byte_offset_to_line_col(source, start);
                // path aliases host-owned, call-scoped NAM memory; see the
                // note on make_diagnostic.
                let diag = make_type_position_diagnostic(
                    entry.path,
                    line,
                    column,
                    (end - start) as u32,
                );
                // SAFETY: written == would_emit < capacity -> in-bounds.
                unsafe {
                    out_entries.add(written).write(diag);
                }
                written += 1;
            }
            would_emit += 1;
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

/// Decode the ast-type-position config blob.
///
/// Layout (little-endian): `[visibility: u8][positions: u8]
/// [forbidden_count: u32]` then `forbidden_count` entries of
/// `[len: u32][bytes...]`. Returns `None` on any malformation (short
/// header, `forbidden_count > MAX_FORBIDDEN`, a type-name length running
/// past the buffer, or a zero-length type name).
fn decode_ast_type_position_config(
    config: &[u8],
) -> Option<(AstTypePositionFlags, [(usize, usize); MAX_FORBIDDEN], usize)> {
    if config.len() < 6 {
        return None;
    }
    let flags = AstTypePositionFlags {
        visibility: config[0],
        positions: config[1],
    };
    let forbidden_count =
        u32::from_le_bytes([config[2], config[3], config[4], config[5]]) as usize;
    if forbidden_count > MAX_FORBIDDEN {
        return None;
    }
    let mut forbidden: [(usize, usize); MAX_FORBIDDEN] = [(0, 0); MAX_FORBIDDEN];
    let mut p = 6;
    for slot in forbidden.iter_mut().take(forbidden_count) {
        if p + 4 > config.len() {
            return None;
        }
        let len =
            u32::from_le_bytes([config[p], config[p + 1], config[p + 2], config[p + 3]]) as usize;
        p += 4;
        if len == 0 || p + len > config.len() {
            return None;
        }
        *slot = (p, len);
        p += len;
    }
    Some((flags, forbidden, forbidden_count))
}

/// Classify a node as a selectable position container. Returns the position
/// bit it satisfies, or `None` when the node is not a position container.
///
/// A PARAMETER is FnParam. A FIELD_DECLARATION is EnumVariantField when an
/// ENUM_VARIANT sits on its parent chain (a struct-style enum variant
/// field), otherwise StructField. ENUM_VARIANT itself is not a leaf
/// container; its struct-style fields are FIELD_DECLARATION children, and
/// tuple-style variant types are TYPE_IDENTIFIER / PRIMITIVE_TYPE children
/// of the ENUM_VARIANT, so the ENUM_VARIANT node is itself treated as an
/// EnumVariantField container to catch the tuple-style case.
fn position_for_container(nodes: &[NamNode], idx: usize, kind: usize) -> Option<u8> {
    if kind == node_kind::PARAMETER.0 {
        return Some(POSITION_FN_PARAM);
    }
    if kind == node_kind::ENUM_VARIANT.0 {
        return Some(POSITION_ENUM_VARIANT_FIELD);
    }
    if kind == node_kind::FIELD_DECLARATION.0 {
        if ancestor_has_kind(nodes, idx, node_kind::ENUM_VARIANT.0) {
            return Some(POSITION_ENUM_VARIANT_FIELD);
        }
        return Some(POSITION_STRUCT_FIELD);
    }
    None
}

/// True when any ancestor of `idx` (its parent, grandparent, ...) has the
/// given kind. The flat array encodes the parent index; a node whose parent
/// is itself, or whose parent index is out of range (the slice-length root
/// sentinel), terminates the walk.
fn ancestor_has_kind(nodes: &[NamNode], idx: usize, kind: usize) -> bool {
    let mut cur = idx;
    let mut guard = 0;
    while guard < nodes.len() {
        let parent = nodes[cur].parent.0;
        if parent >= nodes.len() || parent == cur {
            return false;
        }
        if nodes[parent].kind.0 == kind {
            return true;
        }
        cur = parent;
        guard += 1;
    }
    false
}

/// Find the nearest position-container ancestor of `idx` (its closest
/// FIELD_DECLARATION / PARAMETER / ENUM_VARIANT ancestor), walking the
/// parent chain up. Returns `None` when no container ancestor exists (a
/// type leaf in a non-position context, e.g. a let binding's type). The
/// "nearest" rule is what de-duplicates nested containers: a field type
/// leaf inside an enum variant resolves to the FIELD_DECLARATION, not the
/// ENUM_VARIANT, so it is counted once.
fn nearest_container(nodes: &[NamNode], idx: usize) -> Option<usize> {
    let mut cur = idx;
    let mut guard = 0;
    while guard < nodes.len() {
        let parent = nodes[cur].parent.0;
        if parent >= nodes.len() || parent == cur {
            return None;
        }
        let k = nodes[parent].kind.0;
        if k == node_kind::FIELD_DECLARATION.0
            || k == node_kind::PARAMETER.0
            || k == node_kind::ENUM_VARIANT.0
        {
            return Some(parent);
        }
        cur = parent;
        guard += 1;
    }
    None
}

/// True when the enclosing item of the position container at `idx` is
/// public. Walks up the parent chain to the nearest item-kind node (the
/// struct / enum / fn / trait / impl that encloses the position), then
/// reads its visibility via [`node_is_public`]. When no item ancestor is
/// found (a malformed tree), the position is treated as non-public so the
/// Public filter excludes it.
fn enclosing_item_is_public(nodes: &[NamNode], idx: usize, source: &[u8]) -> bool {
    let mut cur = idx;
    let mut guard = 0;
    while guard < nodes.len() {
        let parent = nodes[cur].parent.0;
        if parent >= nodes.len() || parent == cur {
            return false;
        }
        if is_item_kind(nodes[parent].kind.0) {
            return node_is_public(nodes, parent, source);
        }
        cur = parent;
        guard += 1;
    }
    false
}

fn make_type_position_diagnostic(
    path: BytesRef,
    line: u32,
    column: u32,
    length: u32,
) -> Diagnostic {
    Diagnostic {
        plugin_id: bytes_ref_static(PLUGIN_ID),
        rule_id: bytes_ref_static(AST_TYPE_POSITION_RULE_ID),
        severity: DiagnosticSeverity::Warn,
        message: bytes_ref_static(AST_TYPE_POSITION_MESSAGE),
        path,
        // type leaves are single-line; the span covers the matched leaf.
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

static AST_TYPE_POSITION_VTABLE: LintEvaluateVtable = LintEvaluateVtable {
    evaluate: ast_type_position_evaluate,
};

/// Provider-export marker for the ast-type-position lint.
pub struct AstTypePositionProvider;

impl ProviderExport for AstTypePositionProvider {
    const ID: ProviderId = PROVIDER_AST_TYPE_POSITION;
    const VTABLE_PTR: *const c_void =
        &AST_TYPE_POSITION_VTABLE as *const LintEvaluateVtable as *const c_void;
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

// The remaining token-scan lints differ from no-std only by provider id and
// baked rule id; the tokens arrive at runtime in the config blob. Generate
// each (provider const, evaluate wrapper, vtable, ProviderExport marker)
// from one macro so the rule id cannot drift by copy-paste.
macro_rules! token_scan_lint {
    ($provider:ident, $eval:ident, $vtable:ident, $marker:ident, $id:literal, $rule:literal) => {
        #[doc = concat!("Provider id for the ", $id, " lint.")]
        pub const $provider: ProviderId = ProviderId::from_name($id);

        /// SAFETY: the host upholds the `LintEvaluateVtable` contract;
        /// delegates to the shared token-scan core with this lint's rule id.
        unsafe extern "C" fn $eval(
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
                    $rule,
                )
            }
        }

        static $vtable: LintEvaluateVtable = LintEvaluateVtable { evaluate: $eval };

        #[doc = concat!("Provider-export marker for the ", $id, " lint.")]
        pub struct $marker;

        impl ProviderExport for $marker {
            const ID: ProviderId = $provider;
            const VTABLE_PTR: *const c_void =
                &$vtable as *const LintEvaluateVtable as *const c_void;
        }
    };
}

token_scan_lint!(
    PROVIDER_NO_ALLOC,
    no_alloc_evaluate,
    NO_ALLOC_VTABLE,
    NoAllocProvider,
    "mockspace-builtin.lint.no-alloc.v2",
    b"no-alloc"
);
token_scan_lint!(
    PROVIDER_NO_DYN_DISPATCH,
    no_dyn_dispatch_evaluate,
    NO_DYN_DISPATCH_VTABLE,
    NoDynDispatchProvider,
    "mockspace-builtin.lint.no-dyn-dispatch.v2",
    b"no-dyn-dispatch"
);
token_scan_lint!(
    PROVIDER_NO_RUNTIME_REGISTRATION,
    no_runtime_registration_evaluate,
    NO_RUNTIME_REGISTRATION_VTABLE,
    NoRuntimeRegistrationProvider,
    "mockspace-builtin.lint.no-runtime-registration.v2",
    b"no-runtime-registration"
);
token_scan_lint!(
    PROVIDER_NO_RUNTIME_SPAWN,
    no_runtime_spawn_evaluate,
    NO_RUNTIME_SPAWN_VTABLE,
    NoRuntimeSpawnProvider,
    "mockspace-builtin.lint.no-runtime-spawn.v2",
    b"no-runtime-spawn"
);

#[export_extension(
    name = "mockspace-builtin-lints",
    version = "0.0.0",
    providers = [
        NoTodoProvider,
        FileSizeProvider,
        AstTypePositionProvider,
        NoStdProvider,
        NoAllocProvider,
        NoDynDispatchProvider,
        NoRuntimeRegistrationProvider,
        NoRuntimeSpawnProvider,
    ],
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
        // metric 6 is past the last metric (TotalItemCount = 5).
        let cfg = FileSizeConfig { metric: 6, inclusive: 1, threshold: arvo::USize(1) };
        let (s, _) = run_file_size(&entries, Some(cfg), 8);
        assert!(matches!(s, AbiStatus::InvalidArg));
    }

    // -- file-size AST item-count metrics (bucket-2) --

    fn nn(
        kind: usize,
        parent: usize,
        start: usize,
        end: usize,
    ) -> NamNode {
        NamNode {
            kind: arvo::USize(kind),
            parent: arvo::USize(parent),
            // first_child is unused by item_metric (it navigates by parent).
            first_child: arvo::USize(0),
            start_byte: arvo::USize(start),
            end_byte: arvo::USize(end),
            start_row: arvo::USize(0),
            end_row: arvo::USize(0),
        }
    }

    fn entry_with_nodes<'a>(
        path: &'a [u8],
        source: &'a [u8],
        nodes: &'a [NamNode],
    ) -> NamFileEntry {
        NamFileEntry {
            path: bytes_ref_static_runtime(path),
            language: arvo::USize(0),
            source: bytes_ref_static_runtime(source),
            nodes: BytesRef {
                data: nodes.as_ptr() as *const u8,
                len: arvo::USize(nodes.len() * core::mem::size_of::<NamNode>()),
            },
        }
    }

    // shared synthetic tree for the item-count tests:
    //   pub fn a(){}   fn b(){}   pub struct C;
    // root + 3 top-level items (2 pub, 1 private) + 2 visibility nodes.
    fn item_nodes() -> [NamNode; 6] {
        let src_len = b"pub fn a(){}\nfn b(){}\npub struct C;\n".len();
        [
            nn(node_kind::SOURCE_FILE.0, 6, 0, src_len), // 0 root (parent = sentinel)
            nn(node_kind::FUNCTION_ITEM.0, 0, 0, 12),    // 1 pub fn a
            nn(node_kind::VISIBILITY_MODIFIER.0, 1, 0, 3), // 2 "pub"
            nn(node_kind::FUNCTION_ITEM.0, 0, 13, 21),   // 3 fn b (private)
            nn(node_kind::STRUCT_ITEM.0, 0, 22, 35),     // 4 pub struct C
            nn(node_kind::VISIBILITY_MODIFIER.0, 4, 22, 25), // 5 "pub"
        ]
    }

    #[test]
    fn item_metric_counts_total_pub_private() {
        let src = b"pub fn a(){}\nfn b(){}\npub struct C;\n";
        let nodes = item_nodes();
        let e = entry_with_nodes(b"a.rs", src, &nodes);
        assert_eq!(item_metric(&e, METRIC_TOTAL_ITEM_COUNT), 3);
        assert_eq!(item_metric(&e, METRIC_PUB_ITEM_COUNT), 2);
        assert_eq!(item_metric(&e, METRIC_PRIVATE_ITEM_COUNT), 1);
    }

    #[test]
    fn item_metric_no_nodes_is_zero() {
        // a v1.0.0-style entry with no serialised tree counts nothing.
        let e = entry(b"a.rs", b"pub fn a(){}\n");
        assert_eq!(item_metric(&e, METRIC_TOTAL_ITEM_COUNT), 0);
    }

    #[test]
    fn item_metric_excludes_nested_items() {
        // a fn nested under a mod is not a top-level item.
        let src = b"mod m { fn inner(){} }\n";
        let nodes = [
            nn(node_kind::SOURCE_FILE.0, 3, 0, src.len()), // 0 root
            nn(node_kind::MOD_ITEM.0, 0, 0, 22),           // 1 mod m (top-level)
            nn(node_kind::FUNCTION_ITEM.0, 1, 8, 20),      // 2 fn inner (under mod, parent=1)
        ];
        let e = entry_with_nodes(b"a.rs", src, &nodes);
        // only the mod counts; the nested fn does not.
        assert_eq!(item_metric(&e, METRIC_TOTAL_ITEM_COUNT), 1);
    }

    #[test]
    fn file_size_item_metric_fires_via_evaluate() {
        let src = b"pub fn a(){}\nfn b(){}\npub struct C;\n";
        let nodes = item_nodes();
        let entries = [entry_with_nodes(b"a.rs", src, &nodes)];
        // PubItemCount is 2; exclusive threshold 1 fires.
        let cfg = FileSizeConfig {
            metric: METRIC_PUB_ITEM_COUNT,
            inclusive: 0,
            threshold: arvo::USize(1),
        };
        let (s, n) = run_file_size(&entries, Some(cfg), 8);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 1);
        // exclusive threshold 2 does not fire (2 is not > 2).
        let cfg = FileSizeConfig {
            metric: METRIC_PUB_ITEM_COUNT,
            inclusive: 0,
            threshold: arvo::USize(2),
        };
        let (s, n) = run_file_size(&entries, Some(cfg), 8);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 0);
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

    // -- ast-type-position --

    fn ast_type_position_blob(
        visibility: u8,
        positions: u8,
        forbidden: &[&[u8]],
    ) -> Vec<u8> {
        let mut b = Vec::new();
        b.push(visibility);
        b.push(positions);
        b.extend_from_slice(&(forbidden.len() as u32).to_le_bytes());
        for t in forbidden {
            b.extend_from_slice(&(t.len() as u32).to_le_bytes());
            b.extend_from_slice(t);
        }
        b
    }

    fn run_ast_type_position(
        entries: &[NamFileEntry],
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
            ast_type_position_evaluate(
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

    // synthetic tree for `pub struct S { f: String }`:
    //   the FIELD_DECLARATION (3) wraps a TYPE_IDENTIFIER (4) "String".
    // byte layout: "pub struct S { f: String }\n"
    //                012345678901234567890123456
    //                          1111111111222222
    // "pub" at 0..3, "String" at 18..24.
    fn pub_struct_string_field() -> ([NamNode; 5], &'static [u8]) {
        let src: &[u8] = b"pub struct S { f: String }\n";
        let nodes = [
            nn(node_kind::SOURCE_FILE.0, 5, 0, src.len()), // 0 root
            nn(node_kind::STRUCT_ITEM.0, 0, 0, 26),        // 1 pub struct S
            nn(node_kind::VISIBILITY_MODIFIER.0, 1, 0, 3), // 2 "pub"
            nn(node_kind::FIELD_DECLARATION.0, 1, 15, 24), // 3 field f
            nn(node_kind::TYPE_IDENTIFIER.0, 3, 18, 24),   // 4 "String"
        ];
        (nodes, src)
    }

    #[test]
    fn ast_type_position_fires_on_struct_field() {
        let (nodes, src) = pub_struct_string_field();
        let e = entry_with_nodes(b"a.rs", src, &nodes);
        let blob = ast_type_position_blob(VISIBILITY_ANY, POSITION_STRUCT_FIELD, &[b"String"]);
        let (s, n, diags) = run_ast_type_position(&[e], &blob, 16);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 1);
        // span covers the "String" leaf at byte 18 (line 1, col 18), 6 bytes.
        assert_eq!(diags[0].range.start.line, 1);
        assert_eq!(diags[0].range.start.column, 18);
        assert_eq!(diags[0].range.end.column, 18 + 6);
        // SAFETY: rule_id points at the static b"ast-type-position".
        let rid = unsafe {
            core::slice::from_raw_parts(diags[0].rule_id.data, diags[0].rule_id.len.0)
        };
        assert_eq!(rid, b"ast-type-position");
    }

    #[test]
    fn ast_type_position_fires_on_fn_param() {
        // "pub fn x(s: String) {}\n"
        //  0123456789...        12: "pub" 0..3, "String" at 12..18.
        let src: &[u8] = b"pub fn x(s: String) {}\n";
        let nodes = [
            nn(node_kind::SOURCE_FILE.0, 5, 0, src.len()), // 0 root
            nn(node_kind::FUNCTION_ITEM.0, 0, 0, 22),      // 1 pub fn x
            nn(node_kind::VISIBILITY_MODIFIER.0, 1, 0, 3), // 2 "pub"
            nn(node_kind::PARAMETER.0, 1, 9, 18),          // 3 param s: String
            nn(node_kind::TYPE_IDENTIFIER.0, 3, 12, 18),   // 4 "String"
        ];
        let e = entry_with_nodes(b"a.rs", src, &nodes);
        let blob = ast_type_position_blob(VISIBILITY_ANY, POSITION_FN_PARAM, &[b"String"]);
        let (s, n, _) = run_ast_type_position(&[e], &blob, 16);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 1);
    }

    #[test]
    fn ast_type_position_fires_on_enum_variant_field() {
        // a struct-style enum variant field: the FIELD_DECLARATION sits
        // under an ENUM_VARIANT, so it classifies as EnumVariantField.
        // "pub enum E { V { f: String } }\n"
        //  "String" at 20..26.
        let src: &[u8] = b"pub enum E { V { f: String } }\n";
        let nodes = [
            nn(node_kind::SOURCE_FILE.0, 6, 0, src.len()), // 0 root
            nn(node_kind::ENUM_ITEM.0, 0, 0, 30),          // 1 pub enum E
            nn(node_kind::VISIBILITY_MODIFIER.0, 1, 0, 3), // 2 "pub"
            nn(node_kind::ENUM_VARIANT.0, 1, 13, 28),      // 3 variant V
            nn(node_kind::FIELD_DECLARATION.0, 3, 17, 26), // 4 field f (under variant)
            nn(node_kind::TYPE_IDENTIFIER.0, 4, 20, 26),   // 5 "String"
        ];
        let e = entry_with_nodes(b"a.rs", src, &nodes);
        // selecting only EnumVariantField fires; selecting only StructField
        // does not (the field is under a variant, not a plain struct).
        let blob_evf =
            ast_type_position_blob(VISIBILITY_ANY, POSITION_ENUM_VARIANT_FIELD, &[b"String"]);
        let (s, n, _) = run_ast_type_position(&[e], &blob_evf, 16);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 1);

        let e2 = entry_with_nodes(b"a.rs", src, &nodes);
        let blob_sf =
            ast_type_position_blob(VISIBILITY_ANY, POSITION_STRUCT_FIELD, &[b"String"]);
        let (_, n2, _) = run_ast_type_position(&[e2], &blob_sf, 16);
        assert_eq!(n2, 0);
    }

    #[test]
    fn ast_type_position_visibility_gates_private_enclosing_item() {
        // a private struct field: Public filter excludes it, Any includes.
        // "struct S { f: String }\n"  "String" at 14..20, no "pub" node.
        let src: &[u8] = b"struct S { f: String }\n";
        let nodes = [
            nn(node_kind::SOURCE_FILE.0, 4, 0, src.len()), // 0 root
            nn(node_kind::STRUCT_ITEM.0, 0, 0, 22),        // 1 struct S (private)
            nn(node_kind::FIELD_DECLARATION.0, 1, 11, 20), // 2 field f
            nn(node_kind::TYPE_IDENTIFIER.0, 2, 14, 20),   // 3 "String"
        ];
        let e = entry_with_nodes(b"a.rs", src, &nodes);
        // Public filter (visibility byte 1) excludes the private struct.
        let blob_pub = ast_type_position_blob(1, POSITION_STRUCT_FIELD, &[b"String"]);
        let (s, n, _) = run_ast_type_position(&[e], &blob_pub, 16);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 0);

        let e2 = entry_with_nodes(b"a.rs", src, &nodes);
        // Any filter includes it.
        let blob_any =
            ast_type_position_blob(VISIBILITY_ANY, POSITION_STRUCT_FIELD, &[b"String"]);
        let (_, n2, _) = run_ast_type_position(&[e2], &blob_any, 16);
        assert_eq!(n2, 1);
    }

    #[test]
    fn ast_type_position_does_not_fire_on_allowed_type() {
        // a type NOT in the forbidden list must not fire.
        let (nodes, src) = pub_struct_string_field();
        let e = entry_with_nodes(b"a.rs", src, &nodes);
        // the field type is "String"; forbid only "Vec".
        let blob = ast_type_position_blob(VISIBILITY_ANY, POSITION_STRUCT_FIELD, &[b"Vec"]);
        let (s, n, _) = run_ast_type_position(&[e], &blob, 16);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 0);
    }

    #[test]
    fn ast_type_position_position_bitset_narrows() {
        // the forbidden type sits in a struct field, but only FnParam is
        // selected; the lint stays silent (mirrors the in-process negative
        // case for unlisted positions).
        let (nodes, src) = pub_struct_string_field();
        let e = entry_with_nodes(b"a.rs", src, &nodes);
        let blob = ast_type_position_blob(VISIBILITY_ANY, POSITION_FN_PARAM, &[b"String"]);
        let (s, n, _) = run_ast_type_position(&[e], &blob, 16);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 0);
    }

    #[test]
    fn ast_type_position_matches_primitive_type_leaf() {
        // PRIMITIVE_TYPE leaves are tested too: "pub fn x(n: u32) {}".
        //  "u32" at 12..15.
        let src: &[u8] = b"pub fn x(n: u32) {}\n";
        let nodes = [
            nn(node_kind::SOURCE_FILE.0, 5, 0, src.len()), // 0 root
            nn(node_kind::FUNCTION_ITEM.0, 0, 0, 19),      // 1 pub fn x
            nn(node_kind::VISIBILITY_MODIFIER.0, 1, 0, 3), // 2 "pub"
            nn(node_kind::PARAMETER.0, 1, 9, 15),          // 3 param n: u32
            nn(node_kind::PRIMITIVE_TYPE.0, 3, 12, 15),    // 4 "u32"
        ];
        let e = entry_with_nodes(b"a.rs", src, &nodes);
        let blob = ast_type_position_blob(VISIBILITY_ANY, POSITION_FN_PARAM, &[b"u32"]);
        let (s, n, diags) = run_ast_type_position(&[e], &blob, 16);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 1);
        assert_eq!(diags[0].range.start.column, 12);
        assert_eq!(diags[0].range.end.column, 12 + 3);
    }

    #[test]
    fn ast_type_position_overflow_reports_would_have_count() {
        // two fields with a forbidden type; capacity 1 truncates and the
        // would-have count is the full match total.
        // "pub struct S { a: Vec, b: Vec }\n"  "Vec" at 18..21 and 26..29.
        let src: &[u8] = b"pub struct S { a: Vec, b: Vec }\n";
        let nodes = [
            nn(node_kind::SOURCE_FILE.0, 7, 0, src.len()), // 0 root
            nn(node_kind::STRUCT_ITEM.0, 0, 0, 31),        // 1 pub struct S
            nn(node_kind::VISIBILITY_MODIFIER.0, 1, 0, 3), // 2 "pub"
            nn(node_kind::FIELD_DECLARATION.0, 1, 15, 21), // 3 field a
            nn(node_kind::TYPE_IDENTIFIER.0, 3, 18, 21),   // 4 "Vec"
            nn(node_kind::FIELD_DECLARATION.0, 1, 23, 29), // 5 field b
            nn(node_kind::TYPE_IDENTIFIER.0, 5, 26, 29),   // 6 "Vec"
        ];
        let e = entry_with_nodes(b"a.rs", src, &nodes);
        let blob = ast_type_position_blob(VISIBILITY_ANY, POSITION_STRUCT_FIELD, &[b"Vec"]);
        let (s, n, diags) = run_ast_type_position(&[e], &blob, 1);
        assert!(matches!(s, AbiStatus::Internal));
        assert_eq!(n, 2);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn ast_type_position_rejects_null_out_args_and_empty_config() {
        // null out_entries -> InvalidArg.
        let (nodes, src) = pub_struct_string_field();
        let e = entry_with_nodes(b"a.rs", src, &nodes);
        let entries = [e];
        let nam = payload(&entries);
        let blob = ast_type_position_blob(VISIBILITY_ANY, POSITION_STRUCT_FIELD, &[b"String"]);
        let mut out_len = arvo::USize(0);
        // SAFETY: null out_entries is the case under test; the fn returns
        // before any write.
        let s_null = unsafe {
            ast_type_position_evaluate(
                core::ptr::null_mut(),
                &nam,
                blob.as_ptr(),
                arvo::USize(blob.len()),
                core::ptr::null_mut(),
                arvo::USize(4),
                &mut out_len,
            )
        };
        assert!(matches!(s_null, AbiStatus::InvalidArg));

        // empty config (len 0) is a no-op, not an error.
        let (s_empty, n_empty, _) = run_ast_type_position(&entries, &[], 16);
        assert!(matches!(s_empty, AbiStatus::Ok));
        assert_eq!(n_empty, 0);
    }

    #[test]
    fn ast_type_position_rejects_malformed_config() {
        let (nodes, src) = pub_struct_string_field();
        let e = entry_with_nodes(b"a.rs", src, &nodes);
        let entries = [e];
        // short header (< 6 bytes).
        let (s1, _, _) = run_ast_type_position(&entries, &[0, 1, 0], 16);
        assert!(matches!(s1, AbiStatus::InvalidArg));
        // forbidden_count beyond MAX_FORBIDDEN.
        let mut huge = std::vec![0u8, POSITION_STRUCT_FIELD];
        huge.extend_from_slice(&(u32::MAX).to_le_bytes());
        let (s2, _, _) = run_ast_type_position(&entries, &huge, 16);
        assert!(matches!(s2, AbiStatus::InvalidArg));
        // a type-name length running past the buffer.
        let mut overrun = std::vec![0u8, POSITION_STRUCT_FIELD];
        overrun.extend_from_slice(&(1u32).to_le_bytes()); // count 1
        overrun.extend_from_slice(&(99u32).to_le_bytes()); // len 99, no bytes
        let (s3, _, _) = run_ast_type_position(&entries, &overrun, 16);
        assert!(matches!(s3, AbiStatus::InvalidArg));
    }

    #[test]
    fn ast_type_position_no_nodes_entry_is_skipped() {
        // a v1.0.0-style entry with no serialised tree emits nothing.
        let e = entry(b"a.rs", b"pub struct S { f: String }\n");
        let blob = ast_type_position_blob(VISIBILITY_ANY, POSITION_STRUCT_FIELD, &[b"String"]);
        let (s, n, _) = run_ast_type_position(&[e], &blob, 16);
        assert!(matches!(s, AbiStatus::Ok));
        assert_eq!(n, 0);
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

    #[test]
    fn token_scan_family_bakes_distinct_rule_ids() {
        // the four remaining token-scan wrappers differ only by baked rule
        // id; verify each emits its own, sharing the same core + config.
        type Eval = unsafe extern "C" fn(
            *mut c_void,
            *const NamPayload,
            *const u8,
            arvo::USize,
            *mut Diagnostic,
            arvo::USize,
            *mut arvo::USize,
        ) -> AbiStatus;
        let cases: [(Eval, &[u8]); 4] = [
            (no_alloc_evaluate, b"no-alloc"),
            (no_dyn_dispatch_evaluate, b"no-dyn-dispatch"),
            (no_runtime_registration_evaluate, b"no-runtime-registration"),
            (no_runtime_spawn_evaluate, b"no-runtime-spawn"),
        ];
        let entries = [entry(b"a.rs", b"x marker y\n")];
        let blob = token_blob(false, (true, true, true), &[b"marker"]);
        for (eval, expected_rule) in cases {
            let nam = payload(&entries);
            let mut buf: [MaybeUninit<Diagnostic>; 4] =
                [const { MaybeUninit::uninit() }; 4];
            let mut out_len = arvo::USize(0);
            // SAFETY: nam valid; buf has 4 slots; out_len valid; blob live.
            let status = unsafe {
                eval(
                    core::ptr::null_mut(),
                    &nam,
                    blob.as_ptr(),
                    arvo::USize(blob.len()),
                    buf.as_mut_ptr() as *mut Diagnostic,
                    arvo::USize(4),
                    &mut out_len,
                )
            };
            assert!(matches!(status, AbiStatus::Ok));
            assert_eq!(out_len.0, 1);
            // SAFETY: one entry written.
            let d = unsafe { buf[0].assume_init() };
            // SAFETY: rule_id points at the macro-baked static byte slice.
            let rid = unsafe {
                core::slice::from_raw_parts(d.rule_id.data, d.rule_id.len.0)
            };
            assert_eq!(rid, expected_rule);
        }
    }
}
