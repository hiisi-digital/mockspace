# Lint System Redesign

**Status:** proposed
**Context:** discovered while integrating mockspace into the saalis project

## Problem

The lint system has several issues that block adoption by projects
other than loimu:

1. Many lints hardcode loimu-specific macros (`define_id!`, `define_error!`,
   `define_signal!`, etc.) as the "correct" pattern
2. Lint severity is not configurable per-project
3. No support for consumer-side custom lints
4. Several code quality issues in the lint implementation

## Proposed: Hierarchical Configurable Lints

### Per-lint configuration in mockspace.toml

```toml
[lints]
# Base severity for the whole lint
no-float = "build-gate"

# Sub-finding severity (overrides base for specific report types)
[lints.no-float]
type-annotation = "error"
struct-field = "error"
function-return = "warn"

# Lint parameters (project-specific patterns)
[lints.no-manual-id]
severity = "error"
allowed_patterns = ["EntityId", ".*Id$"]  # regex
suggested_fix = "use a newtype from saalis-sdk"

[lints.no-adhoc-error-enum]
severity = "error"
allowed_patterns = ["ApiError", "ApiResult"]  # web-internal exceptions
```

### Lint trait changes

```rust
pub trait Lint {
    fn name(&self) -> &'static str;
    fn check(&self, ctx: &LintContext) -> Vec<LintError>;
    fn source_only(&self) -> bool { true }
    fn default_severity(&self) -> Severity { Severity::HARD_ERROR }

    /// Sub-finding names this lint can produce (for granular config).
    fn finding_kinds(&self) -> &[&str] { &[] }

    /// Configuration keys this lint accepts (for project-specific params).
    fn config_keys(&self) -> &[&str] { &[] }

    /// Called before check() with project-specific config values.
    fn configure(&mut self, params: &HashMap<String, String>) {}
}
```

Each `LintError` carries a `finding_kind: Option<&'static str>` so
the config system can override severity per sub-finding.

### Severity override logic

1. If `lints.{name}.{finding_kind}` exists → use that severity
2. Else if `lints.{name}` (base) exists → use that severity
3. Else → use `lint.default_severity()`
4. Per-violation severity from the lint is preserved ONLY when no
   override applies (fixes issue #7 from review)

### Consumer custom lints

If `{mock_dir}/lints/` exists:
- Each `.rs` file exports `pub fn lint() -> Box<dyn Lint>`
- Compiled into the proxy crate via `#[path = "..."]`
- Same Lint trait, same config system
- Custom lints can have their own `[lints.my-custom-lint]` config

## Code Review Findings to Fix

### Critical
1. `render_agent.rs` — `use std::os::unix::fs::PermissionsExt` without
   `#[cfg(unix)]` guard. Fails to compile on Windows.

### Important
2. `bootstrap.rs` — Windows path backslashes break `#[path = "..."]`.
   Module names from filenames not validated as Rust identifiers.
3. `entry.rs` — `process::exit(1)` in library function skips destructors.
   Should return `ExitCode::FAILURE`.
4. `config.rs` — `parse_string` stops finding top-level keys after any
   `[section]` header. `in_section` flag never resets.
5. `config.rs` — `parse_string_array` matches `key` as prefix of longer
   keys (e.g., `layers` matches `layers_extra`).
6. `lint/mod.rs` — severity override kills per-violation nuance. Only
   override when config is explicitly present.
7. `config.rs` — `parse_lints_section` silently drops multi-line inline
   tables.

### Minor
8. `render_design.rs` — `now_rfc3339` uses `date` command, non-functional
   on Windows.
9. `bootstrap.rs` — `is_active` uses fragile string contains for
   `core.hooksPath` detection.
10. `no_bare_pub.rs` — brace-depth tracking miscounts braces in string
    literals and doc comments.
11. `design_doc_source_mismatch.rs` — `source_contains_name` is substring
    match, generates false negatives.
12. `render_design.rs` — multiple `unwrap()` on `fs::read_dir` in
    generation path.

## Which Lints Should Default to OFF

These lints assume project-specific macros that don't exist in all
consumers. They should default to OFF and require explicit opt-in
via `[lints]`:

- `no-manual-id` — assumes `define_id!()`
- `no-adhoc-error-enum` — assumes `define_error!()`
- `no-adhoc-framework` — assumes `define_*!()` macros
- `no-bare-macro-types` — assumes framework-generated types
- `no-pool-access` — assumes pool abstraction
- `no-primitive-key` — assumes `define_id!()` types
- `no-raw-error-outside-primitives` — assumes `define_raw_error!()`
- `no-self-define` — assumes `define_*!()` pattern
- `no-float` — assumes arvo fixed-point types
- `no-bare-pub` — assumes `#[public_api]`/`#[internal_api]` attrs
- `no-bare-result` — assumes `LoimuError`/`Outcome` types
- `no-entry-suffix` — assumes `define_registry!()` convention
- `no-bare-vec` — assumes `Collection<T>`/`DenseColumn<T>` types
- `no-box` — assumes framework type erasure patterns

## Implementation Order

1. Fix critical/important code bugs (items 1-7)
2. Add `default_severity()` to Lint trait, set OFF for project-specific lints
3. Add `[lints]` config parsing with string and table forms
4. Add `finding_kind` to LintError, hierarchical severity override
5. Add `config_keys()` and `configure()` to Lint trait
6. Add consumer custom lint loading
7. Update each parametric lint to declare its config keys
