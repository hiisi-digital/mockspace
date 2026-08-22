//! A skill's frontmatter gets variable substitution, through the real path.
//!
//! The persona phase already substitutes across the whole template before
//! splitting, and says so in a comment. The skill phase rendered the body and
//! parsed the frontmatter off the raw text, so `description` was the one line in
//! a skill that could not name the project. It is also the line an agent reads
//! to decide whether the skill applies.
//!
//! Written end to end rather than as a unit test because the existing
//! frontmatter test calls `substitute_vars` and `split_frontmatter` itself, so
//! it would pass with the production path broken, which is exactly what
//! happened.

use std::fs;
use std::path::Path;
use std::process::Command;

fn fixture(root: &Path, description: &str) {
    Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status()
        .unwrap();
    let mock = root.join("mock");
    let skill = mock.join("agent/skills/probe");
    fs::create_dir_all(&skill).unwrap();
    fs::write(
        mock.join("Cargo.toml"),
        "[workspace]\nmembers = []\nresolver = \"2\"\n",
    )
    .unwrap();
    fs::write(
        mock.join("mockspace.toml"),
        "project_name = \"fixture-project\"\ncrate_prefix = \"fx\"\n",
    )
    .unwrap();
    fs::write(
        skill.join("SKILL.md.tmpl"),
        format!("---\nskill_name: probe\nskill_description: {description}\n---\n\nBody names {{{{project_name}}}} too.\n"),
    )
    .unwrap();
}

fn generated(root: &Path) -> String {
    Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .current_dir(root.join("mock"))
        .output()
        .unwrap();
    fs::read_to_string(root.join(".claude/skills/probe/SKILL.md")).unwrap_or_default()
}

#[test]
fn a_skills_description_names_the_project() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), "Reviews {{project_name}} conventions");
    let out = generated(tmp.path());
    assert!(
        out.contains("description: Reviews fixture-project conventions"),
        "the description did not get substitution:\n{out}"
    );
    assert!(
        !out.contains("{{"),
        "a placeholder survived anywhere in the skill:\n{out}"
    );
}

/// The control: a description with no variable in it is passed through
/// unchanged, so the test above is not equally consistent with a renderer that
/// rewrites descriptions.
#[test]
fn a_description_with_no_variable_is_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), "Reviews conventions");
    let out = generated(tmp.path());
    assert!(
        out.contains("description: Reviews conventions"),
        "an ordinary description was rewritten:\n{out}"
    );
}
