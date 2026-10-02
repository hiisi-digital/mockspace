//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Unit tests for a maker's writes rooted at an argument: the declaration
//! refused at registration, the value refused before the run, and the audit
//! over the resolved patterns.

use super::*;

const SOURCE: ArgSpec = ArgSpec {
    name:        "source",
    required:    true,
    description: "the directory of svgs",
};
const OUTPUT: ArgSpec = ArgSpec {
    name:        "output",
    required:    true,
    description: "the module to write, without `.rs`",
};
const EXTRA: ArgSpec = ArgSpec {
    name:        "extra",
    required:    false,
    description: "a second module, written only when given",
};

/// A maker shaped like a generator whose output path is an argument: it
/// writes `<output>.rs` and everything under the directory of that stem.
struct Bake {
    writes: &'static [&'static str],
    roots:  &'static [ArgRoot],
}
impl Tool for Bake {
    fn name(&self) -> &'static str {
        "bake"
    }

    fn description(&self) -> &'static str {
        "bake icons into a module"
    }

    fn purpose(&self) -> Purpose {
        Purpose::Make {
            writes: self.writes,
            roots:  self.roots,
        }
    }

    fn args(&self) -> &[ArgSpec] {
        &[SOURCE, OUTPUT, EXTRA]
    }

    fn value_flags(&self) -> &[&'static str] {
        &["--theme"]
    }

    fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
        ToolReport::reported("", 1)
    }
}

const BAKE: Bake = Bake {
    writes: &["{output}.rs", "{output}/**", "{extra}.rs", "assets/icons/index.md"],
    roots:  &[
        ArgRoot {
            arg:   "output",
            under: "src/icons/*",
        },
        ArgRoot {
            arg:   "extra",
            under: "src/extra/**/*",
        },
    ],
};

fn written(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| p.to_string()).collect()
}

fn faults_of(writes: &'static [&'static str], roots: &'static [ArgRoot]) -> Vec<String> {
    contract_faults(
        &Bake {
            writes,
            roots,
        },
        None,
    )
}

#[test]
fn an_argument_under_its_root_resolves_and_its_writes_pass_the_audit() {
    assert_eq!(contract_faults(&BAKE, None), Vec::<String>::new());
    let args = ["--theme", "dark", "-q", "svg", "./src/icons/ui/"];
    assert_eq!(
        resolve_writes(&BAKE, &args),
        Ok(written(&[
            "src/icons/ui.rs",
            "src/icons/ui/**",
            "assets/icons/index.md"
        ])),
        "an optional argument not given resolves to nothing; `dark` is the flag's value"
    );
    assert_eq!(
        maker_faults(
            &BAKE,
            &ToolReport::reported("", 1),
            &args,
            &written(&["src/icons/ui.rs", "src/icons/ui/home.rs", "assets/icons/index.md"]),
        ),
        Vec::<String>::new()
    );
    // the optional one, given, at a depth its root admits
    assert_eq!(
        resolve_writes(&BAKE, &["svg", "src/icons/ui", "src/extra/a/b"]),
        Ok(written(&[
            "src/icons/ui.rs",
            "src/icons/ui/**",
            "src/extra/a/b.rs",
            "assets/icons/index.md"
        ]))
    );
}

#[test]
fn an_argument_escaping_its_root_is_refused() {
    // The case that must fail: a value outside its root, or one that would
    // widen the pattern it is substituted into, never resolves.
    for (value, why) in [
        ("/etc/icons", "leaves the repository root"),
        ("../sibling/icons", "leaves the repository root"),
        ("src/icons/../../escape", "leaves the repository root"),
        ("src/icons/.git", "has a literal `.git` segment"),
        ("src/icons/*", "a glob character"),
        ("src/icons/u?", "a glob character"),
        ("src/icons/[ab]", "a glob character"),
        ("src//icons", "an empty segment"),
        ("", "is empty"),
        ("src/other/ui", "is not under `src/icons/*`"),
        ("src/icons/ui/deeper", "is not under `src/icons/*`"),
        ("src/icons", "is not under `src/icons/*`"),
    ] {
        let got = resolve_writes(&BAKE, &["svg", value]);
        let Err(refused) = got else {
            panic!("`{value}` must be refused, got {got:?}");
        };
        assert_eq!(refused.len(), 1, "`{value}`: {refused:?}");
        assert!(refused[0].contains(why), "`{value}`: {refused:?}");
        assert!(
            refused[0].contains("`output`"),
            "names the argument: {refused:?}"
        );
    }
}

#[test]
fn a_write_outside_the_resolved_pattern_is_a_fault() {
    // The case that must fail: the declaration admits `{output}/**`, and on
    // this command line that is `src/icons/ui/**` only. A sibling module
    // under the same root was not asked for, and an absent optional argument
    // admits nothing.
    let args = ["svg", "src/icons/ui"];
    let found = maker_faults(
        &BAKE,
        &ToolReport::reported("", 1),
        &args,
        &written(&[
            "src/icons/ui.rs",
            "src/icons/other.rs",
            "src/icons/other/a.rs",
            "src/extra/x.rs",
        ]),
    );
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found[0].contains("`src/icons/other.rs`"), "{found:?}");
    assert!(
        found[0].contains("`src/icons/ui/**`"),
        "names the resolved pattern: {found:?}"
    );
    assert!(found[2].contains("`src/extra/x.rs`"), "{found:?}");
}

