//! Built-in `Platform` implementations.

pub mod claude;
pub mod copilot;

pub use claude::ClaudePlatform;
pub use copilot::CopilotPlatform;
