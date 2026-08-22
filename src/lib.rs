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

pub mod agent_mode;
pub mod autofix;
pub mod bench;
pub mod bench_gen;
pub mod bench_docs;
pub mod bootstrap;
pub mod config;
pub mod custom_lints;
pub mod deny;
pub mod design_round;
pub mod document;
pub mod dylib_check;
mod entry;
pub mod graph;
pub mod lint;
pub mod model;
pub mod panel;
pub mod parse;
pub mod pdf;
pub mod registry;
pub mod render;
pub mod render_agent;
pub mod render_design;
pub mod render_md;
pub mod tool_catalogue;

/// Path to the mockspace source directory, captured at compile time.
///
/// Resolves to wherever cargo placed the source: a git checkout, a local
/// path, or the installed launcher's own build directory.
pub const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

// Re-export lint rules for convenience
/// Entry point: parses CLI args and runs the mockspace pipeline.
///
/// Called by mockspace's own `main.rs` and by the `cargo-mock` launcher,
/// which is the sole entry now that the generated proxy crate is gone.
pub use entry::run;
/// Entry point with consumer-provided custom lints.
///
/// Used when the repo defines custom lints under `{mock_dir}/lints/` or
/// declares external packs under `[lint-crates]`.
pub use entry::run_with_custom_lints;
pub use mockspace_lint_rules::{
    AgentMode,
    CrateLint,
    Invocation,
    Level,
    Lint,
    LintPack,
    MessageContext,
    MessageDomain,
    MessageLint,
    RepoContext,
    RepoLint,
    WorkspaceLint,
    LintConfig,
    LintContext,
    LintError,
    LintMode,
    Severity,
};
/// The tool contract, re-exported whole rather than item by item.
///
/// A tool crate spells this `mockspace::tool::Tool`, because the generated
/// cdylib renames `mockspace-lint-rules` to `mockspace` so consumer source
/// reads the same whether it is compiled into the engine or dlopened beside
/// it. Re-exporting the module here keeps that one spelling true from both
/// sides.
pub use mockspace_lint_rules::tool;
