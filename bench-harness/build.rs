//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

/// The targets a stage link is made on, by the call each one takes. The one
/// list: `stage.rs` reads the `stage_link` cfg this sets and names no target.
const STAGE_LINK_TARGETS: &[(&str, &str)] = &[
    ("linux", "unix"),
    ("macos", "unix"),
    ("freebsd", "unix"),
    ("netbsd", "unix"),
    ("openbsd", "unix"),
    ("windows", "windows"),
];

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // When the `perf-counters` feature is on (macOS), the harness links the
    // private `kperf.framework`, which lives in the private-frameworks
    // directory that is not on the default framework search path. Add it so
    // `#[link(name = "kperf", kind = "framework")]` resolves at link time.
    // No-op when the feature is off or off-mac.
    if std::env::var("CARGO_FEATURE_PERF_COUNTERS").is_ok() && target_os == "macos" {
        println!("cargo:rustc-link-search=framework=/System/Library/PrivateFrameworks");
    }

    // A stage link exists only where the consumer asked for it, by the
    // `stage-link` feature, and the target is one the list above names.
    println!("cargo::rustc-check-cfg=cfg(stage_link, values(\"unix\", \"windows\"))");
    if std::env::var("CARGO_FEATURE_STAGE_LINK").is_ok()
        && let Some((_, call)) = STAGE_LINK_TARGETS.iter().find(|(os, _)| *os == target_os)
    {
        println!("cargo::rustc-cfg=stage_link=\"{call}\"");
    }
}
