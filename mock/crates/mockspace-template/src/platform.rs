//! Platform abstraction for agent-tool output differences.

use std::path::{Path, PathBuf};

/// Declarative hook description passed to `Platform::settings_file()`.
#[derive(Debug, Clone)]
pub struct HookDecl {
    /// Hook event name (e.g. "PreToolUse").
    pub event: String,
    /// Tool matcher pattern (e.g. "Bash", "Write|Edit", or ".*").
    pub matcher: String,
    /// Command line to execute when the hook fires.
    pub command: String,
}

/// Abstracts the per-platform differences in output path, frontmatter,
/// hook-helper shell snippets, and settings file emission.
pub trait Platform {
    /// Short identifier ("claude", "copilot", ...).
    fn name(&self) -> &'static str;

    /// Map a logical template name (e.g. "rules/foo") to the platform's
    /// concrete output path under `repo_root`.
    fn output_path(&self, repo_root: &Path, logical_name: &str) -> PathBuf;

    /// Emit platform-specific frontmatter for a rule's applicability globs.
    fn frontmatter(&self, applies_to: &[&str]) -> String;

    /// Emit the platform-specific hook helper snippet (substituted into
    /// templates at the `{{ hook_helpers }}` variable).
    fn hook_helpers(&self, repo_root: &Path) -> String;

    /// Emit the platform's settings file (if any) for the given hook list.
    /// Returns `Some((path, contents))` to write, or `None` when the
    /// platform does not use a settings file.
    fn settings_file(
        &self,
        repo_root: &Path,
        hooks: &[HookDecl],
    ) -> Option<(PathBuf, String)>;
}
