//! Mockspace: design-round workflow engine for mock workspaces.
//!
//! Provides the complete pipeline for mock-first design workflows:
//! - Rust AST parsing (tree-sitter)
//! - Pluggable lint system with per-gate severity
//! - Doc generation from templates
//! - Agent file generation (Claude + Copilot)
//! - Dependency graph visualization
//! - Dylib module ABI verification
//! - Git hook installation and management
//! - Nuke/restore for reproducibility testing
//! - **Bootstrap**: auto-setup of cargo alias and git hooks via `build.rs`

pub mod bench;
pub mod bootstrap;
pub mod config;
pub mod design_round;
pub mod pdf;
pub mod model;
pub mod parse;
pub mod graph;
pub mod lint;
pub mod render;
pub mod render_md;
pub mod render_design;
pub mod render_agent;
pub mod dylib_check;
mod entry;

/// Path to the mockspace source directory, captured at compile time.
///
/// When mockspace is a `[build-dependency]`, this resolves to wherever cargo
/// cached the source (git checkout, local path, etc.). Used by bootstrap to
/// generate the proxy crate in `target/mockspace-proxy/`.
pub const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

// Re-export lint rules for convenience
pub use mockspace_lint_rules::{LintMode, Level, Severity, LintError, LintContext, Lint, CrossCrateLint, LintConfig};

/// Entry point — parses CLI args and runs the mockspace pipeline.
///
/// Called by both mockspace's own `main.rs` and by the generated
/// `target/mockspace-proxy/` runner crate.
pub use entry::run;

/// Entry point with consumer-provided custom lints.
///
/// Called by proxy crates that define custom lints in `mock/lints/`.
pub use entry::run_with_custom_lints;
