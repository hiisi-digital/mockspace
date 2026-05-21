//! Rust attribute parser for the 5-directive vocabulary.
//!
//! Per the reconciled design memo at
//! `mock/research/202605221700_directive-vocabulary-reconciled.md`,
//! the comment form is canonical, with idiomatic Rust attributes
//! shipping as additive aliases that map to identical internal
//! [`DirectiveRecord`] values.
//!
//! # Attribute grammar
//!
//! Names that contain hyphens (kebab-case lint names like
//! `no-bare-numeric`) cannot be bare identifiers inside a Rust
//! attribute. The attribute alias form uses quoted string literals
//! everywhere a kebab-case name would appear in the comment form:
//!
//! ```rust
//! #[mockspace::allow("no-bare-numeric", reason = "...", tracked = "#427")]
//! #[mockspace::scope_add("no-bare-numeric", axis = "exempt_paths", value = "tests/**")]
//! #[mockspace::defer("no-bare-string", until = "#185", reason = "...")]
//! #[mockspace::file_disable("writing-style", reason = "...", tracked = "#207")]
//! #[mockspace::prop("audited")]
//! #[mockspace::prop("arena_size", value = 4096)]
//! #[mockspace::prop("audit_id", value = "A-2026-04")]
//! #[mockspace::prop("thread_safe", value = true, reason = "verified by audit")]
//! ```
//!
//! Note the snake_case attribute path for `scope_add` and
//! `file_disable`. Rust attribute paths cannot contain `-`; the
//! mapping to canonical directive names preserves the comment form's
//! `scope-add` and `file-disable` after parsing.
//!
//! Each `Directive::ScopeAdd` axis is itself a kebab-case string
//! token in the comment form; the attribute form requires the axis
//! to be a quoted string too. The mockspace-core `ScopeAxis` enum
//! lists the seven valid values; unknown axes parse to a skipped
//! attribute, not an error.
//!
//! `prop` accepts string, integer, and boolean literal values
//! (parsed into `PropValue::String` / `Integer` / `Bool`),
//! mirroring the comment form's three value shapes. The presence
//! form `#[mockspace::prop("name")]` parses to `PropValue::Bool(true)`
//! identically to the comment form `// lint:prop(name)`.

use mockspace_core::lint::{Directive, DirectiveRecord, PropValue, ScopeAxis, Span};
use syn::spanned::Spanned;

/// Walk every item in `ast` looking for `#[mockspace::*]` attribute
/// aliases. Each recognised attribute produces one [`DirectiveRecord`].
///
/// `path` is recorded on every emitted [`Span`] so downstream code can
/// reference the directive's location.
pub fn parse_directive_attributes(ast: &syn::File, path: &str) -> Vec<DirectiveRecord> {
    let mut out = Vec::new();
    walk_items(&ast.items, path, &mut out);
    out
}

fn walk_items(items: &[syn::Item], path: &str, out: &mut Vec<DirectiveRecord>) {
    for item in items {
        let attrs = item_attrs(item);
        for attr in attrs {
            if let Some(record) = parse_attr(attr, path) {
                out.push(record);
            }
        }
        walk_inner(item, path, out);
    }
}

