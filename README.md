# `mockspace`

[![Crates.io](https://img.shields.io/crates/v/cargo-mock)](https://crates.io/crates/cargo-mock) [![docs.rs](https://img.shields.io/docsrs/cargo-mock)](https://docs.rs/cargo-mock)

> A design-first workflow engine for Rust projects, with the design as the thing that counts and the source following from it.

`mockspace` lives in a `mock/` directory next to your code. Design work goes through rounds, from a topic to a locked plan for the docs, then a locked plan for the source, then an archive, and the transitions are subcommands that rename files and check the invariants so nobody edits a suffix by hand. Templates under `mock/` turn into a generated `docs/` tree and a crate graph, and a lint pipeline runs at commit, build and push, with a severity per gate.

It has no say in what you build, only that what you decided got written down and locked before the code, so it fits multi-crate workspaces where the decisions matter. On a small project the ceremony will cost more than it gives, and for linting plain source there are better tools than this one.

It is pre-1.0 and the api still moves, so don't lean on it for anything that can't take a change.

## Usage

Install the launcher once per machine, and activate the hooks once per clone:

```bash
cargo install cargo-mock
cargo mock activate
```

Each repository pins the engine it runs in its `mockspace.toml`:

```toml
mockspace_version = "0.0.2"
```

The engine isn't on crates.io, so a version resolves against the matching git tag. `mockspace_branch = "dev"` follows development and moves under you by design, and `mockspace_rev` holds an exact commit. The launcher builds the pinned engine once into a shared per-version cache and runs it, so every repo on the same pin shares the build.

The commands you reach for most:

| Command | What it does |
|---|---|
| `cargo mock` | check, parse, lint and generate |
| `cargo mock lock` | move a round from `DOC` to `DRAFT`, or from `IMPL` to `CLOSED` |
| `cargo mock close` | archive a closed round |
| `cargo mock test` | run the tests of every tree mockspace owns |
| `cargo mock data-refs` | fetch, update, commit and check the files kept on refs of their own |
| `cargo mock migrate <name>` | move the tree to a new shape, one named migration at a time |
| `cargo mock tools` | list every subcommand and project tool |
| `cargo mock lints` | list every lint with its severity here |

Files of `mock/` that should not ride along with every branch, such as templates or research notes, can live on a git ref of their own. `cargo mock migrate directory-to-ref` moves a directory there, and `--dry-run` shows every path it would touch before it does anything. A migration is always named, checks that the tree is the one it was written for, and refuses anything else.

A fresh clone of a repository that keeps files this way has none of them yet, so run `cargo mock data-refs instantiate` once after cloning; until then most commands stop and say so. After that, `cargo mock data-refs update` brings in what others pushed, `cargo mock data-refs commit` sends your changes, and `cargo mock data-refs status` says where each ref stands. Before the engine writes over a file it keeps a copy in your cache for a day, and a local backup ref keeps the files through git's own checkouts, merges and resets; the usage guide lists what that covers and what it does not.

The [usage guide](https://github.com/hiisi-digital/mockspace/blob/dev/docs/USAGE_GUIDE.md) has the rest, including configuration, lint authoring and the template structure.

## License

> The project is licensed under the **Mozilla Public License 2.0**.

`SPDX-License-Identifier: MPL-2.0`

> You can check out the full license [here](https://github.com/hiisi-digital/mockspace/blob/main/LICENSE)
