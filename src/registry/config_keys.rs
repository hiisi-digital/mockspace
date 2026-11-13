//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The keys of the config's root, which are the engine's to hold.
//!
//! The registry crate holds the keys of the tables it reads (`[registry]`, `[ref]` and what is
//! under them). The root carries every other part of the engine's configuration, so the list of
//! what may stand there is kept here, beside the struct it mirrors, and a tool that keeps a
//! registry without the engine is not told that the engine's keys are unknown.

use crate::registry::{GateScope, RegistryFinding};

/// Keys the document **root** may carry. Mirrors `RawConfig`, plus the one
/// section it deliberately does not deserialize.
///
/// This is the half the first version of this check missed, and it is the half
/// the motivating defect was in: `canon_paths` is a root key, so a typo in it
/// at the root was still discarded in silence after `[ref.roots.*]` was covered.
///
/// It also carries the keys the **launcher** reads rather than the engine: the
/// engine pin and `mock_dir`. Those are `mockspace_manifest::ManifestHeader`'s
/// fields, and leaving them out reported every project's own pin as unknown.
/// Caught by running this against a real config rather than against a fixture,
/// which is the only reason it was caught at all.
///
/// `lints` is here because `config.rs` reads it through `toml_edit` directly
/// rather than through `RawConfig`, its values being heterogeneous. It is a
/// real key and belongs in the list; the test below knows it is the exception.
pub const ROOT_KEYS: &[&str] = &[
    "project_name",
    "crate_prefix",
    "abi_version",
    "src_dirs",
    "append_only",
    "data_ref",
    "lockfiles",
    "proc_macro_crates",
    "lint_proc_macro_source",
    "module_crates",
    "unprefixed_crates",
    "layers",
    "primary_domain_macro",
    "primary_domain_label",
    "install_git_hooks",
    "install_cargo_config",
    "install_agent_files",
    "auto_fmt",
    "auto_clippy_fix",
    "deny_check",
    "domain_kinds",
    "known_macros",
    "agent_macros",
    "macro_styles",
    "crate_colors",
    "crate_grouping",
    "primitive-introductions",
    "canon_paths",
    "panel_consolidate_every",
    "test",
    "registry",
    "deep_dive_index",
    "ref",
    "ordered_docs",
    "primary_docs",
    "lints",
    // `lint-crates` is the same exception as `lints` and was missed for the same
    // reason: `src/bootstrap/lints.rs::parse_lint_crates` reads it out of the
    // document directly, so it is in no struct and a list derived from the
    // structs cannot see it. Every repo declaring an external lint pack was
    // told its declaration is discarded silently, on every run, while the pack
    // compiled a few lines earlier in the same output.
    "lint-crates",
    // `tools` is the third: `tool_config::parse` reads it out of the document,
    // its values being a scalar or an array per key, as `lints` is.
    "tools",
    // the launcher's, from `mockspace_manifest::ManifestHeader`
    "mock_dir",
    "mockspace_git",
    "mockspace_version",
    "mockspace_rev",
    "mockspace_branch",
    "mockspace_tag",
];

/// Keys at the root of the config that deserialize into nothing.
///
/// A key here is the case the motivating defect was in: `canon_paths` is a root key, so a typo in
/// it at the root was discarded in silence while the tables below were covered.
pub fn root_unknown_keys(config_text: &str) -> Vec<RegistryFinding> {
    let Ok(doc) = config_text.parse::<toml_edit::DocumentMut>() else {
        return Vec::new(); // a config that does not parse is somebody else's error
    };
    doc.iter()
        .filter(|(k, _)| !ROOT_KEYS.contains(k))
        .map(|(k, _)| RegistryFinding {
            kind:    "unknown-config-key",
            message: format!(
                "the config declares `{k}` at its root, which mockspace does not read. It is discarded silently."
            ),
            source:  None,
            gate:    GateScope::Always,
        })
        .collect()
}

/// Every key of the config that deserializes into nothing: the root's, then those of the tables the
/// registry reads (`mockspace_registry::config_unknown_keys`).
pub fn config_unknown_keys(config_text: &str) -> Vec<RegistryFinding> {
    let mut found = root_unknown_keys(config_text);
    found.extend(mockspace_registry::config_unknown_keys(config_text));
    found
}