/// Recurse into the nested-item surface of `item` so directives placed
/// on trait methods, impl block members, foreign-mod items, mod
/// contents, enum variants, and named struct fields are also reached.
///
/// Covered:
/// - `Item::Mod`: walks the module's items recursively.
/// - `Item::Impl`: visits each impl-block item's attrs.
/// - `Item::Trait`: visits each trait-item attrs.
/// - `Item::ForeignMod`: visits each foreign item's attrs.
/// - `Item::Enum`: visits each variant's attrs.
/// - `Item::Struct`: visits each named field's attrs.
/// - `Item::Union`: visits each named field's attrs.
fn walk_inner(item: &syn::Item, path: &str, out: &mut Vec<DirectiveRecord>) {
    match item {
        syn::Item::Mod(m) => {
            if let Some((_, inner)) = &m.content {
                walk_items(inner, path, out);
            }
        }
        syn::Item::Impl(im) => {
            for inner in &im.items {
                let attrs: &[syn::Attribute] = match inner {
                    syn::ImplItem::Fn(it) => &it.attrs,
                    syn::ImplItem::Const(it) => &it.attrs,
                    syn::ImplItem::Type(it) => &it.attrs,
                    _ => &[],
                };
                visit_attrs(attrs, path, out);
            }
        }
        syn::Item::Trait(tr) => {
            for inner in &tr.items {
                let attrs: &[syn::Attribute] = match inner {
                    syn::TraitItem::Fn(it) => &it.attrs,
                    syn::TraitItem::Const(it) => &it.attrs,
                    syn::TraitItem::Type(it) => &it.attrs,
                    _ => &[],
                };
                visit_attrs(attrs, path, out);
            }
        }
        syn::Item::ForeignMod(fm) => {
            for inner in &fm.items {
                let attrs: &[syn::Attribute] = match inner {
                    syn::ForeignItem::Fn(it) => &it.attrs,
                    syn::ForeignItem::Static(it) => &it.attrs,
                    syn::ForeignItem::Type(it) => &it.attrs,
                    _ => &[],
                };
                visit_attrs(attrs, path, out);
            }
        }
        syn::Item::Enum(en) => {
            for variant in &en.variants {
                visit_attrs(&variant.attrs, path, out);
            }
        }
        syn::Item::Struct(st) => {
            for field in &st.fields {
                visit_attrs(&field.attrs, path, out);
            }
        }
        syn::Item::Union(un) => {
            for field in &un.fields.named {
                visit_attrs(&field.attrs, path, out);
            }
        }
        _ => {}
    }
}

fn visit_attrs(attrs: &[syn::Attribute], path: &str, out: &mut Vec<DirectiveRecord>) {
    for attr in attrs {
        if let Some(record) = parse_attr(attr, path) {
            out.push(record);
        }
    }
}

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(it) => &it.attrs,
        syn::Item::Enum(it) => &it.attrs,
        syn::Item::ExternCrate(it) => &it.attrs,
        syn::Item::Fn(it) => &it.attrs,
        syn::Item::ForeignMod(it) => &it.attrs,
        syn::Item::Impl(it) => &it.attrs,
        syn::Item::Macro(it) => &it.attrs,
        syn::Item::Mod(it) => &it.attrs,
        syn::Item::Static(it) => &it.attrs,
        syn::Item::Struct(it) => &it.attrs,
        syn::Item::Trait(it) => &it.attrs,
        syn::Item::TraitAlias(it) => &it.attrs,
        syn::Item::Type(it) => &it.attrs,
        syn::Item::Union(it) => &it.attrs,
        syn::Item::Use(it) => &it.attrs,
        _ => &[],
    }
}

/// Identify the directive keyword from a `#[mockspace::<keyword>(...)]`
/// attribute path. Returns `None` for attributes outside the
/// `mockspace` namespace or with an unknown keyword.
fn parse_attr(attr: &syn::Attribute, path: &str) -> Option<DirectiveRecord> {
    let keyword = attr_keyword(attr)?;
    let span = span_of(attr.span(), path);
    let directive = match keyword.as_str() {
        "allow" => parse_allow(attr)?,
        "scope_add" => parse_scope_add(attr)?,
        "defer" => parse_defer(attr)?,
        "file_disable" => parse_file_disable(attr)?,
        "prop" => parse_prop(attr)?,
        _ => return None,
    };
    Some(DirectiveRecord::from_attribute(directive, span))
}

/// Extract `<keyword>` from `#[mockspace::<keyword>(...)]`. Accepts
/// only the `mockspace::*` namespace.
fn attr_keyword(attr: &syn::Attribute) -> Option<String> {
    let path = attr.path();
    if path.segments.len() != 2 {
        return None;
    }
    if path.segments[0].ident != "mockspace" {
        return None;
    }
    Some(path.segments[1].ident.to_string())
}

fn parse_allow(attr: &syn::Attribute) -> Option<Directive> {
    let args = collect_args(attr)?;
    let lint_name = args.positional.first().cloned()?;
    Some(Directive::Allow {
        lint_name,
        reason: args.keyed("reason"),
        tracked: args.keyed("tracked"),
    })
}

fn parse_scope_add(attr: &syn::Attribute) -> Option<Directive> {
    let args = collect_args(attr)?;
    let lint_name = args.positional.first().cloned()?;
    let axis_str = args.keyed("axis")?;
    let value = args.keyed("value")?;
    let axis = parse_axis(&axis_str)?;
    Some(Directive::ScopeAdd {
        lint_name,
        axis,
        value,
    })
}

