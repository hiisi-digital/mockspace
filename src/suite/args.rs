//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a `mock test` command line asks for, split into what this module
//! decides, what chooses packages, and what passes through to the runner.

use super::select::Mode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub mode:      Mode,
    pub cache:     bool,
    /// `cargo test` as before, without nextest, selection or cache.
    pub plain:     bool,
    /// Packages named with `-p`; empty means the default members.
    pub packages:  Vec<String>,
    /// `--workspace` or `--all`: every member rather than the default ones.
    pub workspace: bool,
    pub exclude:   Vec<String>,
    /// A filterset the caller passed with `-E`.
    pub filter:    Option<String>,
    /// Every other cargo or nextest argument, passed through as written.
    pub passed:    Vec<String>,
    /// Whatever followed `--`, for the test binaries.
    pub test_args: Vec<String>,
}

impl Invocation {
    /// The flavour a run's history is kept under: the arguments that change
    /// what is compiled or how it runs, without the ones choosing packages.
    /// As written: sorting the words would make `--features a --target b`
    /// and `--features b --target a` one flavour.
    pub fn flavour(&self) -> String {
        self.passed.join(" ")
    }

    /// Whether the caller narrowed which tests run within a package, so a
    /// package that passed has not passed whole.
    pub fn narrowed(&self) -> bool {
        self.filter.is_some()
            || !self.test_args.is_empty()
            || self.passed.iter().any(|a| {
                a.starts_with("--run-ignored")
                    || a.starts_with("--partition")
                    || a == "--ignored"
                    || a == "--include-ignored"
            })
    }
}

pub fn parse(args: &[&str]) -> Invocation {
    let mut inv = Invocation {
        mode:      Mode::Changed,
        cache:     true,
        plain:     false,
        packages:  Vec::new(),
        workspace: false,
        exclude:   Vec::new(),
        filter:    None,
        passed:    Vec::new(),
        test_args: Vec::new(),
    };
    let mut it = args.iter().copied();
    while let Some(a) = it.next() {
        if a == "--" {
            inv.test_args = it.by_ref().map(str::to_string).collect();
            break;
        }
        // The value of a flag written as two words, or as `--flag=value`.
        let mut value = |long: &str, short: Option<&str>| -> Option<String> {
            if a == long || Some(a) == short {
                return it.next().map(str::to_string);
            }
            a.strip_prefix(long)
                .and_then(|r| r.strip_prefix('='))
                .map(str::to_string)
        };
        match a {
            "--full" => inv.mode = Mode::Full,
            "--cheap" => inv.mode = Mode::Cheap,
            "--no-cache" => inv.cache = false,
            "--plain" => inv.plain = true,
            "--workspace" | "--all" => inv.workspace = true,
            _ => {
                if let Some(p) = value("--package", Some("-p")) {
                    inv.packages.push(p);
                } else if let Some(p) = a.strip_prefix("-p").filter(|p| !p.is_empty()) {
                    inv.packages.push(p.to_string());
                } else if let Some(x) = value("--exclude", None) {
                    inv.exclude.push(x);
                } else if let Some(e) = value("--filterset", Some("-E")) {
                    inv.filter = Some(match inv.filter.take() {
                        Some(prev) => format!("({prev}) | ({e})"),
                        None => e,
                    });
                } else {
                    inv.passed.push(a.to_string());
                }
            },
        }
    }
    inv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_run_is_changed_with_the_cache() {
        let i = parse(&[]);
        assert_eq!((i.mode, i.cache, i.plain), (Mode::Changed, true, false));
        assert_eq!(i.flavour(), "");
        assert!(!i.narrowed());
    }

    #[test]
    fn its_own_flags_are_taken_and_the_rest_passed_through() {
        let i = parse(&[
            "--full",
            "--no-cache",
            "-p",
            "kaski-web",
            "--features",
            "editor",
            "--release",
            "--",
            "--skip",
            "drawn",
        ]);
        assert_eq!((i.mode, i.cache), (Mode::Full, false));
        assert_eq!(i.packages, vec!["kaski-web"]);
        assert_eq!(i.passed, vec!["--features", "editor", "--release"]);
        assert_eq!(i.test_args, vec!["--skip", "drawn"]);
        assert!(i.narrowed(), "a test filter after `--` narrows the run");
    }

    #[test]
    fn every_spelling_of_a_package_is_a_package() {
        let i = parse(&[
            "-p",
            "a",
            "-pb",
            "--package",
            "c",
            "--package=d",
            "--exclude",
            "e",
            "--workspace",
        ]);
        assert_eq!(i.packages, vec!["a", "b", "c", "d"]);
        assert_eq!(i.exclude, vec!["e"]);
        assert!(i.workspace);
        assert!(i.passed.is_empty());
    }

    /// The flavour keeps a features run apart from a plain one, and does not
    /// care which packages were chosen.
    #[test]
    fn the_flavour_is_what_changes_the_build() {
        let a = parse(&["-p", "x", "--features", "editor"]).flavour();
        let b = parse(&["--features", "editor", "--workspace"]).flavour();
        assert_eq!(a, b);
        assert_ne!(a, parse(&[]).flavour());
    }

    #[test]
    fn filtersets_combine_and_narrow() {
        let i = parse(&["-E", "test(a)", "--filterset=test(b)"]);
        assert_eq!(i.filter.as_deref(), Some("(test(a)) | (test(b))"));
        assert!(i.narrowed());
        assert!(parse(&["--run-ignored", "all"]).narrowed());
    }

    #[test]
    fn cheap_and_plain_are_modes_of_their_own() {
        assert_eq!(parse(&["--cheap"]).mode, Mode::Cheap);
        assert!(parse(&["--plain"]).plain);
    }
}
