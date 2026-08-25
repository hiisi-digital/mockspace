//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `cargo-mock` / `mock`: the launcher for the mockspace design-round workflow
//! engine.
//!
//! The launcher machinery is `renki` and is shared with other tools built the
//! same way: finding the repo, reading the pin, building that engine once into
//! a per-version cache, execing it with an absolute mock dir, keeping itself
//! current. None of that is mockspace's and none of it lives here.
//!
//! What lives here is the handful of things that are mockspace's alone. The
//! [`TOOL`] const names the config file, the pin keys and the cache, and the
//! hooks beside it carry the four behaviours no other tool wants: the
//! lint-rules dependency pinned to the exact revision the engine was built
//! from, the durable git-hook gate, the refusal of a retired cargo alias, and
//! the legacy `Cargo.lock` pin that keeps an un-migrated repo running.
//!
//! Installed as two binaries from one source: `cargo-mock`, cargo's external
//! subcommand convention so `cargo mock ...` works, and `mock`, the short
//! direct form.

mod legacy;

use std::path::Path;
use std::process::ExitCode;

use mockspace_manifest::gate::HOOK_VERSION;
use renki::{Anchor, Hooks, Locate, Resolved, Tool, Workdir, pin_keys};

/// The canonical mockspace repository: the engine source when a manifest sets
/// no `mockspace_git`.
pub const CANONICAL_URL: &str = "ssh://git@github.com/hiisi-digital/mockspace.git";

/// mockspace, as a launcher.
pub const TOOL: Tool = Tool {
    // the config lives in the repository, so the root is the repository.
    anchor:          Anchor::Marker(".git"),
    short:           "mock",
    config_file:     "mockspace.toml",
    pin_keys:        pin_keys!("mockspace"),
    engine_crate:    "mockspace",
    cache_namespace: "mockspace",
    default_url:     CANONICAL_URL,
    launcher_crate:  "cargo-mock",
    // `mock_dir`, not renki's conventional `workdir`. The key is a contract
    // with `lib/mock.sh` and with every hook and shell helper that sources it,
    // all of which parse `mock_dir=` and have done since before the launcher
    // was extracted. renki makes the keys fields for exactly this.
    locate:          Some(Locate {
        workdir_key: "mock_dir",
        ..Locate::DEFAULT
    }),
    workdir:         Some(Workdir {
        key:          "mock_dir",
        // at the repo root, `mock` is what almost every consumer uses; in a
        // subdirectory the config's own directory is the workspace.
        root_default: "mock",
    }),
    hooks:           Hooks {
        prepare_repo:      Some(plant_gate),
        engine_args:       Some(lint_rules_from_pin),
        engine_args_local: Some(lint_rules_from_checkout),
        verify_engine_dir: Some(has_lint_rules),
        legacy_pin:        Some(legacy::pin_from_lock_at),
        verify_repo_state: Some(no_retired_alias),
        ..Hooks::NONE
    },
    // Everything renki already answers: the flags, the retention, the skip
    // list, the self-update policy. Spread rather than restated, so a field
    // added to the descriptor arrives as a version bump instead of a build
    // break.
    ..Tool::CONVENTIONS
};

/// The launcher entry, shared by both installed binaries. Each bin is a
/// two-line shim over this.
pub fn run_cli() -> ExitCode {
    // SAFETY: this is the process entry. Nothing else has run, so no other
    // thread exists to observe the environment renki scrubs.
    unsafe { renki::run(&TOOL) }
}

/// The lint-rules dependency, pinned to the same revision the engine was built
/// from.
///
/// A custom-lint cdylib has to link the identical `mockspace-lint-rules`, or
/// its `Box<dyn Lint>` vtables do not match the engine's and crossing the
/// dlopen boundary is undefined. Renamed to the package `mockspace` so the
/// generated crate's dependency spelling does not change.
fn lint_rules_from_pin(resolved: &Resolved) -> Vec<String> {
    let (kind, value) = resolved.git_ref();
    lint_rules_at(&resolved.pin.url, kind, value)
}

/// The dependency text itself, split out from the hook so it can be tested
/// without a resolved pin.
///
/// `Resolved` is renki's and is only built by resolving, which wants a network
/// and a cache directory. Which git ref a given pin resolves to is renki's
/// question and renki answers it; what is left here is the spelling of the
/// dependency, and this is the whole of it.
fn lint_rules_at(url: &str, kind: &str, value: &str) -> Vec<String> {
    vec![
        "--mockspace-lint-rules-dep".to_string(),
        format!(
            "{{ package = \"mockspace-lint-rules\", git = \"{url}\", {kind} = \"{value}\" }}"
        ),
    ]
}

/// The same, for `--engine <path>`, where the revision is a working tree and
/// the dependency is therefore a path to it. Pointing a working tree at a git
/// ref is how the vtables diverge.
fn lint_rules_from_checkout(source: &Path) -> Vec<String> {
    vec![
        "--mockspace-lint-rules-dep".to_string(),
        format!(
            "{{ package = \"mockspace-lint-rules\", path = \"{}\" }}",
            source.join("lint-rules").display()
        ),
    ]
}

