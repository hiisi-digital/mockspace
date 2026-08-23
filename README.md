# `mockspace`

<div align="center" style="text-align: center;">

[![GitHub Stars](https://img.shields.io/github/stars/hiisi-digital/mockspace.svg)](https://github.com/hiisi-digital/mockspace/stargazers)
[![Crates.io](https://img.shields.io/crates/v/mockspace)](https://crates.io/crates/mockspace)
[![docs.rs](https://img.shields.io/docsrs/mockspace)](https://docs.rs/mockspace)
[![GitHub Issues](https://img.shields.io/github/issues/hiisi-digital/mockspace.svg)](https://github.com/hiisi-digital/mockspace/issues)
![License](https://img.shields.io/github/license/hiisi-digital/mockspace?color=%23009689)

> A design-first workflow engine for Rust projects. It has no opinion about what you build, and a fairly strong one about how the design gets there.

</div>

## What it is

`mockspace` lives in a `mock/` directory next to your code. It walks design work through a state machine, from exploring a topic, to a locked plan for the docs, to applying that plan to source, to archiving the whole thing. Templates under `mock/` become a generated documentation tree and a crate dependency graph, and a lint pipeline runs at three gates, commit, build and push, with a severity per gate.

Underneath it there are a few claims about how design work should behave, and they are worth stating plainly because they are what you are agreeing to. Source is never the authority. The design template is the artifact that counts, source follows from it, and the lints keep the two honest with each other. Transitions happen through subcommands that rename files and check invariants, never by editing a filename suffix yourself, because a state machine whose invariants are advisory stops being one fairly quickly.

None of that says anything about what you build. Crate naming, numeric discipline, which frameworks you pull in, the lints particular to your domain: all yours. `mockspace` only cares that whatever you decided got written down, reviewed, locked and is reproducible afterwards.

**What it costs is worth being upfront about.** The ceremony is real, and on a small project it will feel like more than it gives you. You write the plan before the code, and you cannot skip that when you are in a hurry, which is the point and is also occasionally infuriating.

## Status

Pre-1.0, and it moves. The tool is in daily use across a stack of consumer crates and grows whatever those crates turn out to need, so the api hasn't settled and breaking changes should be expected. Pin to a git revision when you consume it.

I'd caution against adopting this just yet for anything you cannot afford to have shift under you. Two reworks are in flight and either will move things: the plugin ABI v1 redesign, and a successor model for the design-round workflow (see `docs/research/`).

## When mockspace is a good fit

- Multi-crate Rust workspaces where the design decisions are consequential enough that you want a paper trail of them.
- Projects that would rather have invariants enforced mechanically than socially, because the social version keeps not working.
- Workspaces where keeping generated documentation in step with source by hand has stopped being sustainable.

Where it isn't:

- Single-file scripts, one-crate demos and quick prototypes. The ceremony costs more than it returns and you'll resent it.
- Projects where the design genuinely does live in the code, and writing it down twice would be writing it down twice.

If what you actually want is a source linter, this is probably not it either. `mockspace` will lint your source, but there are more established, more proven and better-documented tools for that in whatever language you're in, and you should use one of those. What this is for is the part above the source: the design, the plan, and whether the two still agree with each other a month later.

## Contents

| Component | Purpose |
|---|---|
| `mockspace` (binary + library) | The runtime: bootstrap, build integration, generation pipeline, lint runner, transition subcommands. |
| `mockspace-lint-rules` | The `Lint` and `Tool` traits, plus the built-in rules: general quality checks, and the design-round lints that enforce the phase transitions. The lints in a repo's own `mock/lints/` compose against it. |
| `mockspace-bench-core` | The bench framework: `Routine` trait, hardware-counter timing, and an FFI bridge so variants compiled separately can be compared in one run. The harness adds workload and cache control, validation, Pareto analysis, history, perf and disassembly sensors, and findings reporting. |
| `mock bench` | The command surface over that framework. `init` scaffolds a `mock/benches/` tree that is configuration and nothing else, `run` generates the driver binary from `bench.toml` and runs the configured benches, `test` runs `cargo test` across every crate the tree owns, and `report` regenerates the findings. A hand-written driver crate stays available as the escape hatch. |
| `cargo-mock` / `mock` launcher | The sole entrypoint, installed as two binaries from one source. Resolves the engine version a repo pins in its `mockspace.toml`, builds that engine once into a shared per-version cache, and execs it; `mock locate` answers where a repo keeps its mockspace. |

## Installation

Install the launcher once per machine:

```bash
cargo install --git https://github.com/hiisi-digital/mockspace.git cargo-mock
```

That installs `cargo-mock` and `mock`, the same tool under cargo's subcommand
convention and a short direct form. There is no `build.rs` bootstrap, no
`.cargo` alias, and no proxy crate: the launcher is the sole entry.

Each repository pins the engine it runs in its root `mockspace.toml`:

```toml
# a released version once one is tagged, or a branch, or an exact commit
# mockspace_version = "0.0.0-d01"
mockspace_branch = "dev"
```

No release has been tagged yet, so `mockspace_version` has nothing to resolve
to. Track the branch to follow development, or set `mockspace_rev` to an exact
commit to hold the engine still. The commented pin shows the shape a tagged
release takes.

The launcher builds the pinned engine once into a shared per-version cache and
execs it, so every repo on the same pin shares one build and the working
directory never matters.

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
cargo mock test                  # cargo test across every tree mockspace owns
cargo mock tools                 # every subcommand and project tool, with usage
mock locate                      # where this repo keeps its mockspace, shell-assignable
```

For the full subcommand surface, configuration reference, lint authoring, and template structure, see `docs/USAGE_GUIDE.md`.

## Design rounds

The unit of design work in mockspace is a *round*. A round walks five phases:

| Phase | Meaning |
|---|---|
| `TOPIC` | Exploration. Topic and research material is committed; no changelist exists yet. |
| `DOC` | A doc changelist is active. Templates are the only thing changing. |
| `DRAFT` | The doc changelist is locked. The src changelist has not been authored. |
| `IMPL` | The src changelist is active. Source under the consumer crates is being written to match what the docs already promised. |
| `CLOSED` | Both changelists locked. Round is ready to archive. |

Phases are detected from filename suffixes inside `mock/design_rounds/`. Transitions happen through `cargo mock lock` / `unlock` / `deprecate` / `close` / `archive`, never through manual renames. Each one validates its invariants, commits the rename itself when asked with `--auto-commit`, and leaves a stable history anchor.

A successor model for this workflow is in development. It makes tasks, phases, manifests and epochs concepts in their own right, and demotes the branch to ambient context. See `docs/research/TASKS_BRANCHES_PHASES_EPOCHS_DESIGN.md`.

## Lint pipeline

Three sources contribute rules:

- **Built-in lints** in `mockspace-lint-rules`. Universal quality checks (file size, undocumented types, no empty crate) and the design-round lints that hold the phases above to their transitions.
- **Consumer lints** in `mock/lints/<name>.rs`. Each file exports a Rust function returning a lint trait object. The engine discovers them and compiles them, together with any imported packs, into one lint library it loads at run time.
- **Config-driven rules** under `[lints.<rule-name>]` in the same `mockspace.toml`. The `forbidden-imports` rule covers the common case of "this scope must not import these paths".

A check that cannot run at a gate is a tool instead, and tools have a section of their own below.

Each lint declares a severity per gate. The same lint can be `info` at commit, `warn` at build, and `error` at push. The four design-round lints (`changelist-required`, `changelist-doc-gate`, `changelist-lock`, `changelist-immutability`) are always on and non-negotiable.

## Tools

A lint runs at a gate and answers a question nobody asked. Some checks cannot: they need a question from the person running them, or they answer with a ranking rather than a verdict. Those are **tools**, invoked as `mock <name>`.

A tool is a crate under `mock/tools/<name>/`, and the directory name is the subcommand. It is compiled into the same library the consumer lints are, so a tool may declare its own dependencies and may ship a lint alongside itself.

```rust
// mock/tools/phrase-search/src/lib.rs
use mockspace::tool::{ArgSpec, NotALint, Tool, ToolContext, ToolReport};

pub struct PhraseSearch;

impl Tool for PhraseSearch {
    fn name(&self) -> &'static str { "phrase-search" }
    fn description(&self) -> &'static str { "find a phrase across wrapped lines" }
    fn not_a_lint(&self) -> NotALint { NotALint::TakesAQuestion }
    fn args(&self) -> &[ArgSpec] {
        &[ArgSpec { name: "phrase", required: true, description: "what to look for" }]
    }
    fn run(&self, ctx: &ToolContext<'_>) -> ToolReport {
        ToolReport::reported(format!("searching for {}", ctx.args.join(" ")), 1)
    }
}

mockspace::lint_pack! { tools: [PhraseSearch] }
```

`not_a_lint` has no default and takes one of two values, because the question it asks is the one that keeps the gate populated:

- `TakesAQuestion`, when a required argument comes from the person. A gate has nobody to ask.
- `NoFailingCase`, when the answer is the output and no threshold separates pass from fail.

Neither is a matter of taste and both are checked. A tool claiming to take a question and declaring no required argument is refused; one claiming no failing case and returning a finding that blocks a gate is reported. **Being slow, or needing git history, are not reasons.** A repo lint is handed the repository root and may run git itself, so a check with those properties is a lint.

A tool returns one of three outcomes rather than a list of findings. `Clean` carries what it examined, because a clean verdict over nothing is not a pass. `Findings` carries `LintError`s, the same type a lint produces, so a tool that turns out to be gateable becomes a lint without rewriting them. `Inconclusive` says the run establishes nothing, and it fails: a check that silently did not run is worse than no check, since both print the same green.

## Generated documentation

A `cargo mock` run with no subcommand regenerates the `docs/` tree from templates under `mock/`. That includes a top-level `DESIGN.md`, a `STRUCTURE.md` plus Graphviz `.dot` and rendered `.png` / `.svg` files from the per-crate item index, deep dives, and per-crate overviews. The generator owns the top level of `docs/`, and a file sitting there that the run did not produce is swept. Hand-written documents belong in a subdirectory, which the sweep does not descend into, or outside `docs/` entirely.

## Git hooks

`mockspace` never touches `.git/hooks/`. Durable hooks live in the user config home and delegate to a per-repo validator under the mock directory's `target/hooks/`, sourcing the user's existing `.git/hooks/<name>` first and then running mockspace's own validation. The launcher wires them on first contact and the engine keeps them current; deactivation (`cargo mock deactivate`) is explicit and reversible, and git then falls back to whatever the user already had.

## Optional: AI assistant integration

If `mock/agent/` is populated with templates, mockspace renders coordinated configuration from those templates for common AI coding assistants (Claude Code, GitHub Copilot CLI, others as templates require). One source produces semantically equivalent output for each platform. A small set of builtin skills renders alongside: sketching and benchmarking discipline on by default, an interactive design-talk flow opt-in, each declinable per repository in `mock/agent/config.toml`. This is a configuration surface, not a feature; the tool's identity is what it does for human developers.

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