fn parse_defer(attr: &syn::Attribute) -> Option<Directive> {
    let args = collect_args(attr)?;
    let lint_name = args.positional.first().cloned()?;
    let until = args.keyed("until")?;
    Some(Directive::Defer {
        lint_name,
        until,
        reason: args.keyed("reason"),
    })
}

fn parse_file_disable(attr: &syn::Attribute) -> Option<Directive> {
    let args = collect_args(attr)?;
    let lint_name = args.positional.first().cloned()?;
    Some(Directive::FileDisable {
        lint_name,
        reason: args.keyed("reason"),
        tracked: args.keyed("tracked"),
    })
}

/// Parse `#[mockspace::prop("<name>")]` (presence form) or
/// `#[mockspace::prop("<name>", value = <lit>)]` (key-value form),
/// optionally with `reason = "..."`. Mirrors the comment form's three
/// value shapes (Bool / Integer / String) via [`parse_prop_args`].
fn parse_prop(attr: &syn::Attribute) -> Option<Directive> {
    let parsed = parse_prop_args(attr)?;
    Some(Directive::Prop {
        name: parsed.name,
        value: parsed.value,
        reason: parsed.reason,
    })
}

struct PropAttrArgs {
    name: String,
    value: PropValue,
    reason: Option<String>,
}

/// Parse the body of `#[mockspace::prop(...)]`. Distinct from
/// [`collect_args`] because the `value =` arm accepts non-string
/// literals (bool / integer); the four other directives only ever
/// take string literals.
///
/// Forgiving-failure modes (silent rather than parse-rejecting):
/// - Multiple positionals: only the first becomes `name`.
/// - Multiple `value =` assignments: last-write-wins.
/// - Unknown keyed args: silently ignored.
/// - `reason =` with a non-string literal (e.g. `reason = 42`):
///   `reason` stays `None`, the rest of the directive parses
///   successfully. More forgiving than `collect_args`, which
///   rejects the whole attribute when a keyed value is not a
///   `LitStr`. Deliberate divergence; the prop directive's value
///   surface is heterogeneous and a single non-string `reason`
///   should not cancel an otherwise valid directive.
fn parse_prop_args(attr: &syn::Attribute) -> Option<PropAttrArgs> {
    let parsed = attr
        .parse_args_with(syn::punctuated::Punctuated::<PropAttrArg, syn::Token![,]>::parse_terminated)
        .ok()?;
    let mut name: Option<String> = None;
    let mut value: Option<PropValue> = None;
    let mut reason: Option<String> = None;
    for arg in parsed {
        match arg {
            PropAttrArg::Positional(s) => {
                if name.is_none() {
                    name = Some(s);
                }
            }
            PropAttrArg::Keyed(key, val) => match key.as_str() {
                "value" => value = Some(val),
                "reason" => {
                    if let PropValue::String(s) = val {
                        reason = Some(s);
                    }
                }
                _ => {}
            },
        }
    }
    Some(PropAttrArgs {
        name: name?,
        value: value.unwrap_or(PropValue::Bool(true)),
        reason,
    })
}

/// One argument inside `#[mockspace::prop(...)]`. Mirrors the
/// comment form's three value shapes for the `value =` key.
enum PropAttrArg {
    Positional(String),
    Keyed(String, PropValue),
}

impl syn::parse::Parse for PropAttrArg {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        if input.peek(syn::Ident) && input.peek2(syn::Token![=]) {
            let key: syn::Ident = input.parse()?;
            let _eq: syn::Token![=] = input.parse()?;
            let value = parse_prop_literal(input)?;
            Ok(PropAttrArg::Keyed(key.to_string(), value))
        } else {
            let value: syn::LitStr = input.parse()?;
            Ok(PropAttrArg::Positional(value.value()))
        }
    }
}

/// Parse a string / integer / boolean literal as a [`PropValue`].
/// Mirrors [`parse_prop_value`] in `comment.rs`.
fn parse_prop_literal(input: syn::parse::ParseStream) -> syn::Result<PropValue> {
    let lookahead = input.lookahead1();
    if lookahead.peek(syn::LitStr) {
        let lit: syn::LitStr = input.parse()?;
        Ok(PropValue::String(lit.value()))
    } else if lookahead.peek(syn::LitInt) {
        let lit: syn::LitInt = input.parse()?;
        let n: i64 = lit.base10_parse()?;
        Ok(PropValue::Integer(n))
    } else if lookahead.peek(syn::LitBool) {
        let lit: syn::LitBool = input.parse()?;
        Ok(PropValue::Bool(lit.value))
    } else {
        Err(lookahead.error())
    }
}

