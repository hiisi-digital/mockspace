//! TOML parsing for v2 `mockspace.toml` (spec §46).

use std::fs;
use std::path::Path;

use crate::config::Config;
use crate::error::ConfigError;

/// Schema version this loader accepts. The `[mockspace] version` field is
/// matched on the major prefix; minor advances within the same major are
/// loaded with a warning surface to be added once the diagnostic sink lands
/// (spec §50 schema evolution windows).
pub const SCHEMA_MAJOR: u32 = 1;

/// Read and parse a v2 `mockspace.toml` from disk.
pub fn parse_mockspace_toml(path: &Path) -> Result<Config, ConfigError> {
    let contents = fs::read_to_string(path)?;
    parse_mockspace_toml_str(&contents)
}

/// Parse a v2 `mockspace.toml` from a string source.
///
/// The schema is strict: any TOML key not declared on [`Config`] errors
/// at parse time via serde's `deny_unknown_fields`. Retired sections
/// (e.g. legacy `[primitive-introductions]`) surface as unknown-field
/// errors with no special-cased detection.
pub fn parse_mockspace_toml_str(source: &str) -> Result<Config, ConfigError> {
    let cfg: Config = toml::from_str(source)?;
    validate_version(&cfg.mockspace.version)?;
    Ok(cfg)
}

fn validate_version(version: &str) -> Result<(), ConfigError> {
    let major = parse_major(version).ok_or_else(|| {
        ConfigError::Validation {
            rule:    "mockspace.version",
            details: format!("expected `<major>.<minor>` form (e.g. \"1.0\"); got {version:?}"),
        }
    })?;
    if major != SCHEMA_MAJOR {
        return Err(ConfigError::Validation {
            rule:    "mockspace.version",
            details: format!(
                "schema major {major} not supported by this loader (expected {SCHEMA_MAJOR})"
            ),
        });
    }
    Ok(())
}

fn parse_major(version: &str) -> Option<u32> {
    let head = version.split('.').next()?;
    head.parse::<u32>().ok()
}
