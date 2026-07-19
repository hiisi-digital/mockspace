//! The `cargo-mock` binary: cargo's external-subcommand entry, so `cargo mock
//! ...` runs it. All logic lives in the library.

fn main() -> std::process::ExitCode {
    cargo_mock::run_cli()
}
