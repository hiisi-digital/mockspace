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

pub mod bootstrap;
pub mod config;
pub mod model;
pub mod parse;
pub mod graph;
pub mod lint;
pub mod render;
pub mod render_md;
pub mod render_design;
pub mod render_agent;
pub mod dylib_check;

/// Path to the mockspace source directory, captured at compile time.
///
/// When mockspace is a `[build-dependency]`, this resolves to wherever cargo
/// cached the source (git checkout, local path, etc.). The consuming crate's
/// `build.rs` uses this to write the `cargo mock` alias pointing at the right
/// `Cargo.toml`.
pub const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

// Re-export lint rules for convenience
pub use mockspace_lint_rules::{LintMode, Level, Severity, LintError, LintContext};