/// Refuse an `--engine <path>` with no lint-rules crate, since a custom-lint
/// cdylib built against it would have nothing to link.
fn has_lint_rules(abs: &Path) -> Result<(), String> {
    if abs.join("lint-rules").join("Cargo.toml").is_file() {
        return Ok(());
    }
    Err(format!(
        "--engine {} has no lint-rules crate, so a custom-lint cdylib built against it could \
         not link",
        abs.display()
    ))
}

/// Refuse a repo still carrying the retired `cargo mock` alias.
///
/// Cargo resolves aliases before external subcommands, so a leftover alias
/// shadows this launcher whenever `cargo mock` is typed: the user runs whatever
/// the alias points at rather than the pinned engine, and nothing says so. Per
/// the anomalous-state rule that is an error with guidance, never a silent
/// difference between `mock` and `cargo mock`.
fn no_retired_alias(root: &Path) -> Result<(), String> {
    // Both spellings cargo honours. The refusal covers what the retired
    // bootstrap wrote, which was always repo-local; an alias a user keeps
    // elsewhere is their choice rather than an anomaly of ours.
    for cargo_cfg in [root.join(".cargo").join("config.toml"), root.join(".cargo").join("config")] {
        let Ok(cfg) = std::fs::read_to_string(&cargo_cfg) else {
            continue;
        };
        if legacy_alias_present(&cfg) {
            return Err(format!(
                "a retired `cargo mock` alias sits in {}. Cargo resolves aliases before \
                 external subcommands, so `cargo mock` runs whatever the alias points at \
                 instead of this launcher. Delete the `mock = ...` line, and the [alias] \
                 table if that empties it, then re-run.",
                cargo_cfg.display()
            ));
        }
    }
    Ok(())
}

fn legacy_alias_present(config: &str) -> bool {
    let mut in_alias = false;
    for line in config.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_alias = t == "[alias]";
            continue;
        }
        if in_alias
            && (t.starts_with("mock =")
                || t.starts_with("mock=")
                || t.starts_with("\"mock\" =")
                || t.starts_with("\"mock\"="))
        {
            return true;
        }
    }
    false
}

