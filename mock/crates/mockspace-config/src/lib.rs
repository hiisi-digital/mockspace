//! Canonical mockspace.toml schema + IntoMockspaceConfig trait.
//!
//! See `DESIGN.md.tmpl` in this crate's directory for the full design.

pub mod config;
pub mod error;
pub mod mapping;
pub mod parse;

pub use config::{AttributionConfig, CommitStyle, Config, InstallMode, MacroStyle};
pub use error::{ConfigError, MappingError};
pub use mapping::IntoMockspaceConfig;
pub use parse::{parse_mockspace_toml, parse_mockspace_toml_str};
