//! Catalog entries and the registry.
//!
//! Per schema design memo §3 and §12. Each lint registers exactly one
//! [`CatalogEntry`] via `inventory::submit!{...}`. The engine reads the
//! registered set at startup, dedup-checks by `name`, and dispatches per
//! the entry's `kind` and `mode`.
//!
//! External lint packs (e.g. `mockspace-hilavitkutin-stack-lints`) register
//! their entries the same way: the linker concatenates the distributed
//! slice sections from all crates linked into the host. Adding a lint pack
//! is a build-time `Cargo.toml` change, not a runtime mechanism.

use mockspace_core::lint::{Category, GateSeverity, Impact};

use crate::errors::ConfigError;
use crate::lint::{Lint, LintMode};

/// One lint's catalog declaration.
///
/// Field-by-field rationale in the schema memo. `default_config` and
/// `default_scope` are raw TOML literals parsed at engine init: this keeps
/// the declaration `const`-friendly and avoids a global-allocator
/// dependency at static-init.
///
/// `instantiate` is a plain `fn` pointer (not a closure). Per-pack
/// defaults flow through the `default_config` / `default_scope` strings
/// rather than closure capture. State a lint pack ships by default lives
/// in TOML, not in the catalog entry's value.
#[derive(Clone, Copy)]
pub struct CatalogEntry {
    /// Stable identifier matching the `[lints.<name>]` TOML key.
    pub name: &'static str,

    /// One-line human description.
    pub description: &'static str,

    /// Open-string kind discriminator selecting the primitive impl.
    /// Built-in kinds:
    ///   "token-scan", "ast-node-position-match", "ast-type-position",
    ///   "identifier-pattern", "content-regex", "term-replacement-table",
    ///   "file-metric", "undocumented-item", "cross-doc-symbol",
    ///   "workflow-state", "suppression-meta".
    /// Bespoke lints register their own kind strings.
    pub kind: &'static str,

    /// Raw TOML default config block. Parsed at engine init.
    pub default_config: &'static str,

    /// Raw TOML default scope block.
    pub default_scope: &'static str,

    /// Per-gate default severity.
    pub default_severity: GateSeverity,

    /// Optional default impact for diagnostic display.
    pub default_impact: Option<Impact>,

    /// Optional default category for diagnostic display.
    pub default_category: Option<Category>,

    /// Optional URL into rendered docs; populates `Finding::rule_id`.
    pub doc_url: Option<&'static str>,

    pub mode: LintMode,

    pub staging_aware: bool,

    /// Whether this lint is skipped under `RunSurface::Editor` by default.
    /// `true` for `TwoPhaseProject` / `ProjectScoped` modes (single-buffer
    /// LSP cannot supply a full project cheaply). `false` for `PerDocument`
    /// (the lint runs on the currently-edited buffer with commit-gate
    /// severities). Consumers may override per-lint via TOML.
    pub editor_skip: bool,

    /// Constructor. Validates the merged TOML and produces the boxed lint.
    /// Receives `(merged_config_table, merged_scope_table)`. State a lint
    /// pack supplies via static defaults flows through the TOML, not via
    /// closure capture (see module doc).
    pub instantiate: fn(&toml::Table, &toml::Table) -> Result<Box<dyn Lint>, ConfigError>,

    /// Finding kinds this lint may emit. Drives per-finding-kind severity
    /// validation. Empty slice means "one anonymous kind".
    pub finding_kinds: &'static [&'static str],
}

impl std::fmt::Debug for CatalogEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CatalogEntry")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("mode", &self.mode)
            .field("staging_aware", &self.staging_aware)
            .field("editor_skip", &self.editor_skip)
            .finish()
    }
}

inventory::collect!(CatalogEntry);

/// Return every registered catalog entry across linked crates.
pub fn catalog_entries() -> Vec<&'static CatalogEntry> {
    inventory::iter::<CatalogEntry>().collect()
}

/// Look up a registered entry by name. Returns `None` for absent names.
pub fn find_entry(name: &str) -> Option<&'static CatalogEntry> {
    inventory::iter::<CatalogEntry>().find(|e| e.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_accessible() {
        // No entries are registered today; the lint packs register via
        // inventory::submit! after Phase 2D-4 builtins land.
        let entries = catalog_entries();
        // No panic, no error; the iter is at least walkable.
        let _ = entries.len();
    }

    #[test]
    fn find_entry_returns_none_for_unknown() {
        assert!(find_entry("absolutely-does-not-exist-zzzz").is_none());
    }
}