/// Install the durable hooks and point `core.hooksPath` at them.
///
/// Planted by the launcher rather than the engine, because every way the engine
/// can fail to run also leaves the repo ungated, silently: its build can fail on
/// a bad pin, on no network, or on a compile error in the pinned revision, and
/// it can fail on the repo's own contents. A workspace with no members exited
/// non-zero before reaching any setup. The launcher cannot fail for any of
/// those reasons, so it plants the gate and the engine keeps it current.
///
/// The hook version is shared with the engine through `mockspace-manifest`
/// rather than duplicated: two copies of a version number is how a repo ends up
/// with hooks from one era wired by another.
///
/// Best-effort and quiet on success. A gate that cannot be written is worth
/// reporting, and it must not stop the command the user ran.
fn plant_gate(root: &Path) {
    let Some(dir) = mockspace_manifest::gate::durable_hooks_dir(HOOK_VERSION) else {
        return; // no home directory to write into; nothing to do
    };
    let mut actions = mockspace_manifest::gate::install_durable_hooks(&dir, HOOK_VERSION);
    // The same opt-out the engine honours. Hooks are still written; they are
    // inert files until wired.
    if std::env::var("MOCKSPACE_NO_AUTO_ACTIVATE").is_err() {
        actions.extend(mockspace_manifest::gate::activate(root, &dir));
    }
    for a in actions {
        eprintln!("mock: {a}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retired_alias_is_detected_only_under_its_table() {
        assert!(legacy_alias_present("[alias]\nmock = \"run --quiet\"\n"));
        assert!(legacy_alias_present(
            "[build]\njobs = 4\n[alias]\nmock=\"x\"\n"
        ));
        assert!(legacy_alias_present("[alias]\n\"mock\" = \"x\"\n"));
        // the controls: a `mock` key under another table is not an alias, and a
        // longer name that merely starts with it is a different alias.
        assert!(!legacy_alias_present("[env]\nmock = \"not an alias\"\n"));
        assert!(!legacy_alias_present("[alias]\nmockery = \"other\"\n"));
        assert!(!legacy_alias_present(""));
    }

    #[test]
    fn the_alias_check_reports_the_file_it_found_it_in() {
        let d = tempfile::tempdir().unwrap();
        assert!(no_retired_alias(d.path()).is_ok(), "control: a clean repo");

        std::fs::create_dir(d.path().join(".cargo")).unwrap();
        std::fs::write(
            d.path().join(".cargo/config.toml"),
            "[alias]\nmock = \"x\"\n",
        )
        .unwrap();
        let err = no_retired_alias(d.path()).unwrap_err();
        assert!(err.contains("config.toml"), "{err}");

        // the extensionless legacy spelling cargo also honours
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join(".cargo")).unwrap();
        std::fs::write(d.path().join(".cargo/config"), "[alias]\nmock = \"x\"\n").unwrap();
        assert!(no_retired_alias(d.path()).is_err());
    }

    #[test]
    fn the_lint_dep_carries_the_ref_it_was_given_and_nothing_else() {
        // Which ref a pin resolves to is renki's, and renki tests it. What is
        // this crate's is that the ref it hands over is the one that lands in
        // the dependency, verbatim, with the package rename intact.
        let dep = lint_rules_at(CANONICAL_URL, "tag", "0.0.0-d05");
        assert_eq!(dep[0], "--mockspace-lint-rules-dep");
        assert!(dep[1].contains("tag = \"0.0.0-d05\""), "{}", dep[1]);
        assert!(
            dep[1].contains("package = \"mockspace-lint-rules\""),
            "{}",
            dep[1]
        );
        assert!(dep[1].contains(CANONICAL_URL), "{}", dep[1]);

        // A rev reads the same way, and the kind is not hardcoded anywhere: a
        // builder that always wrote `tag` would pass the case above.
        let dep = lint_rules_at(CANONICAL_URL, "rev", "feedface99c0ffee");
        assert!(dep[1].contains("rev = \"feedface99c0ffee\""), "{}", dep[1]);
        assert!(!dep[1].contains("tag ="), "{}", dep[1]);

        // The url is not hardcoded either, which the two cases above cannot
        // see because both pass the canonical one.
        let dep = lint_rules_at("ssh://git@example.invalid/other.git", "rev", "abc");
        assert!(dep[1].contains("example.invalid"), "{}", dep[1]);
        assert!(!dep[1].contains("hiisi-digital"), "{}", dep[1]);
    }

    #[test]
    fn a_local_engine_gets_a_path_dep_and_never_a_git_one() {
        let dep = lint_rules_from_checkout(Path::new("/tmp/ms"));
        assert_eq!(dep[0], "--mockspace-lint-rules-dep");
        assert!(
            dep[1].contains("path = \"/tmp/ms/lint-rules\""),
            "{}",
            dep[1]
        );
        assert!(!dep[1].contains("git ="), "{}", dep[1]);
    }

    #[test]
    fn an_engine_checkout_without_lint_rules_is_refused() {
        let d = tempfile::tempdir().unwrap();
        assert!(has_lint_rules(d.path()).is_err());
        std::fs::create_dir(d.path().join("lint-rules")).unwrap();
        assert!(
            has_lint_rules(d.path()).is_err(),
            "a bare directory is not a crate"
        );
        std::fs::write(d.path().join("lint-rules/Cargo.toml"), b"[package]\n").unwrap();
        assert!(has_lint_rules(d.path()).is_ok());
    }

    #[test]
    fn every_key_the_engine_reflects_over_is_one_the_launcher_actually_reads() {
        // `mockspace-manifest::ManifestHeader` exists so the engine's config
        // gate knows which top-level keys belong to the launcher and does not
        // report a project's own pin as unknown. renki reads those keys by
        // building the names from `pin_prefix` at runtime, so the two are
        // otherwise independent and can drift apart in silence.
        //
        // This is the join, and it is exhaustive on purpose: the destructuring
        // below stops compiling the moment a field is added to the header,
        // which is the only way anyone finds out that renki was never taught
        // to read it.
        const EVERY_KEY: &str = "\
mock_dir = \"d\"
mockspace_git = \"u\"
mockspace_version = \"1\"
mockspace_rev = \"2\"
mockspace_branch = \"3\"
mockspace_tag = \"4\"
";
        let mockspace_manifest::ManifestHeader {
            mock_dir,
            mockspace_git,
            mockspace_version,
            mockspace_rev,
            mockspace_branch,
            mockspace_tag,
        } = mockspace_manifest::ManifestHeader::parse(EVERY_KEY);
        for (name, got) in [
            ("mock_dir", &mock_dir),
            ("mockspace_git", &mockspace_git),
            ("mockspace_version", &mockspace_version),
            ("mockspace_rev", &mockspace_rev),
            ("mockspace_branch", &mockspace_branch),
            ("mockspace_tag", &mockspace_tag),
        ] {
            assert!(got.is_some(), "control: the fixture does not set `{name}`");
        }

        // the two that are not the pin
        let h = renki::Header::parse(&TOOL, EVERY_KEY);
        assert_eq!(h.workdir, mock_dir, "renki does not read `mock_dir`");
        assert_eq!(h.url, mockspace_git, "renki does not read `mockspace_git`");

        // and the four that are, each read on its own, since a config carrying
        // all four resolves to one of them by precedence and the other three
        // would go unchecked.
        for (key, value, want) in [
            (
                "mockspace_version",
                &mockspace_version,
                renki::Reference::Version("1".into()),
            ),
            (
                "mockspace_rev",
                &mockspace_rev,
                renki::Reference::Rev("2".into()),
            ),
            (
                "mockspace_branch",
                &mockspace_branch,
                renki::Reference::Branch("3".into()),
            ),
            (
                "mockspace_tag",
                &mockspace_tag,
                renki::Reference::Tag("4".into()),
            ),
        ] {
            let alone = format!("{key} = \"{}\"\n", value.as_deref().unwrap());
            assert_eq!(
                renki::Header::parse(&TOOL, &alone).pin,
                Some(want),
                "renki does not read `{key}`"
            );
        }
    }
}
