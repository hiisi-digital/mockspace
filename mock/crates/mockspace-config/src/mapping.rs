//! `IntoMockspaceConfig` — bridge from consumer schemas to canonical Config.

use crate::config::Config;
use crate::error::MappingError;

/// Implemented by any consumer typed config that can produce a valid
/// canonical `Config` for mockspace template rendering.
///
/// The trait is the contract that lets mockspace templates render against
/// configs whose schemas differ from mockspace.toml (e.g. homma.toml).
pub trait IntoMockspaceConfig {
    fn into_mockspace_config(self) -> Result<Config, MappingError>;
}

/// Identity impl: a Config is already a Config.
impl IntoMockspaceConfig for Config {
    fn into_mockspace_config(self) -> Result<Config, MappingError> {
        Ok(self)
    }
}
