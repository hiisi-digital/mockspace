//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The schema check: the generated schemas run against the registry data by a TOML validator.

use crate::VerdictCache;

use super::*;

/// The outcome of delegating schema validation to a TOML validator.
pub enum SchemaCheck {
    /// The validator ran. Non-empty means it reported problems.
    Ran {
        failures: Vec<String>,
    },
    /// The validator is not installed. Reported rather than treated as a pass,
    /// because a check that silently does not run is worse than no check: it
    /// produces the same green output as a check that ran and found nothing.
    Unavailable,
}

/// Run the generated schemas against the registry data with `taplo`.
///
/// Delegated rather than reimplemented. The schemas already state required
/// fields, identifier patterns, and types, and they are what the editor
/// validates against; a second implementation inside mockspace would be a
/// second definition of one contract, and the two would eventually disagree.
///
/// The cost of delegating is a dependency the gate cannot assume, which is why
/// absence is reported rather than passed over.
pub fn check_schemas(
    repo_root: &Path,
    namespaces: &[RegistryNamespace],
    saved: Option<&dyn VerdictCache>,
) -> SchemaCheck {
    check_schemas_with(std::ffi::OsStr::new("taplo"), repo_root, namespaces, saved)
}

/// [`check_schemas`] with the validator named, so a test can stand a counting one in for taplo.
pub fn check_schemas_with(
    program: &std::ffi::OsStr,
    repo_root: &Path,
    namespaces: &[RegistryNamespace],
    saved: Option<&dyn VerdictCache>,
) -> SchemaCheck {
    if namespaces.is_empty() {
        return SchemaCheck::Ran {
            failures: Vec::new(),
        };
    }
    let Ok(probe) = std::process::Command::new(program)
        .arg("--version")
        .output()
    else {
        return SchemaCheck::Unavailable;
    };

    // The same inputs as the last pass replay it. Only a pass is ever saved below, so a failure,
    // a check that examined nothing and an absent taplo (returned above, before any lookup) are
    // each judged again.
    let mut version = probe.stdout.clone();
    version.extend_from_slice(&probe.stderr);
    let env_changes_taplo = ["TAPLO_CONFIG", "RUST_LOG"].iter().any(|v| std::env::var_os(v).is_some());
    let key_of = |cache: &dyn VerdictCache| {
        super::schema_replay::input_key(cache, repo_root, &version, env_changes_taplo)
    };
    let key = saved
        .filter(|_| probe.status.success())
        .and_then(|cache| key_of(cache).map(|key| (cache, key)));
    if let Some((cache, key)) = &key {
        let hit = cache.replay(key).as_deref() == Some(super::schema_replay::PASSED);
        cache.tally(hit, "registry-schema-check");
        if hit {
            return SchemaCheck::Ran {
                failures: Vec::new(),
            };
        }
    }

    let out = std::process::Command::new(program)
        .arg("check")
        .current_dir(repo_root)
        .output();

    match out {
        Ok(o) if o.status.success() => {
            // A check that examined nothing succeeded, which is not the same as
            // a check that passed. taplo reports zero files when its `include`
            // does not match, so a wrong pattern reads exactly like a clean
            // registry. Reported rather than trusted.
            let text = String::from_utf8_lossy(&o.stderr);
            if let Some(0) = files_examined(&text) {
                return SchemaCheck::Ran {
                    failures: vec![
                        "taplo examined no files. Check `include` in .taplo.toml: a pattern that matches nothing passes without checking anything."
                            .to_string(),
                    ],
                };
            }
            // Saved only if the inputs are still what the check began with: a file edited while
            // taplo ran must not get this pass saved under its old content.
            if let Some((cache, key)) = &key {
                if key_of(*cache).as_deref() == Some(key.as_str()) {
                    cache.remember(key, super::schema_replay::PASSED);
                }
            }
            SchemaCheck::Ran {
                failures: Vec::new(),
            }
        },
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stderr);
            let failures = text
                .lines()
                .filter(|l| l.contains("error") || l.contains("does not match"))
                .map(|l| l.trim().to_string())
                .collect::<Vec<String>>();
            // A run that failed is a failure whatever it managed to log: its logger can lose the last
            // lines when the process exits, and no lines is not the same as no problem.
            let failures = if failures.is_empty() {
                vec!["taplo failed and logged no line that names a problem.".to_string()]
            } else {
                failures
            };
            SchemaCheck::Ran {
                failures,
            }
        },
        Err(_) => SchemaCheck::Unavailable,
    }
}

/// How many files taplo reported examining, if it said.
///
/// It logs `found files total=N excluded=M`, and what was checked is the
/// difference. Absent from the output, the count is unknown rather than zero,
/// so a taplo that stops logging it does not start failing every run.
pub fn files_examined(stderr: &str) -> Option<usize> {
    let line = stderr.lines().find(|l| l.contains("found files total="))?;
    let get = |key: &str| -> Option<usize> {
        let at = line.find(key)? + key.len();
        line[at ..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .ok()
    };
    let total = get("total=")?;
    let excluded = get("excluded=").unwrap_or(0);
    Some(total.saturating_sub(excluded))
}
