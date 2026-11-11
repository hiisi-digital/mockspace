//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The registry's shared test world, as the engine reads a clone of it: through its own config.

use crate::config::Config;

/// A clone of the world as the engine's config of it.
pub(crate) trait EngineClone {
    /// A clone whose project mounts `ticket` on the ref (`mounted`) or keeps it in the tree.
    fn clone_of(&self, name: &str, mounted: bool) -> Config;
}

impl EngineClone for mockspace_registry::test_world::World {
    fn clone_of(&self, name: &str, mounted: bool) -> Config {
        let root = self.project(name, mounted);
        let cfg = Config::from_dir(&root.join("mock"));
        crate::registry::generate_schemas(&cfg.repo_root, &cfg.mock_dir, &cfg.registry_namespaces);
        cfg
    }
}
