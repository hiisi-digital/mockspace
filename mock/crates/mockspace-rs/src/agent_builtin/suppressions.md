# Suppression directives

Mockspace ships five directives that source files can carry to
modify how lints apply at specific sites. The directives are
comment-canonical: every language preprocessor parses the same five
verbs from comment syntax. Per-language attribute aliases (Rust
attributes, TS decorators) map to the same internal record.

Lint packs ship new lint names and new categories, but cannot ship
new directive variants. The directive set is bounded to these five;
adding a sixth requires a framework schema version bump.

## The five directives

### lint:allow

Per-site suppression of a specific lint at a single source location.

Comment form:

```rust
// lint:allow(no-bare-string, reason = "ABI boundary; FFI shim", tracked = "#42")
const C_NAME: &str = "...";
```

Rust attribute alias:

```rust
#[lint::allow(no_bare_string, reason = "ABI boundary; FFI shim", tracked = "#42")]
const C_NAME: &str = "...";
```

Required fields: `lint_name`. The `reason` and `tracked` clauses are
required at the project level (per the `lint-allow-requires-task-id`
workspace rule) but parsed as optional at the directive parsing
layer; the meta-lint reports missing fields as findings rather than
parser failures.

### lint:scope-add

At a module or file boundary, extends a lint's scope along one axis
for the contained items. Useful for opt-in patterns where a few
modules need a stricter scope than the workspace default.

Comment form:

```rust
// lint:scope-add(no-bare-vec, axis = "crates", value = "loimu-bench")
```

The axis must be one of the `ScopeAxis` variants:
`paths`, `exempt_paths`, `crates`, `exempt_crates`, `languages`,
`exempt_categories`. Lint packs cannot invent new axes through this
directive; the axis set is bounded to the `ScopeConfig` fields.

### lint:defer

Acknowledges a known violation that will be fixed when the linked
task closes. Semantically distinct from `allow`: defers expire when
the linked task closes, while allows accumulate as a policy
question.

Comment form:

```rust
// lint:defer(no-bare-numeric, until = "#118", reason = "post-Round-D type debt")
```

The `SuppressionMetaLint`'s `forbid_expired` config flags defers
that reference closed tasks; the lint catches the case where a
defer outlives its anchor.

### lint:file-disable

File-level disable for the named lint. Placed at the top of a file.
Distinct from `scope-add` in that it is a disable, not a scope
extension.

Comment form (must be the first non-blank line of the file):

```rust
// lint:file-disable(no-bare-vec, reason = "generated code", tracked = "#0")
```

Requires the same `reason` + `tracked` shape as `allow`.

### lint:prop

Lint-provided per-site property consumed by lints. The framework
does not interpret the prop name or value; lints declare via
`Lint::declared_props` which names they read and query the resolved
`PropMap` for matches.

Comment forms:

```rust
// lint:prop(audited)                          # presence form, true bool
// lint:prop(rate-limit = 100)                 # key-value, integer
// lint:prop(category = "experimental")        # key-value, string
// lint:prop(audited, reason = "see #42")      # presence + reason
```

Value types: bool, integer (i64), string. No list variant; multi-value
props write multiple directives that accumulate in the `PropMap`
naturally.

## Where directives go

All five directives parse from comments anywhere a comment can
appear in the source file. The preprocessor pairs each directive
with the item it precedes (per-site directives) or with the file
itself (`lint:file-disable` only).

`lint:allow` and `lint:defer` apply to the immediately following
item: function definition, type declaration, impl block, etc. A
directive at end-of-file or between items unaffiliates and surfaces
as a warning.

`lint:scope-add` and `lint:file-disable` apply at module or file
scope. They land before any item declaration.

`lint:prop` applies to the next item, same as `lint:allow`. Props
do not stack with other directives on the same item; if both
`lint:allow` and `lint:prop` precede an item, they each take effect
once.

## Rust attribute aliases

The five verbs all have Rust attribute aliases under the `lint::`
prefix:

| Comment form | Attribute form |
|---|---|
| `lint:allow` | `#[lint::allow(...)]` |
| `lint:scope-add` | `#[lint::scope_add(...)]` |
| `lint:defer` | `#[lint::defer(...)]` |
| `lint:file-disable` | comment form only (no attribute alias yet) |
| `lint:prop` | `#[lint::prop(...)]` |

The attribute aliases parse into the same `Directive` enum as the
comment form. `lint:file-disable` currently ships as comment-only;
the inner-attribute alias `#![lint::file_disable(...)]` is reserved
for future wiring. Style preference for the four wired aliases is
per-project; mockspace does not prefer one over the other.

## What directives do NOT cover

- Reordering lint priority. Use TOML config + the cascade.
- Adding new lint names. Lint packs do that via the catalog.
- Defining new severity levels. Severity is a fixed enum
  (`info / warn / error`).
- Disabling lints workspace-wide. Use `mockspace.toml`.
