//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------


//! A test that writes a script and runs it writes it with `write_executable`.
//!
//! A plain write followed by a `chmod` leaves the file open for writing in the
//! window where another thread of the test process forks, and Linux then
//! refuses the exec with "Text file busy". The helper in `tests/common/exec.rs`
//! is the one writer that avoids it, so what is checked here is the spelling of
//! the other way: a permission set on a file by `set_permissions`,
//! `from_mode`, `set_mode`, an `OpenOptionsExt` mode or a `chmod` command, in
//! test code.
//!
//! Test code is every file under a `tests/` directory, every source file named
//! `tests.rs` or `*_tests.rs`, and the part of any other source file that
//! follows its first `#[cfg(test)]` line. That last is a convention (the test
//! module closes the file) and not a parse, so a test module placed ahead of
//! production code is read as production; the files named in `EXEMPT` are the
//! packages that cannot take the helper.

use std::path::{Path, PathBuf};

/// The spellings that give a file its execute bit, or any mode at all.
const SPELLINGS: &[&str] = &[
    "set_permissions",
    "from_mode",
    "set_mode",
    "OpenOptionsExt",
    "\"chmod\"",
    "chmod ",
];

/// Files that may set a mode, with the reason each cannot use the helper.
const EXEMPT: &[(&str, &str)] = &[
    (
        "tests/common/exec.rs",
        "it is the helper",
    ),
    (
        "tests/an_executable_a_test_writes_goes_through_the_one_helper.rs",
        "it names the spellings it looks for",
    ),
    (
        "mockspace-manifest/src/follow_up.rs",
        "a package that publishes ships its tests, and a path out of the package root does \
         not exist in the tarball",
    ),
];

/// Whether the item that follows a `#[cfg(test)]` is an inline module, as against
/// a `mod name;` declaration or a `use`, which carry no test code of their own.
fn opens_a_module(rest: &[&str]) -> bool {
    rest.iter()
        .map(|l| l.trim())
        .find(|l| !l.starts_with("#["))
        .is_some_and(|l| l.contains("mod ") && l.ends_with('{'))
}

/// The lines of `text` that are test code, given the file's own name.
fn test_lines<'a>(rel: &str, text: &'a str) -> Vec<(usize, &'a str)> {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let whole = rel.starts_with("tests/")
        || rel.contains("/tests/")
        || name == "tests.rs"
        || name.ends_with("_tests.rs");
    let lines: Vec<&str> = text.lines().collect();
    let mut inside = whole;
    let mut out = Vec::new();
    for (i, line) in lines.iter().copied().enumerate() {
        if !inside && line.trim_start().starts_with("#[cfg(test)]") && opens_a_module(&lines[i + 1 ..]) {
            inside = true;
        }
        if inside {
            out.push((i + 1, line));
        }
    }
    out
}

/// Every `file:line` in `text` that sets a mode, for a file at `rel`.
fn violations(rel: &str, text: &str) -> Vec<String> {
    test_lines(rel, text)
        .into_iter()
        .filter(|(_, line)| {
            let code = line.trim_start();
            !code.starts_with("//") && SPELLINGS.iter().any(|s| line.contains(s))
        })
        .map(|(n, line)| format!("{rel}:{n}: {}", line.trim()))
        .collect()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            // `mock/` is its own workspace, `target/` is built output.
            if !matches!(name.as_ref(), "target" | "mock" | ".git" | ".worktrees") {
                rust_files(&path, out);
            }
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_test_sets_a_mode_except_through_the_one_helper() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    assert!(files.len() > 40, "the walk found {} files; it is looking in the wrong place", files.len());
    let mut found = Vec::new();
    for path in &files {
        let rel = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        if EXEMPT.iter().any(|(e, _)| *e == rel) {
            continue;
        }
        found.extend(violations(&rel, &std::fs::read_to_string(path).unwrap()));
    }
    assert!(
        found.is_empty(),
        "these set a mode in test code; write the script with `write_executable` from \
         `tests/common/exec.rs` (`crate::test_exec` in the lib tests):\n{}",
        found.join("\n")
    );
}

#[test]
fn every_exempt_file_exists_and_still_needs_the_exemption() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for (rel, why) in EXEMPT {
        let text = std::fs::read_to_string(root.join(rel))
            .unwrap_or_else(|e| panic!("{rel} is exempt ({why}) and cannot be read: {e}"));
        let names_one = SPELLINGS.iter().any(|s| text.contains(s));
        assert!(names_one, "{rel} is exempt ({why}) but sets no mode; drop the exemption");
    }
}

#[test]
fn the_plain_write_then_chmod_is_flagged_in_each_place_test_code_lives() {
    let plain = "std::fs::write(&p, body).unwrap();\n\
                 std::fs::set_permissions(&p, Permissions::from_mode(0o755)).unwrap();\n";
    // a file under tests/
    assert_eq!(violations("tests/x.rs", plain).len(), 1);
    // a lib test file by name
    assert_eq!(violations("src/a/b_tests.rs", plain).len(), 1);
    assert_eq!(violations("src/a/tests.rs", plain).len(), 1);
    // the test module at the end of a production file, and not the code above it
    let mixed = format!("fn prod() {{ {plain} }}\n#[cfg(test)]\nmod t {{\n{plain}}}\n");
    let hits = violations("src/a/b.rs", &mixed);
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert!(hits[0].starts_with("src/a/b.rs:7:"), "{hits:?}");
    // a `mod name;` under `#[cfg(test)]` opens nothing
    let declared = format!("#[cfg(test)]\nmod t;\n{plain}");
    assert!(violations("src/a/b.rs", &declared).is_empty());
    // another spelling of the same thing
    assert_eq!(violations("tests/x.rs", "Command::new(\"chmod\").arg(\"+x\");").len(), 1);
    assert_eq!(violations("tests/x.rs", "let mut p = m.permissions(); p.set_mode(0o755);").len(), 1);
}

#[test]
fn the_helper_and_production_code_are_not_flagged() {
    let plain = "std::fs::set_permissions(&p, Permissions::from_mode(0o755)).unwrap();\n";
    assert!(violations("src/a/b.rs", plain).is_empty(), "production code sets modes");
    assert!(violations("tests/x.rs", "common::write_executable(&p, body);\n").is_empty());
    assert!(violations("tests/x.rs", "// set_permissions would race\n").is_empty());
}
