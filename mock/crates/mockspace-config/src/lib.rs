//! v2 `mockspace.toml` schema (spec §46) + parser.
//!
//! See `DESIGN.md.tmpl` in this crate's directory for the full design.

pub mod config;
pub mod error;
pub mod parse;

pub use config::{
    BuiltInLiteral, Config, CrateColor, DomainKind, ExtImport, ForgeKind, HostSection,
    ImportsSection, KnownMacro, LanguageEntry, LanguageHost, LintConfig, LintCrateRef,
    MergeStyle, MockspaceSection, OnDirtyState, ProfileSection, RefsSection,
    RefsSecuritySection, ScopedLintConfig, Severity, TransparencySection, UndoSection,
};
pub use error::ConfigError;
pub use parse::{parse_mockspace_toml, parse_mockspace_toml_str};
