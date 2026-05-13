use std::fmt;
use std::io;

#[derive(Debug)]
pub enum ConfigError {
    Io(io::Error),
    Parse(toml::de::Error),
    MissingField(&'static str),
    ValidationFailed { rule: String, details: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "config io error: {e}"),
            Self::Parse(e) => write!(f, "config parse error: {e}"),
            Self::MissingField(name) => write!(f, "missing required config field: {name}"),
            Self::ValidationFailed { rule, details } => {
                write!(f, "config validation failed [{rule}]: {details}")
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Parse(e) => Some(e),
            Self::MissingField(_) | Self::ValidationFailed { .. } => None,
        }
    }
}

impl From<io::Error> for ConfigError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(e: toml::de::Error) -> Self {
        Self::Parse(e)
    }
}

#[derive(Debug)]
pub enum MappingError {
    MissingField { name: &'static str },
    Custom(String),
}

impl fmt::Display for MappingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField { name } => {
                write!(f, "consumer config cannot produce mockspace Config: missing field `{name}`")
            }
            Self::Custom(s) => write!(f, "mapping error: {s}"),
        }
    }
}

impl std::error::Error for MappingError {}
