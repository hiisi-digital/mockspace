//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The `cargo-mock` binary: cargo's external-subcommand entry, so `cargo mock
//! ...` runs it. All logic lives in the library.

fn main() -> std::process::ExitCode {
    cargo_mock::run_cli()
}
