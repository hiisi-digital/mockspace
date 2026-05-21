//! First-party preset source (#539).
//!
//! Resolves `mockspace::<name>` shorthand against the preset table
//! embedded by `build.rs` at compile time. Per the design memo at
//! `mock/research/202605220500_lint-preset-infrastructure.md`:
//!
//! > first-party (mockspace-shipped) presets live at
//! > `mock://@/export/lint-preset/<name>` and are embedded into the
//! > mockspace binary at build time.
//!
//! External preset hosts (lockfile-pinned `mock://ext/<pkg>/...`)
//! are out of scope for this slice; they land alongside Phase 4 of
//! the v2 spec when the URI scheme + lockfile machinery ships.
//! Until then, `FirstPartyPresetSource::resolve` returns
//! `NotFound` for any host other than `mockspace`.
//!
//! # Lookup cost
//!
//! Each call parses the matching TOML body anew. The set is small
//! (single-digit to low double-digit entries) and the parser runs
//! once per lint instantiation, not per dispatch. If the cost ever
//! shows up in profiles, a `OnceLock<HashMap<&'static str,
//! PresetFile>>` cache slots in transparently behind the same
//! trait surface; the change would be local to this module.

use mockspace_config::{parse_preset_shorthand, PresetFile, PresetRef, PresetResolveError, PresetSource};

include!(concat!(env!("OUT_DIR"), "/embedded_presets.rs"));

/// Host name reserved for first-party presets. The `mockspace::<name>`
/// shorthand at consumer sites resolves to a preset under this host.
pub const FIRST_PARTY_HOST: &str = "mockspace";

/// Source that resolves first-party (`mockspace::*`) presets from the
/// embedded table. External (`<other>::*`) lookups return
/// [`PresetResolveError::NotFound`] until external preset loading
/// lands.
pub struct FirstPartyPresetSource {
    embedded: &'static [(&'static str, &'static str)],
}

impl FirstPartyPresetSource {
    /// Construct from the build.rs-generated `EMBEDDED_PRESET_TOML`
    /// slice.
    pub fn new() -> Self {
        Self {
            embedded: EMBEDDED_PRESET_TOML,
        }
    }

    /// Test-only constructor accepting an injected preset table.
    /// Production callers use [`Self::new`].
    pub fn from_static_slice(embedded: &'static [(&'static str, &'static str)]) -> Self {
        Self { embedded }
    }

    /// Number of embedded presets. Useful for explain commands and
    /// tests; cheap.
    pub fn len(&self) -> usize {
        self.embedded.len()
    }

    /// Whether the embedded table is empty (no first-party presets
    /// shipped in this build).
    pub fn is_empty(&self) -> bool {
        self.embedded.is_empty()
    }

    /// Iterate the embedded preset names in lexicographic order
    /// (the order build.rs writes them).
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.embedded.iter().map(|(name, _)| *name)
    }
}

impl Default for FirstPartyPresetSource {
    fn default() -> Self {
        Self::new()
    }
}

impl PresetSource for FirstPartyPresetSource {
    fn resolve(&self, preset_ref: &PresetRef) -> Result<PresetFile, PresetResolveError> {
        if preset_ref.host != FIRST_PARTY_HOST {
            return Err(PresetResolveError::NotFound {
                host: preset_ref.host.clone(),
                name: preset_ref.name.clone(),
            });
        }
        let Some((_, body)) = self.embedded.iter().find(|(n, _)| *n == preset_ref.name) else {
            return Err(PresetResolveError::NotFound {
                host: preset_ref.host.clone(),
                name: preset_ref.name.clone(),
            });
        };
        let preset: PresetFile = toml::from_str(body).map_err(|e| {
            // First-party presets are authored in-tree; any parse
            // failure is a build-time discipline bug rather than a
            // user-facing failure. Surface it loudly through the
            // NotFound channel for now (the embedded table is the
            // only path that can reach this), with the underlying
            // toml error in the host slot for visibility. A dedicated
            // error variant lands when external presets join the
            // surface.
            PresetResolveError::NotFound {
                host: FIRST_PARTY_HOST.to_string(),
                name: format!("{} (parse: {e})", preset_ref.name),
            }
        })?;
        // Schema discipline: filename (key in the embedded table)
        // must match the preset's `name` field. The build.rs codegen
        // derives the key from the filename, so this catches a
        // preset author who renamed a file but forgot to update the
        // `name` field inside.
        if preset.name != preset_ref.name {
            return Err(PresetResolveError::NotFound {
                host: FIRST_PARTY_HOST.to_string(),
                name: format!(
                    "{} (filename / preset.name mismatch: file `{}.toml` carries name = `{}`)",
                    preset_ref.name, preset_ref.name, preset.name
                ),
            });
        }
        Ok(preset)
    }
}