fn parse_axis(s: &str) -> Option<ScopeAxis> {
    Some(match s {
        "paths" => ScopeAxis::Paths,
        "exempt_paths" => ScopeAxis::ExemptPaths,
        "crates" => ScopeAxis::Crates,
        "exempt_crates" => ScopeAxis::ExemptCrates,
        "languages" => ScopeAxis::Languages,
        "proc_macro_exempt" => ScopeAxis::ProcMacroExempt,
        _ => return None,
    })
}

/// Parsed argument list from a `#[mockspace::<keyword>(<args>)]`
/// attribute. Positional args are string-literal values; keyed args
/// are `key = "value"` pairs.
#[derive(Default)]
struct Args {
    positional: Vec<String>,
    keyed: Vec<(String, String)>,
}

impl Args {
    fn keyed(&self, key: &str) -> Option<String> {
        self.keyed
            .iter()
            .find_map(|(k, v)| (k == key).then(|| v.clone()))
    }
}

/// Parse the body of `#[mockspace::<keyword>(<body>)]` into
/// positional + keyed args. Returns `None` if the attribute body
/// is not in the expected `(<args>)` shape.
fn collect_args(attr: &syn::Attribute) -> Option<Args> {
    let mut args = Args::default();
    // syn's `parse_args_with` accepts a `Punctuated<...>` parser.
    // Each comma-separated arg is one of:
    //   - a string literal (positional)
    //   - `<ident> = <string-literal>` (keyed)
    let parsed = attr
        .parse_args_with(syn::punctuated::Punctuated::<AttrArg, syn::Token![,]>::parse_terminated)
        .ok()?;
    for arg in parsed {
        match arg {
            AttrArg::Positional(s) => args.positional.push(s),
            AttrArg::Keyed(k, v) => args.keyed.push((k, v)),
        }
    }
    Some(args)
}

/// A single argument inside `#[mockspace::<keyword>(...)]`.
enum AttrArg {
    Positional(String),
    Keyed(String, String),
}

impl syn::parse::Parse for AttrArg {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        if input.peek(syn::Ident) && input.peek2(syn::Token![=]) {
            let key: syn::Ident = input.parse()?;
            let _eq: syn::Token![=] = input.parse()?;
            let value: syn::LitStr = input.parse()?;
            Ok(AttrArg::Keyed(key.to_string(), value.value()))
        } else {
            let value: syn::LitStr = input.parse()?;
            Ok(AttrArg::Positional(value.value()))
        }
    }
}

