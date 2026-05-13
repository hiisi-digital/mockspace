//! TOML parsing for mockspace.toml.

use std::fs;
use std::path::Path;

use crate::config::Config;
use crate::error::ConfigError;

/// Read and parse a mockspace.toml file from disk.
pub fn parse_mockspace_toml(path: &Path) -> Result<Config, ConfigError> {
    let contents = fs::read_to_string(path)?;
    let cfg: Config = toml::from_str(&contents)?;
    Ok(cfg)
}

/// Parse a mockspace.toml from a string source (useful for tests and
/// in-memory rendering pipelines).
pub fn parse_mockspace_toml_str(source: &str) -> Result<Config, ConfigError> {
    let cfg: Config = toml::from_str(source)?;
    Ok(cfg)
}
