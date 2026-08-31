//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The readme tells a reader which key to write in `mockspace.toml` and what a
//! version pin resolves to. Both are facts about the descriptor, so both are
//! checked against it rather than against anybody's memory of it.

use cargo_mock::TOOL;

const README: &str = include_str!("../../README.md");

/// Every `mockspace_*` key the readme names, in the order it names them.
fn keys_named_in_the_readme() -> Vec<String> {
    let mut found = Vec::new();
    for line in README.lines() {
        let Some(at) = line.find("mockspace_") else {
            continue;
        };
        let rest = &line[at ..];
        let end = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        let key = rest[.. end].to_string();
        if !found.contains(&key) {
            found.push(key);
        }
    }
    found
}

#[test]
fn the_readme_names_no_pin_key_the_descriptor_does_not_read() {
    let renki::PinKeys {
        version,
        rev,
        tag,
        branch,
        git,
    } = TOOL.pin_keys;
    let real = [version, rev, tag, branch, git];

    let named = keys_named_in_the_readme();
    // A readme that mentions none of them would pass every assertion below
    // while documenting nothing, so the count is asserted before the contents.
    assert!(
        named.len() >= 3,
        "the readme names {} pin keys, which is too few to be the install \
         section this test is about: {named:?}",
        named.len()
    );

    for key in &named {
        assert!(
            real.contains(&key.as_str()),
            "the readme tells a reader to write `{key}`, which is not one of \
             the keys the descriptor reads: {real:?}"
        );
    }
}

#[test]
fn a_version_pin_resolves_to_the_bare_version_as_a_tag() {
    // The readme says a version resolves against the matching git tag, which
    // holds because the engine crate is `publish = false` and so the registry
    // attempt renki tries first can never land. A `version_tags` hook would
    // spell the tag differently and make that sentence wrong.
    assert!(
        TOOL.hooks.version_tags.is_none(),
        "a version_tags hook changes what tag a version pin looks for, so the \
         readme's pin section has to change with it"
    );

    let engine_manifest = include_str!("../../Cargo.toml");
    assert!(
        engine_manifest.contains("publish = false"),
        "the engine publishes now, so a version pin resolves from the registry \
         and the readme should stop saying it resolves against a tag"
    );
}

/// The version in the readme's worked example is the one this workspace is.
///
/// The example tells a reader to write `mockspace_version = "<x>"`, and a version
/// pin resolves against the matching git tag. So a stale number there sends every
/// stranger following the readme at a tag that does not exist, and they get a
/// resolution failure and no engine, with nothing in the message about the readme
/// being wrong.
///
/// The sibling test above reads the pin *keys* and never the value, which is
/// exactly how the readme sat at `0.0.1` while the workspace moved: every
/// assertion about the example passed, because none of them looked at the number.
#[test]
fn the_readme_example_pins_the_version_this_workspace_is() {
    let readme = include_str!("../../README.md");
    let manifest = include_str!("../../Cargo.toml");

    let workspace_version = manifest
        .lines()
        .find_map(|l| l.trim().strip_prefix("version = "))
        .map(|v| v.trim().trim_matches('"').to_string())
        .expect("the workspace manifest declares a version");

    let example = readme
        .lines()
        .find_map(|l| l.trim().strip_prefix("mockspace_version = "))
        .map(|v| v.trim().trim_matches('"').to_string())
        .expect("the readme shows a mockspace_version example");

    assert_eq!(
        example, workspace_version,
        "the readme pins {example} and this workspace is {workspace_version}. A version \
         resolves against a git tag, so the readme is telling readers to pin a tag that \
         will not exist. Move both together, and tag the release."
    );
}
