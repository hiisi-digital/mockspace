//! Preset chain resolution + cycle detection (task #538).
//!
//! The preset infrastructure shipped in #55 added the [`PresetFile`]
//! type and the `LintConfig.extends` shorthand. This module implements
//! the algorithm that turns `extends = "<host>::<name>"` into a fully
//! resolved chain of preset overlays.
//!
//! # The algorithm
//!
//! Given a starting preset reference (typically from `LintConfig.extends`
//! or `PresetFile.extends`), the resolver walks the `extends` chain
//! depth-first, returning the chain in **innermost-first** order. The
//! caller then applies overlays in that order, layering each preset's
//! configuration on top of the deeper presets. This matches the cascade
//! ordering documented in
//! `mock/research/202605220500_lint-preset-infrastructure.md`.
//!
//! ```text
//! consumer extends outer extends inner
//!
//! resolve_preset_chain returns: [inner, outer, consumer]
//! caller applies overlays: catalog_defaults
//!                       <- inner   (innermost; applied first)
//!                       <- outer
//!                       <- consumer
//!                       <- workspace defaults (downstream of the chain)
//! ```
//!
//! # Cycle detection
//!
//! The chain can self-reference (`a extends b extends a`) or otherwise
//! loop. The resolver walks with a `BTreeSet<(host, name)>` visited set
//! and emits [`PresetResolveError::Cycle`] with the full path on re-entry.
//! This is a hard error per the memo: "silent infinite recursion at
//! config load is the worst possible failure mode."
//!
//! # Source abstraction
//!
//! [`PresetSource`] is the lookup trait the resolver calls into. The
//! production implementation reads from the lockfile + imports; tests
//! pass mock sources. This keeps the resolver itself free of filesystem
//! or network concerns; integration into `instantiate_with_cascade` lands
//! in #539+ alongside first-party preset embedding.

use std::collections::BTreeSet;
use std::fmt;

use crate::config::PresetFile;

/// A reference to a preset by host and name. Produced by parsing the
/// `<host>::<name>` shorthand.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PresetRef {
    pub host: String,
    pub name: String,
}

impl PresetRef {
    /// Reconstruct the shorthand form (`<host>::<name>`).
    pub fn shorthand(&self) -> String {
        format!("{}::{}", self.host, self.name)
    }
}

impl fmt::Display for PresetRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.host, self.name)
    }
}

/// Parse the `<host>::<name>` preset shorthand.
///
/// Both `host` and `name` are required and non-empty. Whitespace is not
/// trimmed; consumers should pass clean strings. The separator is the
/// two-character literal `::` (matching the existing identifier-path
/// separator used across the v2 spec).
///
/// First-party presets use the literal host name `mockspace`; the loader
/// expands `mockspace::<name>` to `mock://@/export/lint-preset/<name>`
/// (the embedded preset tree) downstream of this parse.
pub fn parse_preset_shorthand(input: &str) -> Result<PresetRef, PresetResolveError> {
    let trimmed = input.trim();
    let (host, name) =
        trimmed
            .split_once("::")
            .ok_or_else(|| PresetResolveError::MalformedShorthand {
                input: input.to_string(),
                reason: "missing `::` separator".to_string(),
            })?;
    if host.is_empty() {
        return Err(PresetResolveError::MalformedShorthand {
            input: input.to_string(),
            reason: "empty host before `::`".to_string(),
        });
    }
    if name.is_empty() {
        return Err(PresetResolveError::MalformedShorthand {
            input: input.to_string(),
            reason: "empty preset name after `::`".to_string(),
        });
    }
    if name.contains("::") {
        return Err(PresetResolveError::MalformedShorthand {
            input: input.to_string(),
            reason: "shorthand contains more than one `::` separator".to_string(),
        });
    }
    Ok(PresetRef {
        host: host.to_string(),
        name: name.to_string(),
    })
}

/// Lookup interface the resolver calls into to fetch preset files.
///
/// The production implementation reads from the lockfile + imports tree.
/// Tests pass mock implementations to verify resolution and cycle
/// detection without filesystem or network state.
pub trait PresetSource {
    /// Fetch the preset at `<host>::<name>`. Implementors return
    /// [`PresetResolveError::NotFound`] when the preset is missing
    /// (typical when the consumer forgot to register an import for
    /// the host).
    fn resolve(&self, preset_ref: &PresetRef) -> Result<PresetFile, PresetResolveError>;
}