/// Parse the optional `extends` string from a per-lint TOML block
/// into a `PresetRef`. Returns `Ok(None)` when the field is absent.
pub(crate) fn parse_extends(value: Option<&toml::Value>) -> Result<Option<PresetRef>, PresetResolveError> {
    match value {
        None => Ok(None),
        Some(toml::Value::String(s)) => parse_preset_shorthand(s).map(Some),
        Some(other) => Err(PresetResolveError::MalformedShorthand {
            input: format!("{other:?}"),
            reason: "extends value must be a TOML string".to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inline preset TOML used by tests. Constructed as a `&'static str`
    /// so it can be stuffed into a `&'static [(&str, &str)]` slice.
    const STUB_BASE: &str = r#"
schema_version = "1.0"
name = "base"
primitive = "forbidden_imports"

[config]
forbidden = ["alloc::*"]
"#;

    const STUB_EXTENDER: &str = r#"
schema_version = "1.0"
name = "extender"
primitive = "forbidden_imports"
extends = "mockspace::base"

[config]
reason = "extends base"
"#;

    const STUB_MISNAMED_FILE: &str = r#"
schema_version = "1.0"
name = "different-name"
primitive = "forbidden_imports"
"#;

    const STUB_UNPARSEABLE: &str = r#"
schema_version = "1.0"
name = "broken
"#;

    fn source_with(entries: &'static [(&'static str, &'static str)]) -> FirstPartyPresetSource {
        FirstPartyPresetSource::from_static_slice(entries)
    }

    #[test]
    fn resolves_known_first_party_preset() {
        let src = source_with(&[("base", STUB_BASE)]);
        let pref = parse_preset_shorthand("mockspace::base").unwrap();
        let preset = src.resolve(&pref).unwrap();
        assert_eq!(preset.name, "base");
        assert_eq!(preset.primitive, "forbidden_imports");
        assert!(preset.config.contains_key("forbidden"));
    }

    #[test]
    fn unknown_first_party_preset_returns_not_found() {
        let src = source_with(&[("base", STUB_BASE)]);
        let pref = parse_preset_shorthand("mockspace::unknown").unwrap();
        match src.resolve(&pref) {
            Err(PresetResolveError::NotFound { host, name }) => {
                assert_eq!(host, "mockspace");
                assert_eq!(name, "unknown");
            }
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn non_first_party_host_returns_not_found() {
        // External hosts are scoped out of this slice; they land with
        // Phase 4. The resolver must surface NotFound (rather than e.g.
        // panic) so callers can fall through to the next source in a
        // future chained-source design.
        let src = source_with(&[("base", STUB_BASE)]);
        let pref = parse_preset_shorthand("stack-lints::no-heap").unwrap();
        match src.resolve(&pref) {
            Err(PresetResolveError::NotFound { host, .. }) => {
                assert_eq!(host, "stack-lints");
            }
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn filename_name_mismatch_is_hard_error() {
        let src = source_with(&[("mismatch", STUB_MISNAMED_FILE)]);
        let pref = parse_preset_shorthand("mockspace::mismatch").unwrap();
        match src.resolve(&pref) {
            Err(PresetResolveError::NotFound { name, .. }) => {
                // The diagnostic mentions the mismatch so a preset
                // author sees the actual cause.
                assert!(
                    name.contains("filename") && name.contains("name = `different-name`"),
                    "diagnostic should name the mismatch; got `{name}`"
                );
            }
            other => panic!("expected NotFound (mismatch), got {other:?}"),
        }
    }

    #[test]
    fn unparseable_preset_surfaces_toml_error_in_name() {
        let src = source_with(&[("broken", STUB_UNPARSEABLE)]);
        let pref = parse_preset_shorthand("mockspace::broken").unwrap();
        match src.resolve(&pref) {
            Err(PresetResolveError::NotFound { name, .. }) => {
                assert!(
                    name.contains("parse:"),
                    "diagnostic should include parse-error tail; got `{name}`"
                );
            }
            other => panic!("expected NotFound (parse), got {other:?}"),
        }
    }

    #[test]
    fn resolve_chain_walks_first_party_extends() {
        let src = source_with(&[("base", STUB_BASE), ("extender", STUB_EXTENDER)]);
        let start = parse_preset_shorthand("mockspace::extender").unwrap();
        let chain = mockspace_config::resolve_preset_chain(&start, &src).unwrap();
        // Innermost-first: base before extender.
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].name, "base");
        assert_eq!(chain[1].name, "extender");
    }

    #[test]
    fn default_constructor_uses_embedded_table() {
        // Default::default == ::new(). Both bind to the build.rs-generated
        // EMBEDDED_PRESET_TOML slice. When the workspace has no first-party
        // presets shipped, the table is empty; either way, the constructors
        // agree on length and on the produced names.
        let from_new = FirstPartyPresetSource::new();
        let from_default = FirstPartyPresetSource::default();
        assert_eq!(from_new.len(), from_default.len());
        let names_new: Vec<&str> = from_new.names().collect();
        let names_default: Vec<&str> = from_default.names().collect();
        assert_eq!(names_new, names_default);
    }

    #[test]
    fn names_iter_is_lexicographically_sorted() {
        let entries: &'static [(&'static str, &'static str)] = &[
            ("alpha", STUB_BASE),
            ("beta", STUB_BASE),
            ("charlie", STUB_BASE),
        ];
        let src = source_with(entries);
        let names: Vec<&str> = src.names().collect();
        assert_eq!(names, vec!["alpha", "beta", "charlie"]);
    }

    // ---- parse_extends ----

    #[test]
    fn parse_extends_returns_none_on_absent_field() {
        assert!(parse_extends(None).unwrap().is_none());
    }

    #[test]
    fn parse_extends_parses_shorthand_string() {
        let value = toml::Value::String("mockspace::base".to_string());
        let pref = parse_extends(Some(&value)).unwrap().unwrap();
        assert_eq!(pref.host, "mockspace");
        assert_eq!(pref.name, "base");
    }

    #[test]
    fn parse_extends_rejects_non_string_value() {
        let value = toml::Value::Integer(42);
        match parse_extends(Some(&value)) {
            Err(PresetResolveError::MalformedShorthand { reason, .. }) => {
                assert!(reason.contains("TOML string"));
            }
            other => panic!("expected MalformedShorthand, got {other:?}"),
        }
    }

    // ---- shipped-tree integrity (#541) -------------------------------------

    /// Catalog primitive names every first-party preset must point at.
    /// Defined inline rather than imported from the catalog so this test
    /// stays a discipline assertion: when a new primitive ships, a preset
    /// author adds it here before the lookup will accept it.
    const KNOWN_PRIMITIVES: &[&str] = &[
        "token-scan",
        "ast-type-position",
        "ast-node-position-match",
        "identifier-pattern",
        "content-regex",
        "term-replacement-table",
        "file-metric",
        "undocumented-item",
        "cross-doc-symbol",
        "workflow-state",
        "suppression-meta",
        "directive-style-consistency",
        "no-bare-vec",
        "no-manual-id",
        "no-manual-impl",
        "no-adhoc-framework",
        "registrable-completeness",
        "deprecation-comparison",
    ];

    /// Every preset shipped in `presets/` must resolve, parse cleanly,
    /// match its filename, name a known primitive, and carry at least
    /// one of (config, scope) populated. This is the per-file gate that
    /// catches typos in the embedded preset tree.
    #[test]
    fn every_shipped_first_party_preset_loads_clean() {
        let src = FirstPartyPresetSource::new();
        assert!(
            !src.is_empty(),
            "presets/*.toml should ship at least one entry; build.rs found none"
        );
        for name in src.names() {
            let pref = parse_preset_shorthand(&format!("mockspace::{name}"))
                .unwrap_or_else(|e| panic!("shorthand for `mockspace::{name}` failed: {e}"));
            let preset = src
                .resolve(&pref)
                .unwrap_or_else(|e| panic!("preset `mockspace::{name}` resolve failed: {e}"));
            assert_eq!(
                preset.name, name,
                "filename / preset.name mismatch for `{name}`"
            );
            assert!(
                KNOWN_PRIMITIVES.contains(&preset.primitive.as_str()),
                "preset `{name}` names unknown primitive `{}`; \
                 add to KNOWN_PRIMITIVES or fix the preset",
                preset.primitive
            );
            assert!(
                !preset.config.is_empty() || !preset.scope.is_empty(),
                "preset `{name}` carries neither config nor scope; \
                 a preset with no overlays is dead weight"
            );
        }
    }

    /// Spot-check the canonical stack-lint preset names ship in the
    /// embedded table. Catches the case where a preset file is renamed
    /// or accidentally deleted without an updating reference here.
    #[test]
    fn canonical_stack_lint_presets_are_shipped() {
        let src = FirstPartyPresetSource::new();
        let names: std::collections::HashSet<&str> = src.names().collect();
        for expected in &[
            "no-alloc",
            "no-std",
            "no-dyn-dispatch",
            "no-runtime-spawn",
            "no-runtime-registration",
            "no-bare-numeric",
            "no-bare-string",
            "no-bare-option",
            "no-bare-result",
            "no-public-raw-field",
            "no-vec-in-trait-sig",
            "strategy-marker-required",
            "trait-first-signatures",
            "writing-style",
            "lint-allow-requires-task-id",
        ] {
            assert!(
                names.contains(expected),
                "canonical first-party preset `{expected}` missing from embedded table"
            );
        }
    }
}
