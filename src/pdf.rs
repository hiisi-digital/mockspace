use std::path::Path;
use std::process::{Command, ExitCode};

/// Invoke `scripts/pdf.sh` to generate a design documentation PDF.
///
/// `docs_dir` is passed as `--docs-dir` so the script skips auto-detection.
/// Extra CLI args (e.g. `--out`, `--open`, `--title`) are forwarded verbatim.
pub fn cmd_pdf(docs_dir: &Path, repo_root: &Path, extra_args: &[&str]) -> ExitCode {
    let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("pdf.sh");

    if !script.exists() {
        eprintln!("error: pdf.sh not found at {}", script.display());
        return ExitCode::FAILURE;
    }

    let mut cmd = Command::new("bash");
    cmd.arg(&script)
        .arg("--docs-dir")
        .arg(docs_dir)
        .args(extra_args)
        .current_dir(repo_root);

    match cmd.status() {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => {
            if let Some(code) = s.code() {
                eprintln!("error: pdf generation failed (exit {code})");
            }
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("error: failed to run pdf.sh: {e}");
            ExitCode::FAILURE
        }
    }
}
