//! Pre-compiled per-document scope filter.
//!
//! Each lint's `ScopeConfig` compiles once at instantiate time into a
//! [`ScopeFilter`] (glob sets pre-built, crate-name patterns indexed,
//! language whitelist materialised). The engine consults the filter on
//! every per-document dispatch to decide whether the lint sees the doc.
//!
//! Filter semantics (all must be true for a doc to be accepted):
//!
//! - Path: `paths` empty OR document path matches `paths` glob set,
//!   AND document path does NOT match `exempt_paths` glob set.
//! - Crate: `crates` empty / contains `"*"` OR crate name matches one
//!   of the patterns, AND crate name does NOT match `exempt_crates`.
//! - Language: `languages` empty OR document language is in the list.
//! - Proc-macro: if `proc_macro_exempt = true`, exempt documents whose
//!   crate appears in `WorkspaceMetadata::proc_macro_crates`.
//! - Category: if any category in `exempt_categories` matches a
//!   `[primitive-introductions]` category declared for the document's
//!   crate, the document is exempt.

use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::config_types::ScopeConfig;
use crate::document::MockspaceDocument;
use crate::errors::{ConfigError, ConfigErrorKind};
use crate::project::MockspaceProject;

/// Pre-compiled scope filter. Cheap to consult per document; expensive
/// only at instantiate time (one globset compile per filter).
#[derive(Debug, Clone)]
pub struct ScopeFilter {
    paths: GlobSet,
    paths_empty: bool,
    exempt_paths: GlobSet,
    /// Pre-compiled crate-name globset. `crates_accepts_all` short-circuits
    /// the common `*` / empty case.
    crates: GlobSet,
    crates_accepts_all: bool,
    exempt_crates: GlobSet,
    exempt_crates_empty: bool,
    languages: Vec<mockspace_core::lint::Language>,
    exempt_categories: Vec<String>,
    proc_macro_exempt: bool,
}

impl ScopeFilter {
    /// Compile globsets and store the rest. Returns `ConfigError` with
    /// `UnparseableGlob` on bad glob syntax in any of paths / exempt_paths
    /// / crates / exempt_crates. Bad crate patterns no longer silently
    /// filter everything out.
    pub fn from_config(lint_name: &str, config: &ScopeConfig) -> Result<Self, ConfigError> {
        let paths = build_globset(lint_name, "paths", &config.paths)?;
        let exempt_paths = build_globset(lint_name, "exempt_paths", &config.exempt_paths)?;

        // Crate patterns accept-all when the list is empty OR any entry
        // is the literal `*`. Otherwise compile into a GlobSet so the
        // per-doc match is a single hash + extension table lookup.
        let crates_accepts_all = config.crates.is_empty() || config.crates.iter().any(|p| p == "*");
        let crates = if crates_accepts_all {
            GlobSet::empty()
        } else {
            build_globset(lint_name, "crates", &config.crates)?
        };
        let exempt_crates_empty = config.exempt_crates.is_empty();
        let exempt_crates = if exempt_crates_empty {
            GlobSet::empty()
        } else {
            build_globset(lint_name, "exempt_crates", &config.exempt_crates)?
        };

        Ok(Self {
            paths,
            paths_empty: config.paths.is_empty(),
            exempt_paths,
            crates,
            crates_accepts_all,
            exempt_crates,
            exempt_crates_empty,
            languages: config.languages.clone(),
            exempt_categories: config.exempt_categories.clone(),
            proc_macro_exempt: config.proc_macro_exempt,
        })
    }

    /// True iff the lint should see this document under the current scope.
    pub fn accepts(&self, doc: &MockspaceDocument, project: &MockspaceProject) -> bool {
        // Path filter.
        let path = doc.path();
        if !self.paths_empty && !self.paths.is_match(path) {
            return false;
        }
        if self.exempt_paths.is_match(path) {
            return false;
        }

        // Crate filter.
        let crate_name = doc.crate_name();
        if !self.crates_accepts_all && !self.crates.is_match(crate_name) {
            return false;
        }
        if !self.exempt_crates_empty && self.exempt_crates.is_match(crate_name) {
            return false;
        }

        // Language filter.
        if !self.languages.is_empty() && !self.languages.contains(&doc.language()) {
            return false;
        }

        // Proc-macro filter.
        if self.proc_macro_exempt && project.workspace().proc_macro_crates.contains(crate_name) {
            return false;
        }

        // Category exempt.
        if !self.exempt_categories.is_empty() {
            if let Some(declared) = project.introduced_categories(crate_name) {
                if self.exempt_categories.iter().any(|c| declared.contains(c)) {
                    return false;
                }
            }
        }

        true
    }
}