/// Convert a `proc_macro2::Span` into a `mockspace_core::lint::Span`
/// scoped to `path`. proc_macro2 spans on stable lack column data;
/// we record the line and best-effort column.
fn span_of(p_span: proc_macro2::Span, path: &str) -> Span {
    let start = p_span.start();
    let end = p_span.end();
    let len = if end.line == start.line {
        (end.column.saturating_sub(start.column)) as u32
    } else {
        0
    };
    Span::single_line(
        path,
        start.line as u32,
        (start.column + 1) as u32,
        len.max(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockspace_core::lint::Directive;

    fn parse(src: &str) -> Vec<DirectiveRecord> {
        let ast = syn::parse_file(src).expect("parse_file");
        parse_directive_attributes(&ast, "x.rs")
    }

    #[test]
    fn parses_mockspace_allow_attribute() {
        let src = r##"
#[mockspace::allow("no-bare-numeric", reason = "spec-fixed", tracked = "#427")]
const X: u64 = 1;
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Allow {
                lint_name,
                reason,
                tracked,
            } => {
                assert_eq!(lint_name, "no-bare-numeric");
                assert_eq!(reason.as_deref(), Some("spec-fixed"));
                assert_eq!(tracked.as_deref(), Some("#427"));
            }
            other => panic!("expected Allow, got {other:?}"),
        }
    }

    #[test]
    fn parses_mockspace_scope_add_attribute() {
        let src = r##"
#[mockspace::scope_add("no-bare-numeric", axis = "exempt_paths", value = "tests/**")]
mod ffi {}
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::ScopeAdd {
                lint_name,
                axis,
                value,
            } => {
                assert_eq!(lint_name, "no-bare-numeric");
                assert_eq!(*axis, ScopeAxis::ExemptPaths);
                assert_eq!(value, "tests/**");
            }
            other => panic!("expected ScopeAdd, got {other:?}"),
        }
    }

    #[test]
    fn parses_mockspace_defer_attribute() {
        let src = r##"
#[mockspace::defer("no-bare-string", until = "#185", reason = "test rehab pending")]
fn legacy(name: String) {}
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Defer {
                lint_name,
                until,
                reason,
            } => {
                assert_eq!(lint_name, "no-bare-string");
                assert_eq!(until, "#185");
                assert_eq!(reason.as_deref(), Some("test rehab pending"));
            }
            other => panic!("expected Defer, got {other:?}"),
        }
    }

    #[test]
    fn parses_mockspace_file_disable_attribute() {
        // file_disable is item-level in attribute form (Rust has no
        // inner-attribute syntax for arbitrary files in stable). The
        // attribute placed on the crate-root marker item is enough.
        let src = r##"
#[mockspace::file_disable("writing-style", reason = "generated FFI", tracked = "#207")]
pub struct CrateRoot;
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 1);
        assert!(matches!(
            &recs[0].directive,
            Directive::FileDisable { lint_name, .. } if lint_name == "writing-style"
        ));
    }

    #[test]
    fn ignores_non_mockspace_attributes() {
        let src = r##"
#[derive(Debug)]
#[allow(dead_code)]
struct X;
"##;
        let recs = parse(src);
        assert!(recs.is_empty());
    }

    #[test]
    fn ignores_unknown_mockspace_keyword() {
        let src = r##"
#[mockspace::unknown("foo")]
struct X;
"##;
        let recs = parse(src);
        assert!(recs.is_empty());
    }

    #[test]
    fn walks_into_modules_and_impl_blocks() {
        let src = r##"
mod inner {
    #[mockspace::allow("no-bare-numeric")]
    const X: u64 = 1;
}

struct Y;
impl Y {
    #[mockspace::file_disable("writing-style")]
    fn helper() {}
}
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 2);
        let names: Vec<&str> = recs
            .iter()
            .map(|r| match &r.directive {
                Directive::Allow { lint_name, .. } => lint_name.as_str(),
                Directive::FileDisable { lint_name, .. } => lint_name.as_str(),
                _ => "other",
            })
            .collect();
        assert!(names.contains(&"no-bare-numeric"));
        assert!(names.contains(&"writing-style"));
    }

    #[test]
    fn missing_reason_and_tracked_yield_none() {
        let src = r##"
#[mockspace::allow("no-bare-numeric")]
const X: u64 = 1;
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Allow {
                reason, tracked, ..
            } => {
                assert!(reason.is_none());
                assert!(tracked.is_none());
            }
            other => panic!("expected Allow, got {other:?}"),
        }
    }

    #[test]
    fn ill_formed_attribute_body_is_skipped() {
        let src = r##"
#[mockspace::allow(not_a_string_literal)]
const X: u64 = 1;
"##;
        let recs = parse(src);
        assert!(recs.is_empty(), "got {recs:?}");
    }

    #[test]
    fn span_records_the_attribute_position() {
        let src = "struct A;\n#[mockspace::allow(\"no-bare-numeric\")]\nstruct B;\n";
        let recs = parse(src);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].span.start_line, 2);
    }

    #[test]
    fn multiple_mockspace_attributes_on_same_item() {
        let src = r##"
#[mockspace::allow("no-bare-numeric")]
#[mockspace::allow("no-bare-string")]
fn f() {}
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 2);
    }

    #[test]
    fn walks_into_trait_items() {
        let src = r##"
trait T {
    #[mockspace::allow("no-bare-numeric")]
    fn method();

    #[mockspace::allow("no-bare-vec")]
    type Assoc;

    #[mockspace::allow("no-bare-string")]
    const C: u64;
}
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 3);
    }

    #[test]
    fn walks_into_foreign_mod_items() {
        let src = r##"
extern "C" {
    #[mockspace::allow("no-bare-numeric")]
    fn external_call();

    #[mockspace::allow("no-bare-string")]
    static FOO: i32;
}
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 2);
    }

    #[test]
    fn walks_into_enum_variants() {
        let src = r##"
enum E {
    #[mockspace::allow("no-bare-numeric")]
    A,
    #[mockspace::allow("no-bare-string")]
    B(u32),
}
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 2);
    }

    #[test]
    fn walks_into_struct_fields() {
        let src = r##"
struct S {
    #[mockspace::allow("no-bare-numeric")]
    raw_count: u64,
    #[mockspace::allow("no-bare-string")]
    raw_ptr: *const u8,
}
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 2);
    }

    #[test]
    fn unknown_scope_axis_is_skipped() {
        // parse_axis returning None must surface as a skipped record,
        // not a ScopeAdd with a fabricated axis.
        let src = r##"#[mockspace::scope_add("my-lint", axis = "bogus_axis", value = "v")]
fn f() {}
"##;
        let recs = parse(src);
        assert!(recs.is_empty(), "expected empty, got {recs:?}");
    }

    #[test]
    fn all_six_scope_axes_parse() {
        // Six scope axes after the category retirement in #549. The
        // loop iterates and asserts each parses cleanly. Name reflects
        // the actual count; the previous name claimed seven.
        for axis_str in [
            "paths",
            "exempt_paths",
            "crates",
            "exempt_crates",
            "languages",
            "proc_macro_exempt",
        ] {
            let src = format!(
                "#[mockspace::scope_add(\"my-lint\", axis = \"{axis_str}\", value = \"v\")]\nfn f() {{}}\n"
            );
            let recs = parse(&src);
            assert_eq!(recs.len(), 1, "axis `{axis_str}` did not parse");
        }
    }

    // ---- prop attribute (#545) -------------------------------------------

    #[test]
    fn parses_mockspace_prop_presence_form() {
        let src = r##"
#[mockspace::prop("audited")]
fn critical_path() {}
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Prop { name, value, reason } => {
                assert_eq!(name, "audited");
                assert_eq!(*value, PropValue::Bool(true));
                assert!(reason.is_none());
            }
            other => panic!("expected Prop, got {other:?}"),
        }
    }

    #[test]
    fn parses_mockspace_prop_integer_value() {
        let src = r##"
#[mockspace::prop("arena_size", value = 4096)]
struct StaticBuffer;
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Prop { name, value, .. } => {
                assert_eq!(name, "arena_size");
                assert_eq!(*value, PropValue::Integer(4096));
            }
            other => panic!("expected Prop, got {other:?}"),
        }
    }

    #[test]
    fn parses_mockspace_prop_string_value() {
        let src = r##"
#[mockspace::prop("audit_id", value = "A-2026-04")]
pub fn export_descriptor() {}
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Prop { name, value, .. } => {
                assert_eq!(name, "audit_id");
                assert_eq!(*value, PropValue::String("A-2026-04".to_string()));
            }
            other => panic!("expected Prop, got {other:?}"),
        }
    }

    #[test]
    fn parses_mockspace_prop_bool_value_with_reason() {
        let src = r##"
#[mockspace::prop("thread_safe", value = true, reason = "verified by audit")]
struct Pool;
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Prop { name, value, reason } => {
                assert_eq!(name, "thread_safe");
                assert_eq!(*value, PropValue::Bool(true));
                assert_eq!(reason.as_deref(), Some("verified by audit"));
            }
            other => panic!("expected Prop, got {other:?}"),
        }
    }

    #[test]
    fn parses_mockspace_prop_negative_integer() {
        let src = r##"
#[mockspace::prop("offset", value = -8)]
struct Frame;
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Prop { name, value, .. } => {
                assert_eq!(name, "offset");
                assert_eq!(*value, PropValue::Integer(-8));
            }
            other => panic!("expected Prop, got {other:?}"),
        }
    }

    #[test]
    fn prop_without_positional_name_silently_drops() {
        // Parser refuses to fabricate a name; the attribute is silently
        // dropped per the broader "missing required positional" pattern.
        let src = r##"
#[mockspace::prop(value = 42)]
struct X;
"##;
        let recs = parse(src);
        assert!(recs.is_empty());
    }

    #[test]
    fn prop_duplicate_value_keys_last_write_wins() {
        // Pins the forgiving-failure mode documented in parse_prop_args:
        // a later `value =` assignment overwrites an earlier one.
        let src = r##"
#[mockspace::prop("arena_size", value = 1024, value = 4096)]
struct X;
"##;
        let recs = parse(src);
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Prop { value, .. } => {
                assert_eq!(*value, PropValue::Integer(4096));
            }
            other => panic!("expected Prop, got {other:?}"),
        }
    }
}
