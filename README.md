# `mockspace`

<div align="center" style="text-align: center;">

[![GitHub Stars](https://img.shields.io/github/stars/hiisi-digital/mockspace.svg)](https://github.com/hiisi-digital/mockspace/stargazers)
[![Crates.io](https://img.shields.io/crates/v/mockspace)](https://crates.io/crates/mockspace)
[![docs.rs](https://img.shields.io/docsrs/mockspace)](https://docs.rs/mockspace)
[![GitHub Issues](https://img.shields.io/github/issues/hiisi-digital/mockspace.svg)](https://github.com/hiisi-digital/mockspace/issues)
![License](https://img.shields.io/github/license/hiisi-digital/mockspace?color=%23009689)

> A design-first workflow engine for Rust projects. Opinion-free about what you build; opinionated about how the design converges.

</div>

## What it is

`mockspace` lives in a `mock/` directory next to your code and enforces a file-based state machine that walks design work through topic exploration, locked doc-side planning, source application, and archival. Templates under `mock/` become a generated documentation tree and a crate dependency graph. A configurable lint pipeline runs on every build with per-gate severity (commit, build, push), so a rule can be a hint at commit time and a hard error before push.

The framework is built around a few claims about how design work should behave. Source code is never the authority. The design template is the canonical artifact, source is regenerable from it, and the lint pack ensures the two stay in agreement. Workflow transitions move through subcommands that rename files atomically and validate invariants, never through hand-edited filename suffixes. State machines rot when invariants are advisory; mockspace lints them.

None of that is opinionated about what you build. Crate naming, numeric discipline, framework choice, lint rules specific to your domain are entirely yours. Mockspace only cares that whatever you decide is captured, reviewed, locked, and reproducible.

## Status

Pre-1.0. The tool is in active use across a stack of consumer crates and evolves alongside them. Expect breaking changes; pin to a git revision when consuming. The plugin ABI v1 redesign and the workflow upgrade to first-class tasks/phases/manifests/epochs are in flight (see `docs/research/`).

## Contents

| Component | Purpose |
|---|---|
| `mockspace` (binary + library) | The runtime: bootstrap, build integration, generation pipeline, lint runner, transition subcommands. |
| `mockspace-lint-rules` | Sibling crate of universal-quality lints and design-round state-machine lints. Consumers compose against this from `mock/lints/`. |
| `mockspace-bench-core` | Canonical bench framework: `Routine` trait, hardware-counter timing, FFI bridge for variant-comparison benches. v2 ships harness orchestration (workload/cache modules, validation, Pareto analysis, history + perf + disasm sensors, findings + meta-level reporting). |
| `cargo mock` alias | Cargo subcommand entrypoint installed by bootstrap; the surface most users interact with. |
| `mock bench init / run / report` | Scaffolds `mock/benches/` in the consumer with a starter `Routine`, runs the configured benches, and emits findings or meta-level reports. |

## Design rounds

The unit of design work in mockspace is a *round*. A round walks five phases:

| Phase | Meaning |
|---|---|
| `TOPIC` | Exploration. Topic and research material is committed; no changelist exists yet. |
| `DOC` | A doc changelist is active. Templates are the only thing changing. |
| `DRAFT` | The doc changelist is locked. The src changelist has not been authored. |
| `IMPL` | The src changelist is active. Source under the consumer crates is being written to match what the docs already promised. |
| `CLOSED` | Both changelists locked. Round is ready to archive. |

Phases are detected from filename suffixes inside `mock/design_rounds/`. Transitions happen through `cargo mock lock` / `unlock` / `deprecate` / `close`, never through manual renames. Each transition validates invariants, can commit surgically when asked, and is recorded as a stable history anchor.

A successor model for this workflow (tasks, phases, manifests, epochs as first-class concepts; branch demoted to ambient context) is in development. See `docs/research/TASKS_BRANCHES_PHASES_EPOCHS_DESIGN.md`.

## Lint pipeline

Three sources contribute rules:

- **Built-in lints** in `mockspace-lint-rules`. Universal quality checks (file size, undocumented types, no empty crate) and the design-round state-machine lints that enforce the phase transitions above.
- **Consumer lints** in `mock/lints/<name>.rs`. Each file exports a Rust function returning a lint trait object. Bootstrap discovers them and wires them into the generated proxy crate.
- **Config-driven rules** under `[lints.<rule-name>]` in `mock/mockspace.toml`. The `forbidden-imports` rule covers the common case of "this scope must not import these paths".

Each lint declares a severity per gate. The same lint can be `info` at commit, `warn` at build, and `error` at push. The four design-round lints (`changelist-required`, `changelist-doc-gate`, `changelist-lock`, `changelist-immutability`) are always on and non-negotiable.

## Generated documentation

Every `cargo mock` run regenerates the `docs/` tree from templates under `mock/`. That includes a top-level `DESIGN.md`, a `STRUCTURE.md` plus Graphviz `.dot` and rendered `.png` / `.svg` files from the per-crate item index, deep-dives, and per-crate overviews. Anything in `docs/` is overwritten by the generator; user-authored docs belong elsewhere.

## Git hooks

`mockspace` never touches `.git/hooks/`. It generates parallel hooks under `mock/target/hooks/` that source the user's existing `.git/hooks/<name>` first and then run mockspace's own validation. Activation is explicit (`cargo mock activate`) and reversible (`cargo mock deactivate`); when deactivated, git falls back to whatever the user already had.

## Installation

Add a one-line `build.rs` to any crate inside your mock workspace:

```rust
fn main() { mockspace::bootstrap_from_buildscript(); }
```

and the matching dependency:

```toml
[build-dependencies]
mockspace = { git = "https://github.com/hiisi-digital/mockspace.git" }
```

On the first `cargo check` inside `mock/`, bootstrap writes the `cargo mock` alias into `.cargo/config.toml`, generates a proxy crate under `target/mockspace-proxy/`, and materialises hook scripts under `mock/target/hooks/`. Subsequent builds re-validate the bootstrap and skip when nothing has drifted.

Activate the hooks once per clone:

```bash
cargo mock activate
```

## Usage

```bash
cargo mock                       # default: check + parse + lint + generate
cargo mock --lint-only --commit  # lint at commit-gate severity, skip generation
cargo mock --lint-only --strict  # lint at push-gate severity (used by pre-push hook)
cargo mock lock                  # transition: DOC -> DRAFT, or IMPL -> CLOSED
cargo mock close                 # archive a CLOSED round
```

For the full subcommand surface, configuration reference, lint authoring, and template structure, see `docs/USAGE_GUIDE.md`.

## When mockspace is a good fit

- Multi-crate Rust workspaces where design decisions are consequential and need a paper trail.
- Projects that want invariants enforced mechanically rather than socially.
- Workspaces where keeping a generated documentation tree in sync with source is more sustainable than maintaining both by hand.

When it is not a fit:

- Single-file scripts, one-crate demos, or quick prototypes.
- Projects where design evolves purely through code without a docs dimension.

## Optional: AI assistant integration

If `mock/agent/` is populated with templates, mockspace renders coordinated configuration from those templates for common AI coding-assistant integrations (Claude Code, GitHub Copilot CLI, others as templates require). One source produces semantically equivalent output for each platform. This is a configuration surface, not a feature; the framework's identity is what it does for human developers.

If you choose to use this surface:

> ## A note on coding agents
>
> We do not recommend using coding agents with mockspace-managed codebases. Mockspace exists because design discipline is hard to enforce mechanically, and that discipline does not transfer cleanly to a system that has been trained primarily on patterns where source is the authority and design is post-hoc. Models default to writing the source first and treating the docs as documentation; mockspace inverts that. Expect friction.
>
> If you still choose to use a coding agent:
>
> - Be aware of the environmental and social impact of large-scale model inference. Minimise agent use where it is not needed. Be responsible.
> - Only use an agent if you yourself understand the architecture. Do not use an agent because you do not understand; you will waste time and energy, both yours and the planet's.
> - The agent template surface (`mock/agent/*.tmpl`) lets you encode your project's actual rules in one place that emits to every supported assistant. It helps but does not eliminate the problem. You will still need to correct the agent frequently.
>
> The recommendation stands: do this work yourself unless you know what you are doing and why.

## Support

Whether you use this project, have learned something from it, or just like it, please consider supporting it by buying me a coffee, so I can dedicate more time on open-source projects like this :)

<a href="https://buymeacoffee.com/orgrinrt" target="_blank"><img src="https://www.buymeacoffee.com/assets/img/custom_images/orange_img.png" alt="Buy Me A Coffee" style="height: auto !important;width: auto !important;" ></a>

## License

> The project is licensed under the **Mozilla Public License 2.0**.

`SPDX-License-Identifier: MPL-2.0`

> You can check out the full license [here](https://github.com/hiisi-digital/mockspace/blob/dev/LICENSE)
