# Reference syntax

A reference is `{{ root::step::step }}`, a path in the letkajenkka standard. The `reference-syntax` rule has the syntax and this project's roots and namespaces. This is the rest.

## Fields that hold a row

- A field's `type` is a builtin (`string`, `string[]`, `integer`, `boolean`, `ref`, `ref[]`, `date`, `time`, `datetime`, `span`) or a namespace name: `type = "slot"` holds one slug of `slot`, `"slot[]"` several. The value is a bare slug (`"display"`), never `"slot::display"`.
- Time types are quoted strings in one form each: `date` `2026-10-28`; `time` `18:49`, seconds and a fraction of one optional; `datetime` date and time joined by `T` or a space, optional offset; `span` an ISO 8601 duration (`P3D`, `PT1H30M`). A bare TOML date is `unquoted-time`. No namespace may take these names.
- Reported: a slug naming no row, a type naming neither builtin nor namespace, a target declaring `value_field`.

## One relation field, typed `ref[]`

```toml
related = ["tenet::engine_time", "bevy::time_step", "research::bevy_reference::notes::time"]
```

- Two segments (`<ns>::<slug>`) is a link; three or more a citation. A link naming no row is `unknown-row-reference` (gates the commit or push of the file or template holding it, a warning otherwise, so a row can retire first); a link in the spelling before the standard is scoped the same way until `references-to-letkajenkka` rewrites it. A bad slug is `malformed-row-reference`, and it, any other text the standard refuses and every other finding gate every run.
- Never a field per target namespace. A namespace-typed field is for a value that is by nature one namespace's.
- Write a link once; `{{ refsto(tenet::engine_time) }}` and `cargo mock links tenet::engine_time` derive the other end. Never add the inverse.

## Field values

- `values` is a closed set in the field's type; `min` / `max` bound an `integer`.
- Refused at the declaration: a bound on a non-integer or `min` above `max` (`misplaced-field-bound`); a listed value the field cannot hold, e.g. `+5`, `05`, out of range, wrong time form (`unfit-field-value`).

## The three functions

- `{{ pathof(x) }}`: where x is declared, the file to open to change it.
- `{{ sourcesof(x) }}`: what x rests on, in precedence order.
- `{{ refsto(x) }}`: what points at x, derived. Empty is the finding.

A postfix chain narrows a result, left to right: `{{ pathof(crates::store).dir() }}`. Path methods `dir`, `filename`, `stem`, `ext`; list methods `first`, `last`, `count`. An unknown method is reported.

## Citations

- `root::path::anchor`; the anchor is a heading (`the_four_lanes`), a line (`12`) or a span (`12..20`). Prefer the heading: a line number shifts silently. Line numbers only in a frozen root.
- Any depth. Every step is lowercase letters, digits and `_`, starting with a letter; the host folds its names into that: `mock::design::12` finds `DESIGN.md.tmpl`, `crates::my_crate` the directory `my-crate`, `the_four_lanes` the heading a forge anchors as `the-four-lanes`.
- Never an extension, uppercase letter, hyphen or dot; the old spelling is refused with a pointer to `cargo mock migrate references-to-letkajenkka`. Two files folding to one name is an error; a name that cannot fold cannot be cited.
- A namespace is addressed by its name (`law::keys`); `reg::law::keys` still resolves.

## Rows

- Identified by snake_case slug, never a number.
- Identity is `namespace::slug`, never the file: any `*.toml` under `registry/` make one registry. Never write a rule or check that depends on the file. The same key path in two files is an error naming both.

## Merging registry files

The `mockspace-registry` driver (registered by `cargo mock`; `cargo mock register-drivers --global` for the user) runs `cargo mock merge-registry` on `{mock_dir}/registry/*.toml`, keyed by `[[namespace]]` and `id`:

- additions on either side kept, in each side's order; identical rows once;
- a one-sided change or deletion taken;
- a row changed both ways, deleted against a change, or added twice differently conflicts, with markers around that row only; a head or plain `[table]` both sides changed conflicts too. Resolve keeping both intents.
- A file not readable as rows merges as text. The same `id` added in two files is caught by the duplicate check after.

## Adding to a list field

`cargo mock registry-append <namespace>::<id> <field> <value>` adds one string to a `string[]` field, silently, never twice; use it rather than a text edit. A delegate at `.mockspace/hooks/registry-append` (repo root or above) may take it over: exit 0 done, 3 leave it to mockspace, else failure; `--local` skips the delegate.