#[test]
fn a_declaration_naming_an_unknown_or_unrooted_argument_is_refused() {
    // The case that must fail: a pattern naming an argument nothing bounds.
    let output_root: &'static [ArgRoot] = &[ArgRoot {
        arg:   "output",
        under: "src/icons/*",
    }];
    for (writes, roots, why) in [
        (
            &["{out}.rs"][..],
            output_root,
            "names argument `out`, which the tool does not declare",
        ),
        (
            &["{source}.rs"][..],
            output_root,
            "declares no root for argument `source`",
        ),
        (
            &["gen/{output}.rs"][..],
            output_root,
            "an argument only at its start",
        ),
        (
            &["{output}/{output}.rs"][..],
            output_root,
            "an argument only at its start",
        ),
        (&["{}.rs"][..], output_root, "names no argument"),
        (&["{output.rs"][..], output_root, "an unclosed `{`"),
        (
            &["{output}/../x"][..],
            output_root,
            "leaves the repository root",
        ),
        (
            &["{output}/.git/x"][..],
            output_root,
            "has a literal `.git` segment",
        ),
        (
            &["{output}.rs"][..],
            &[
                ArgRoot {
                    arg:   "output",
                    under: "src/icons/*",
                },
                ArgRoot {
                    arg:   "nope",
                    under: "src/*",
                },
            ][..],
            "root for argument `nope`, which the tool does not declare",
        ),
        (
            &["{output}.rs"][..],
            &[
                ArgRoot {
                    arg:   "output",
                    under: "src/icons/*",
                },
                ArgRoot {
                    arg:   "source",
                    under: "svg/*",
                },
            ][..],
            "root for argument `source`, which no declared write names",
        ),
        (
            &["{output}.rs"][..],
            &[
                ArgRoot {
                    arg:   "output",
                    under: "src/icons/*",
                },
                ArgRoot {
                    arg:   "output",
                    under: "src/*",
                },
            ][..],
            "declares a root for argument `output` more than once",
        ),
        (
            &["{output}.rs"][..],
            &[ArgRoot {
                arg:   "output",
                under: "**/*",
            }][..],
            "is only wildcards",
        ),
        (
            &["{output}.rs"][..],
            &[ArgRoot {
                arg:   "output",
                under: "../elsewhere/*",
            }][..],
            "leaves the repository root",
        ),
    ] {
        // every case also carries a sound `{output}.rs`, so the root it bounds is
        // used and the one fault is the shape under test
        let mut all = vec!["{output}.rs"];
        all.extend_from_slice(writes);
        let writes: &'static [&'static str] = Box::leak(all.into_boxed_slice());
        let roots: &'static [ArgRoot] = Box::leak(roots.to_vec().into_boxed_slice());
        let found = faults_of(writes, roots);
        assert_eq!(found.len(), 1, "{writes:?} {roots:?}: {found:?}");
        assert!(found[0].contains(why), "{writes:?} {roots:?}: {found:?}");
    }
}

#[test]
fn the_purpose_renders_each_argument_with_its_root() {
    assert_eq!(
        BAKE.purpose().describe(),
        "make, writes `{output}.rs`, `{output}/**`, `{extra}.rs`, `assets/icons/index.md`, \
         with `output` under `src/icons/*` and `extra` under `src/extra/**/*`"
    );
}

#[test]
fn a_git_segment_is_refused_in_any_case() {
    // The case that must fail: on a case-insensitive filesystem `.GIT` is
    // the repository's `.git`, so a check on the exact spelling grants it.
    for (writes, why) in [
        (&[".GIT/hooks/*"][..], "has a literal `.git` segment"),
        (&["vendor/.Git/config"][..], "has a literal `.git` segment"),
        (&["{output}/.GIT/x"][..], "has a literal `.git` segment"),
    ] {
        let mut all = vec!["{output}.rs"];
        all.extend_from_slice(writes);
        let writes: &'static [&'static str] = Box::leak(all.into_boxed_slice());
        let found = faults_of(writes, BAKE.roots);
        let found: Vec<&String> = found.iter().filter(|f| !f.contains("extra")).collect();
        assert_eq!(found.len(), 1, "{writes:?}: {found:?}");
        assert!(found[0].contains(why), "{writes:?}: {found:?}");
    }
    for value in ["src/icons/.GIT", "src/icons/.Git"] {
        let got = resolve_writes(&BAKE, &["svg", value]);
        let Err(refused) = got else {
            panic!("`{value}` must be refused, got {got:?}");
        };
        assert!(
            refused[0].contains("has a literal `.git` segment"),
            "{refused:?}"
        );
    }
}

#[test]
fn a_bracket_in_a_fixed_pattern_is_refused() {
    // Git's glob pathspec reads `[ab]` as a class and the audit's matcher
    // reads it literally, so the ignored outputs observed and the writes
    // admitted would be two different sets.
    for pattern in ["gen/[ab].rs", "gen/a]b.rs", "{output}/[x]"] {
        let writes: &'static [&'static str] =
            Box::leak(vec!["{output}.rs", pattern].into_boxed_slice());
        let found = faults_of(writes, BAKE.roots);
        let found: Vec<&String> = found.iter().filter(|f| !f.contains("extra")).collect();
        assert_eq!(found.len(), 1, "`{pattern}`: {found:?}");
        assert!(found[0].contains("`[` or `]`"), "`{pattern}`: {found:?}");
    }
}