/// Walk the `extends` chain from `start`, returning the resolved presets
/// in **innermost-first** order.
///
/// The returned vector is ordered so the caller can iterate it directly
/// to apply overlays in the right order: deeper presets first, then
/// progressively shallower presets, with the start preset last.
///
/// # Cycles
///
/// Cycles trigger [`PresetResolveError::Cycle`] with the full path
/// (from start to the re-entered node). The walk halts on first
/// detection; partial state is not returned.
///
/// # Missing extends targets
///
/// A preset whose `extends` field names a `<host>::<name>` the source
/// does not know triggers [`PresetResolveError::NotFound`]. Missing
/// targets are install-time problems surfaced at check-time per the
/// memo.
pub fn resolve_preset_chain(
    start: &PresetRef,
    source: &dyn PresetSource,
) -> Result<Vec<PresetFile>, PresetResolveError> {
    let mut visited = BTreeSet::new();
    let mut path = Vec::new();
    let mut chain = Vec::new();
    walk(start, source, &mut visited, &mut path, &mut chain)?;
    Ok(chain)
}

fn walk(
    current: &PresetRef,
    source: &dyn PresetSource,
    visited: &mut BTreeSet<PresetRef>,
    path: &mut Vec<PresetRef>,
    chain: &mut Vec<PresetFile>,
) -> Result<(), PresetResolveError> {
    // The `visited` set is global across the walk and is never popped.
    // For a chain graph (each preset has at most one `extends` parent)
    // "visited at all" implies "ancestor of the current node", so the
    // cycle detection is sound. If `extends` ever grows to support
    // multiple parents (overlays from several presets at once), the
    // diamond `start -> A -> C` then `start -> B -> C` would report
    // `Cycle` against `C` despite no cycle existing; the right fix
    // then is to track the active path with a separate set and pop
    // on backtrack.
    if !visited.insert(current.clone()) {
        path.push(current.clone());
        return Err(PresetResolveError::Cycle {
            path: path.iter().map(|r| r.shorthand()).collect(),
        });
    }
    path.push(current.clone());
    let preset = source.resolve(current)?;
    if let Some(parent_shorthand) = preset.extends.as_deref() {
        let parent = parse_preset_shorthand(parent_shorthand).map_err(|source| {
            PresetResolveError::MalformedExtends {
                parent: current.clone(),
                source: Box::new(source),
            }
        })?;
        walk(&parent, source, visited, path, chain)?;
    }
    chain.push(preset);
    path.pop();
    Ok(())
}

/// Errors produced by the preset resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetResolveError {
    /// The `<host>::<name>` shorthand was malformed (missing separator,
    /// empty parts, extra `::`).
    MalformedShorthand { input: String, reason: String },
    /// An `extends` field on a preset carried a malformed shorthand.
    /// `parent` names the preset that owns the bad `extends`; `source`
    /// carries the underlying [`MalformedShorthand`].
    MalformedExtends {
        parent: PresetRef,
        source: Box<PresetResolveError>,
    },
    /// A preset named in an `extends` chain was not findable through
    /// the source. The host has likely not been imported, or the
    /// import resolution failed.
    NotFound { host: String, name: String },
    /// The `extends` chain cycles back on itself. `path` records every
    /// shorthand visited from the start of the chain to (and including)
    /// the re-entered preset.
    Cycle { path: Vec<String> },
}

