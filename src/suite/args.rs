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
    /// Which crates the caller asked to be built with symbols.
    pub debug:     DebugAsk,
    /// `--benches`: run the benches tree whether or not its inputs moved, and
    /// beside a `-p`.
    pub benches:   bool,
}

/// `--debug`, `--debug=<crate,...>` and `--no-debug`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DebugAsk {
    /// A bare `--debug`: the crates named with `-p`, or where none is, every
    /// crate that runs.
    pub bare:  bool,
    /// Crates named with `--debug=`. Each is also chosen as with `-p`.
    pub names: Vec<String>,
    /// `--no-debug`: no crate gets symbols, a failed one included.
    pub off:   bool,
}

impl DebugAsk {
    /// Whether symbols were asked for by name or by `--debug`, as against left
    /// to the record of what failed.
    pub fn asked(&self) -> bool {
        !self.off && (self.bare || !self.names.is_empty())
    }
}

/// The flags this module reads and no cargo understands, which are kept from
/// every cargo command line `mock test` builds. Only ahead of a `--`, which
/// is the test binaries'.
pub fn is_own_flag(a: &str) -> bool {
    matches!(
        a,
        "--full" | "--cheap" | "--no-cache" | "--plain" | "--debug" | "--no-debug" | "--benches"
    ) || a.starts_with("--debug=")
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
        debug:     DebugAsk::default(),
        benches:   false,
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
            "--benches" => inv.benches = true,
            "--debug" => {
                inv.debug.bare = true;
                inv.debug.off = false;
            },
            "--no-debug" => inv.debug.off = true,
            "--workspace" | "--all" => inv.workspace = true,
            _ => {
                if let Some(list) = a.strip_prefix("--debug=") {
                    // A crate named here is chosen as well, so that asking for
                    // its symbols runs it.
                    for n in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                        inv.debug.names.push(n.to_string());
                        inv.packages.push(n.to_string());
                    }
                    inv.debug.off = false;
                } else if let Some(p) = value("--package", Some("-p")) {
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
    // Symbols asked for on crates that were named are asked for to trace
    // something in them, which a pass that still stands would leave unrun.
    if inv.mode == Mode::Changed && inv.debug.asked() && !inv.packages.is_empty() {
        inv.mode = Mode::Full;
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

    #[test]
    fn a_bare_run_asks_for_no_symbols() {
        let i = parse(&[]);
        assert_eq!(i.debug, DebugAsk::default());
        assert!(!i.debug.asked());
    }

    /// `-p x --debug` is a crate asked about with symbols: it runs, passed or
    /// not, and the flag is none of cargo's.
    #[test]
    fn debug_on_a_named_crate_runs_it_in_full() {
        let i = parse(&["-p", "x", "--debug"]);
        assert!(i.debug.bare && i.debug.asked());
        assert_eq!(i.packages, vec!["x"]);
        assert_eq!(i.mode, Mode::Full);
        assert!(i.passed.is_empty(), "{:?}", i.passed);
        // Nothing is named, so what runs is still decided by what changed.
        assert_eq!(parse(&["--debug"]).mode, Mode::Changed);
    }

    #[test]
    fn debug_with_names_chooses_them_as_well() {
        let i = parse(&["--debug=a,b", "--debug=c"]);
        assert_eq!(i.debug.names, vec!["a", "b", "c"]);
        assert_eq!(i.packages, vec!["a", "b", "c"]);
        assert_eq!(i.mode, Mode::Full);
        assert!(!i.debug.bare);
        assert!(i.passed.is_empty());
        // A trailing comma and a space name nothing.
        assert_eq!(parse(&["--debug=a, ,"]).debug.names, vec!["a"]);
    }

    /// An explicit `--cheap` stays what it was written as.
    #[test]
    fn debug_does_not_widen_a_cheap_run() {
        assert_eq!(parse(&["--cheap", "-p", "x", "--debug"]).mode, Mode::Cheap);
    }

    #[test]
    fn no_debug_wins_over_an_earlier_ask_and_loses_to_a_later_one() {
        assert!(parse(&["--debug", "--no-debug"]).debug.off);
        assert!(!parse(&["--debug", "--no-debug"]).debug.asked());
        assert!(!parse(&["--no-debug", "--debug"]).debug.off);
        assert!(parse(&["--no-debug", "--debug"]).debug.asked());
        // It does not widen the run either.
        assert_eq!(parse(&["-p", "x", "--no-debug"]).mode, Mode::Changed);
    }

    /// A `--debug` after `--` is the test binary's, not ours, so it is neither
    /// taken here nor dropped from the command line by [`is_own_flag`]'s caller.
    #[test]
    fn a_debug_flag_after_the_separator_is_the_test_binarys() {
        let i = parse(&["--", "--debug"]);
        assert_eq!(i.test_args, vec!["--debug"]);
        assert!(!i.debug.asked());
    }

    #[test]
    fn benches_is_ours_and_is_off_unless_asked() {
        assert!(!parse(&[]).benches);
        assert!(parse(&["--benches"]).benches);
        assert!(parse(&["-p", "x", "--benches"]).benches);
        // After the separator it is the test binaries'.
        assert!(!parse(&["--", "--benches"]).benches);
    }

    #[test]
    fn the_flags_read_here_are_the_flags_that_are_not_cargos() {
        for f in [
            "--full",
            "--cheap",
            "--no-cache",
            "--plain",
            "--debug",
            "--no-debug",
            "--benches",
            "--debug=a",
        ] {
            assert!(is_own_flag(f), "{f}");
        }
        for f in ["--features", "--release", "-p", "--debugger", "--debug-assertions", "x"] {
            assert!(!is_own_flag(f), "{f}");
        }
    }
}
