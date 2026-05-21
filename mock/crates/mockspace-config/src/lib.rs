//! v2 `mockspace.toml` schema (spec §46) + parser.
//!
//! See `DESIGN.md.tmpl` in this crate's directory for the full design.

pub mod config;
pub mod error;
pub mod parse;
pub mod preset_resolver;

pub use config::{
    BuiltInLiteral, Config, CrateColor, DomainKind, ExtImport, ForgeKind, GateSeverities,
    HostSection, ImportEntry, ImportKind, ImportsSection, KnownMacro, LanguageEntry, LanguageHost,
    LintConfig, LintCrateRef, MergeStyle, MockspaceSection, OnDirtyState, PresetFile,
    ProfileSection, RefsSection, RefsSecuritySection, ScopedLintConfig, Severity,
    TransparencySection, TypedImport, UndoSection,
};
pub use error::ConfigError;
pub use parse::{parse_mockspace_toml, parse_mockspace_toml_str};
pub use preset_resolver::{
    parse_preset_shorthand, resolve_preset_chain, PresetRef, PresetResolveError, PresetSource,
};
