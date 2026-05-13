//! Template engine wrapping minijinja.

use minijinja::{Environment, UndefinedBehavior};
use serde::Serialize;

use crate::error::RenderError;

/// Registry of named templates with shared filters and globals.
pub struct TemplateEnv {
    inner: Environment<'static>,
}

impl TemplateEnv {
    /// Construct a new environment with strict undefined-variable handling
    /// and autoescape disabled (templates emit plain text, not HTML).
    pub fn new() -> Self {
        let mut inner = Environment::new();
        inner.set_undefined_behavior(UndefinedBehavior::Strict);
        inner.set_auto_escape_callback(|_name| minijinja::AutoEscape::None);
        Self { inner }
    }

    /// Register a template under the given name. The source is copied.
    pub fn add_template(&mut self, name: &str, source: &str) -> Result<(), RenderError> {
        self.inner
            .add_template_owned(name.to_string(), source.to_string())?;
        Ok(())
    }

    /// Look up a previously registered template by name.
    pub fn get_template<'a>(&'a self, name: &str) -> Result<Template<'a>, RenderError> {
        let t = self
            .inner
            .get_template(name)
            .map_err(|_| RenderError::TemplateNotFound(name.to_string()))?;
        Ok(Template { inner: t })
    }

    /// Render a one-off template string without registering it.
    pub fn render_str<C: Serialize>(
        &self,
        source: &str,
        ctx: &C,
    ) -> Result<String, RenderError> {
        Ok(self.inner.render_str(source, ctx)?)
    }

    /// Access the underlying minijinja environment for advanced consumers
    /// (custom filter / function registration).
    pub fn inner_mut(&mut self) -> &mut Environment<'static> {
        &mut self.inner
    }
}

impl Default for TemplateEnv {
    fn default() -> Self {
        Self::new()
    }
}

/// A handle to a compiled template registered in a `TemplateEnv`.
pub struct Template<'env> {
    inner: minijinja::Template<'env, 'env>,
}

impl<'env> Template<'env> {
    /// Render the template against a serde-serializable context.
    pub fn render<C: Serialize>(&self, ctx: &C) -> Result<String, RenderError> {
        Ok(self.inner.render(ctx)?)
    }
}
