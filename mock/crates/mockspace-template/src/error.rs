use std::fmt;
use std::io;

#[derive(Debug)]
pub enum RenderError {
    Minijinja(minijinja::Error),
    Io(io::Error),
    TemplateNotFound(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Minijinja(e) => write!(f, "template error: {e}"),
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::TemplateNotFound(name) => write!(f, "template not found: {name}"),
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Minijinja(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::TemplateNotFound(_) => None,
        }
    }
}

impl From<minijinja::Error> for RenderError {
    fn from(e: minijinja::Error) -> Self {
        Self::Minijinja(e)
    }
}

impl From<io::Error> for RenderError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
