# `cargo-mock`

[![Crates.io](https://img.shields.io/crates/v/cargo-mock)](https://crates.io/crates/cargo-mock) [![docs.rs](https://img.shields.io/docsrs/cargo-mock)](https://docs.rs/cargo-mock)

> The launcher for the mockspace design-round workflow engine.

`cargo-mock` is the one entry to `mockspace`. It reads the engine version a repo pins in its `mockspace.toml`, builds that engine once into a shared per-version cache and runs it from wherever you are, so the working directory never matters and every repo on the same pin shares one build. It installs two binaries, `cargo-mock` for `cargo mock` and the shorter `mock`.

## Usage

```bash
cargo install cargo-mock
cargo mock activate
```

The pin goes in the repository's `mockspace.toml`, as `mockspace_version`, `mockspace_branch` or `mockspace_rev`. `mock locate` prints where a repo keeps its mockspace, in a form the shell can assign. What the engine itself does is in the [repository readme](https://github.com/hiisi-digital/mockspace) and the usage guide it links.

## License

> The project is licensed under the **Mozilla Public License 2.0**.

`SPDX-License-Identifier: MPL-2.0`

> You can check out the full license [here](https://github.com/hiisi-digital/mockspace/blob/main/LICENSE)
