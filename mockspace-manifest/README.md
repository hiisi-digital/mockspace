# `mockspace-manifest`

[![Crates.io](https://img.shields.io/crates/v/mockspace-manifest)](https://crates.io/crates/mockspace-manifest) [![docs.rs](https://img.shields.io/docsrs/mockspace-manifest)](https://docs.rs/mockspace-manifest)

> The `mockspace.toml` keys the mockspace launcher reads, and the durable git-hook gate, shared between the launcher and the engine.

This is the part of `mockspace.toml` that the launcher owns, declared as a type, so the launcher and the engine can't disagree about what a key means. It also holds the git-hook gate both of them plant. You only need it as a dependency if you are building on `cargo-mock` or the engine, and for using mockspace itself the [repository readme](https://github.com/hiisi-digital/mockspace) is the place.

## Usage

```bash
cargo add mockspace-manifest
```

## License

> The project is licensed under the **Mozilla Public License 2.0**.

`SPDX-License-Identifier: MPL-2.0`

> You can check out the full license [here](https://github.com/hiisi-digital/mockspace/blob/main/LICENSE)