fn build_globset(
    lint_name: &str,
    field: &str,
    patterns: &[String],
) -> Result<GlobSet, ConfigError> {
    let mut builder = GlobSetBuilder::new();
    for pat in patterns {
        let glob = Glob::new(pat).map_err(|e| ConfigError {
            lint_name: lint_name.to_string(),
            field_path: format!("scope.{field}"),
            kind: ConfigErrorKind::UnparseableGlob {
                error: format!("{e}"),
            },
            message: format!("invalid glob `{pat}`: {e}"),
            source_location: None,
        })?;
        builder.add(glob);
    }
    builder.build().map_err(|e| ConfigError {
        lint_name: lint_name.to_string(),
        field_path: format!("scope.{field}"),
        kind: ConfigErrorKind::UnparseableGlob {
            error: format!("{e}"),
        },
        message: format!("globset build failed: {e}"),
        source_location: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectBuilder;
    use mockspace_core::lint::{Gate, Language, RunSurface};
    use std::path::PathBuf;

    fn make_doc(path: &str, crate_name: &str, lang: Language) -> MockspaceDocument {
        MockspaceDocument::new(PathBuf::from(path), crate_name, lang, "fn x() {}")
    }

    fn empty_project() -> MockspaceProject {
        ProjectBuilder::new(PathBuf::from("/tmp"), RunSurface::Local, Gate::Commit).build()
    }

    #[test]
    fn empty_scope_accepts_anything() {
        let filter = ScopeFilter::from_config("test", &ScopeConfig::default()).unwrap();
        let doc = make_doc("foo/bar.rs", "anything", Language::Rust);
        assert!(filter.accepts(&doc, &empty_project()));
    }

    #[test]
    fn paths_glob_filters_doc() {
        let cfg = ScopeConfig {
            paths: vec!["**/*.rs".to_string()],
            ..Default::default()
        };
        let filter = ScopeFilter::from_config("test", &cfg).unwrap();
        let project = empty_project();
        assert!(filter.accepts(&make_doc("a/b.rs", "x", Language::Rust), &project));
        assert!(!filter.accepts(&make_doc("a/b.md", "x", Language::Markdown), &project));
    }

    #[test]
    fn exempt_paths_overrides_paths() {
        let cfg = ScopeConfig {
            paths: vec!["**/*.rs".to_string()],
            exempt_paths: vec!["**/ffi/**".to_string()],
            ..Default::default()
        };
        let filter = ScopeFilter::from_config("test", &cfg).unwrap();
        let project = empty_project();
        assert!(filter.accepts(&make_doc("src/lib.rs", "x", Language::Rust), &project));
        assert!(!filter.accepts(&make_doc("src/ffi/c.rs", "x", Language::Rust), &project));
    }

    #[test]
    fn crates_filter_accepts_listed() {
        let cfg = ScopeConfig {
            crates: vec!["arvo".to_string(), "notko".to_string()],
            ..Default::default()
        };
        let filter = ScopeFilter::from_config("test", &cfg).unwrap();
        let project = empty_project();
        assert!(filter.accepts(&make_doc("a.rs", "arvo", Language::Rust), &project));
        assert!(filter.accepts(&make_doc("a.rs", "notko", Language::Rust), &project));
        assert!(!filter.accepts(&make_doc("a.rs", "other", Language::Rust), &project));
    }

    #[test]
    fn star_crate_pattern_accepts_anything() {
        let cfg = ScopeConfig {
            crates: vec!["*".to_string()],
            ..Default::default()
        };
        let filter = ScopeFilter::from_config("test", &cfg).unwrap();
        let project = empty_project();
        assert!(filter.accepts(&make_doc("a.rs", "anything", Language::Rust), &project));
    }

    #[test]
    fn languages_filter_excludes_others() {
        let cfg = ScopeConfig {
            languages: vec![Language::Rust],
            ..Default::default()
        };
        let filter = ScopeFilter::from_config("test", &cfg).unwrap();
        let project = empty_project();
        assert!(filter.accepts(&make_doc("a.rs", "x", Language::Rust), &project));
        assert!(!filter.accepts(&make_doc("a.md", "x", Language::Markdown), &project));
    }

    #[test]
    fn proc_macro_exempt_skips_proc_macro_crates() {
        let cfg = ScopeConfig {
            proc_macro_exempt: true,
            ..Default::default()
        };
        let filter = ScopeFilter::from_config("test", &cfg).unwrap();
        let mut project_set = std::collections::HashSet::new();
        project_set.insert("mac".to_string());
        let project = ProjectBuilder::new(PathBuf::from("/tmp"), RunSurface::Local, Gate::Commit)
            .with_proc_macro_crates(project_set)
            .build();
        assert!(!filter.accepts(&make_doc("a.rs", "mac", Language::Rust), &project));
        assert!(filter.accepts(&make_doc("a.rs", "regular", Language::Rust), &project));
    }

    #[test]
    fn exempt_categories_skips_declaring_crates() {
        let cfg = ScopeConfig {
            exempt_categories: vec!["string-foundation".to_string()],
            ..Default::default()
        };
        let filter = ScopeFilter::from_config("test", &cfg).unwrap();
        let mut builder =
            ProjectBuilder::new(PathBuf::from("/tmp"), RunSurface::Local, Gate::Commit);
        builder.declare_introduced_category("hilavitkutin-str", "string-foundation");
        let project = builder.build();
        assert!(!filter.accepts(
            &make_doc("a.rs", "hilavitkutin-str", Language::Rust),
            &project
        ));
        assert!(filter.accepts(&make_doc("a.rs", "other", Language::Rust), &project));
    }

    #[test]
    fn invalid_crate_pattern_returns_config_error() {
        // Bad crate pattern must surface at instantiate, not silently
        // filter every document out.
        let cfg = ScopeConfig {
            crates: vec!["[unterminated".to_string()],
            ..Default::default()
        };
        let result = ScopeFilter::from_config("test", &cfg);
        match result {
            Err(e) => {
                assert!(matches!(e.kind, ConfigErrorKind::UnparseableGlob { .. }));
                assert_eq!(e.field_path, "scope.crates");
            }
            Ok(_) => panic!("expected UnparseableGlob on bad crate pattern"),
        }
    }

    #[test]
    fn invalid_glob_returns_config_error() {
        let cfg = ScopeConfig {
            paths: vec!["[invalid".to_string()],
            ..Default::default()
        };
        let result = ScopeFilter::from_config("test", &cfg);
        match result {
            Err(e) => {
                assert!(matches!(e.kind, ConfigErrorKind::UnparseableGlob { .. }));
            }
            Ok(_) => panic!("expected UnparseableGlob"),
        }
    }
}
