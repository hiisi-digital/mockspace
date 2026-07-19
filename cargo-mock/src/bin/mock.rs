//! The `mock` binary: the short, direct entry (bypasses the `cargo` prefix).
//! All logic lives in the library.

fn main() -> std::process::ExitCode {
    cargo_mock::run_cli()
}