impl fmt::Display for PresetResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedShorthand { input, reason } => write!(
                f,
                "malformed preset shorthand `{input}`: {reason} (expected `<host>::<name>`)"
            ),
            Self::MalformedExtends { parent, source } => write!(
                f,
                "while resolving extends of `{parent}`: {source}"
            ),
            Self::NotFound { host, name } => write!(
                f,
                "preset `{host}::{name}` not found; ensure `{host}` is in [imports] and the preset name is correct"
            ),
            Self::Cycle { path } => {
                writeln!(f, "preset cycle detected:")?;
                for (i, step) in path.iter().enumerate() {
                    if i == 0 {
                        writeln!(f, "  {step}")?;
                    } else if i + 1 == path.len() {
                        writeln!(f, "    extends {step} (cycle)")?;
                    } else {
                        writeln!(f, "    extends {step}")?;
                    }
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for PresetResolveError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn preset(name: &str, extends: Option<&str>) -> PresetFile {
        PresetFile {
            schema_version: "1.0".to_string(),
            name: name.to_string(),
            primitive: "forbidden_imports".to_string(),
            description: None,
            extends: extends.map(String::from),
            config: BTreeMap::new(),
            severity: Default::default(),
            scope: BTreeMap::new(),
        }
    }

    /// In-memory source keyed by full `<host>::<name>` shorthand.
    struct MockSource {
        files: BTreeMap<String, PresetFile>,
    }

    impl MockSource {
        fn new() -> Self {
            Self {
                files: BTreeMap::new(),
            }
        }

        fn insert(mut self, host: &str, preset: PresetFile) -> Self {
            self.files
                .insert(format!("{host}::{}", preset.name), preset);
            self
        }
    }

    impl PresetSource for MockSource {
        fn resolve(&self, preset_ref: &PresetRef) -> Result<PresetFile, PresetResolveError> {
            self.files
                .get(&preset_ref.shorthand())
                .cloned()
                .ok_or_else(|| PresetResolveError::NotFound {
                    host: preset_ref.host.clone(),
                    name: preset_ref.name.clone(),
                })
        }
    }

    // ---- parse_preset_shorthand ----

    #[test]
    fn shorthand_parses_host_and_name() {
        let r = parse_preset_shorthand("stack-lints::no-heap").unwrap();
        assert_eq!(r.host, "stack-lints");
        assert_eq!(r.name, "no-heap");
        assert_eq!(r.shorthand(), "stack-lints::no-heap");
    }

    #[test]
    fn shorthand_parses_first_party_mockspace_host() {
        let r = parse_preset_shorthand("mockspace::no-bare-numeric").unwrap();
        assert_eq!(r.host, "mockspace");
        assert_eq!(r.name, "no-bare-numeric");
    }

    #[test]
    fn shorthand_trims_surrounding_whitespace() {
        // Internal whitespace is rejected (split_once will leave it in);
        // surrounding trim mirrors how a TOML reader hands us the value.
        let r = parse_preset_shorthand("  stack-lints::no-heap  ").unwrap();
        assert_eq!(r.host, "stack-lints");
    }

    #[test]
    fn shorthand_rejects_missing_separator() {
        let err = parse_preset_shorthand("no-heap").unwrap_err();
        assert!(matches!(
            err,
            PresetResolveError::MalformedShorthand { ref reason, .. }
                if reason.contains("missing")
        ));
    }

    #[test]
    fn shorthand_rejects_empty_host() {
        let err = parse_preset_shorthand("::no-heap").unwrap_err();
        assert!(matches!(
            err,
            PresetResolveError::MalformedShorthand { ref reason, .. }
                if reason.contains("empty host")
        ));
    }

    #[test]
    fn shorthand_rejects_empty_name() {
        let err = parse_preset_shorthand("stack-lints::").unwrap_err();
        assert!(matches!(
            err,
            PresetResolveError::MalformedShorthand { ref reason, .. }
                if reason.contains("empty preset name")
        ));
    }

    #[test]
    fn shorthand_rejects_triple_colon() {
        // `stack::lints::no-heap` would split into host=`stack`,
        // name=`lints::no-heap`. Invalid because name carries `::`.
        let err = parse_preset_shorthand("stack::lints::no-heap").unwrap_err();
        assert!(matches!(
            err,
            PresetResolveError::MalformedShorthand { ref reason, .. }
                if reason.contains("more than one")
        ));
    }

    // ---- resolve_preset_chain ----

    #[test]
    fn resolves_single_preset_with_no_extends() {
        let source = MockSource::new().insert("stack-lints", preset("no-heap", None));
        let start = parse_preset_shorthand("stack-lints::no-heap").unwrap();
        let chain = resolve_preset_chain(&start, &source).unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].name, "no-heap");
    }

    #[test]
    fn resolves_two_level_chain_innermost_first() {
        let source = MockSource::new()
            .insert("mockspace", preset("no-bare-numeric", None))
            .insert(
                "stack-lints",
                preset("no-heap", Some("mockspace::no-bare-numeric")),
            );
        let start = parse_preset_shorthand("stack-lints::no-heap").unwrap();
        let chain = resolve_preset_chain(&start, &source).unwrap();
        assert_eq!(chain.len(), 2);
        // Innermost first: no-bare-numeric is the deepest, then no-heap.
        assert_eq!(chain[0].name, "no-bare-numeric");
        assert_eq!(chain[1].name, "no-heap");
    }

    #[test]
    fn resolves_three_level_chain_innermost_first() {
        let source = MockSource::new()
            .insert("a", preset("base", None))
            .insert("b", preset("middle", Some("a::base")))
            .insert("c", preset("outer", Some("b::middle")));
        let start = parse_preset_shorthand("c::outer").unwrap();
        let chain = resolve_preset_chain(&start, &source).unwrap();
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].name, "base");
        assert_eq!(chain[1].name, "middle");
        assert_eq!(chain[2].name, "outer");
    }

    #[test]
    fn missing_start_returns_not_found() {
        let source = MockSource::new();
        let start = parse_preset_shorthand("stack-lints::no-heap").unwrap();
        let err = resolve_preset_chain(&start, &source).unwrap_err();
        assert!(matches!(
            err,
            PresetResolveError::NotFound { ref host, .. } if host == "stack-lints"
        ));
    }

    #[test]
    fn missing_extends_target_returns_not_found() {
        let source =
            MockSource::new().insert("stack-lints", preset("no-heap", Some("absent::base")));
        let start = parse_preset_shorthand("stack-lints::no-heap").unwrap();
        let err = resolve_preset_chain(&start, &source).unwrap_err();
        assert!(matches!(
            err,
            PresetResolveError::NotFound { ref host, .. } if host == "absent"
        ));
    }

    #[test]
    fn self_referential_cycle_is_detected() {
        let source = MockSource::new().insert("a", preset("loop", Some("a::loop")));
        let start = parse_preset_shorthand("a::loop").unwrap();
        let err = resolve_preset_chain(&start, &source).unwrap_err();
        match err {
            PresetResolveError::Cycle { path } => {
                assert_eq!(path, vec!["a::loop", "a::loop"]);
            }
            other => panic!("expected Cycle, got {other:?}"),
        }
    }

    #[test]
    fn two_node_cycle_is_detected() {
        let source = MockSource::new()
            .insert("a", preset("first", Some("b::second")))
            .insert("b", preset("second", Some("a::first")));
        let start = parse_preset_shorthand("a::first").unwrap();
        let err = resolve_preset_chain(&start, &source).unwrap_err();
        match err {
            PresetResolveError::Cycle { path } => {
                assert_eq!(path, vec!["a::first", "b::second", "a::first"]);
            }
            other => panic!("expected Cycle, got {other:?}"),
        }
    }

    #[test]
    fn three_node_cycle_is_detected_with_full_path() {
        let source = MockSource::new()
            .insert("a", preset("one", Some("b::two")))
            .insert("b", preset("two", Some("c::three")))
            .insert("c", preset("three", Some("a::one")));
        let start = parse_preset_shorthand("a::one").unwrap();
        let err = resolve_preset_chain(&start, &source).unwrap_err();
        match err {
            PresetResolveError::Cycle { path } => {
                assert_eq!(path, vec!["a::one", "b::two", "c::three", "a::one"]);
            }
            other => panic!("expected Cycle, got {other:?}"),
        }
    }

    #[test]
    fn malformed_extends_target_returns_malformed_extends_with_parent() {
        let source = MockSource::new().insert("stack-lints", preset("no-heap", Some("garbage")));
        let start = parse_preset_shorthand("stack-lints::no-heap").unwrap();
        let err = resolve_preset_chain(&start, &source).unwrap_err();
        match err {
            PresetResolveError::MalformedExtends { parent, source } => {
                assert_eq!(parent.shorthand(), "stack-lints::no-heap");
                assert!(matches!(
                    *source,
                    PresetResolveError::MalformedShorthand { .. }
                ));
            }
            other => panic!("expected MalformedExtends, got {other:?}"),
        }
    }

    #[test]
    fn malformed_extends_display_shows_parent_context() {
        let err = PresetResolveError::MalformedExtends {
            parent: PresetRef {
                host: "stack-lints".to_string(),
                name: "no-heap".to_string(),
            },
            source: Box::new(PresetResolveError::MalformedShorthand {
                input: "garbage".to_string(),
                reason: "missing `::` separator".to_string(),
            }),
        };
        let msg = format!("{err}");
        assert!(msg.contains("stack-lints::no-heap"));
        assert!(msg.contains("while resolving extends"));
        assert!(msg.contains("garbage"));
    }

    // ---- Display formatting ----

    #[test]
    fn cycle_error_display_lists_full_path() {
        let err = PresetResolveError::Cycle {
            path: vec![
                "stack-lints::no-heap".to_string(),
                "mockspace::no-bare-numeric".to_string(),
                "stack-lints::no-heap".to_string(),
            ],
        };
        let msg = format!("{err}");
        assert!(msg.contains("stack-lints::no-heap"));
        assert!(msg.contains("mockspace::no-bare-numeric"));
        assert!(msg.contains("(cycle)"));
    }

    #[test]
    fn not_found_error_display_names_host_and_preset() {
        let err = PresetResolveError::NotFound {
            host: "stack-lints".to_string(),
            name: "no-heap".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("stack-lints::no-heap"));
        assert!(msg.contains("[imports]"));
    }
}
