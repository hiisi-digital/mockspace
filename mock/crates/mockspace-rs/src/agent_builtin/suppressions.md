# Suppression directives

Five directives, comment-canonical, parsed the same way in every language.
**Lint packs ship new lint names and categories, never a sixth directive**; that
takes a schema version bump.

## lint:allow

Suppresses one lint at one site. Applies to the item immediately following.

```rust
// lint:allow(no-bare-string, reason = "ABI boundary; FFI shim", tracked = "#42")
```

`lint_name` required. `reason` and `tracked` are required by project policy and
optional to the parser, so a missing one is a finding rather than a parse error.

## lint:scope-add

Extends a lint's scope along one axis for the items below, at module or file
boundary.

```rust
// lint:scope-add(no-bare-vec, axis = "crates", value = "loimu-bench")
```

Axis is one of `paths`, `exempt_paths`, `crates`, `exempt_crates`, `languages`,
`exempt_categories`. **Bounded to the `ScopeConfig` fields**; a pack cannot
invent one.

## lint:defer

A known violation with a task that closes it. **Not `allow` with nicer wording**:
a defer expires when its task closes, an allow accumulates as a policy question.

```rust
// lint:defer(no-bare-numeric, until = "#118", reason = "post-Round-D type debt")
```

`SuppressionMetaLint`'s `forbid_expired` flags a defer whose anchor has closed.

## lint:file-disable

Disables a lint for the whole file. **First non-blank line.** Same `reason` and
`tracked` shape as `allow`. A disable rather than a scope extension.

```rust
// lint:file-disable(no-bare-vec, reason = "generated code", tracked = "#0")
```

## lint:prop

A per-site property a lint reads. The framework interprets neither name nor
value; a lint declares what it reads via `Lint::declared_props` and queries the
resolved `PropMap`.

```rust
// lint:prop(audited)                     presence, true
// lint:prop(rate-limit = 100)            integer
// lint:prop(category = "experimental")   string
// lint:prop(audited, reason = "see #42")
```

Types are bool, i64, string. No list: write several and they accumulate.

## Where they attach

- **To the next item** (`allow`, `defer`, `prop`): function, type, impl block.
  One at end-of-file or between items attaches to nothing and warns.
- **To the file or module** (`scope-add`, `file-disable`): before any item.
- Props do not stack with other directives on one item. Each takes effect once.

## Rust attribute aliases

| Comment | Attribute |
|---|---|
| `lint:allow` | `#[lint::allow(...)]` |
| `lint:scope-add` | `#[lint::scope_add(...)]` |
| `lint:defer` | `#[lint::defer(...)]` |
| `lint:prop` | `#[lint::prop(...)]` |
| `lint:file-disable` | comment only |

Both forms parse to the same `Directive`. `#![lint::file_disable(...)]` is
reserved and unwired. Neither form is preferred.

## What a directive cannot do

- Reorder lint priority. That is TOML config and the cascade.
- Add a lint name. That is the pack's catalog.
- Add a severity. Fixed at `info` / `warn` / `error`.
- Disable a lint workspace-wide. That is `mockspace.toml`.
