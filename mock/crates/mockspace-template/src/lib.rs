//! Template engine and agent-file generator extracted from mockspace internals.
//!
//! See `DESIGN.md.tmpl` in this crate's directory for the full design.

pub mod atomic;
pub mod error;
pub mod platform;
pub mod platforms;
pub mod renderer;
pub mod template;

pub use atomic::{render_atomic, write_atomic};
pub use error::RenderError;
pub use platform::{HookDecl, Platform};
pub use platforms::{ClaudePlatform, CopilotPlatform};
pub use renderer::{walk_template_tree, AgentRenderer, RenderReport, RenderedFile};
pub use template::{Template, TemplateEnv};
