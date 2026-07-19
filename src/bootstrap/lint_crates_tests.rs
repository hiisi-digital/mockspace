#![allow(unused_imports)]
use super::*;

fn write_toml(contents: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mockspace.toml");
    fs::write(&path, contents).unwrap();
    (dir, path)
}

#[test]
fn missing_file_returns_empty() {
    let result = parse_lint_crates(Path::new("/definitely/does/not/exist"));
    assert!(result.is_empty());
}

#[test]
fn gen_pre_push_passes_bash_syntax_check() {
    // Run `bash -n` on the generated script to catch syntax errors
    // (unbalanced quotes, missing fi/done, format-string slips). Does
    // not execute the script, just parses it.
    let script = gen_pre_push("mock", Path::new("/dev/null"));
    let mut path = std::env::temp_dir();
    path.push(format!("mockspace_pre_push_test_{}.sh", std::process::id()));
    std::fs::write(&path, &script).unwrap();
    let output = std::process::Command::new("bash")
        .arg("-n")
        .arg(&path)
        .output()
        .expect("bash -n");
    let _ = std::fs::remove_file(&path);
    assert!(
        output.status.success(),
        "bash -n rejected the generated pre-push hook:\n{}\n--- script ---\n{}",
        String::from_utf8_lossy(&output.stderr),
        script
    );
}

#[test]
fn gen_pre_push_includes_scope_branch() {
    // Sanity check: the generated script names the new-branch /
    // changed-crates scopes the way the hook expects.
    let script = gen_pre_push("mock", Path::new("/dev/null"));
    assert!(
        script.contains("CHANGED_CRATES"),
        "expected CHANGED_CRATES var in generated pre-push hook"
    );
    assert!(
        script.contains("NEW_BRANCH"),
        "expected NEW_BRANCH var in generated pre-push hook"
    );
    assert!(
        script.contains("--scope"),
        "expected --scope flag in generated pre-push hook"
    );
}

#[test]
fn absent_section_returns_empty() {
    let (_dir, path) = write_toml("project_name = \"foo\"\n");
    assert!(parse_lint_crates(&path).is_empty());
}

#[test]
fn inline_table_form() {
    let toml = r#"
[lint-crates]
foo-pack = { path = "../foo-pack" }
bar-pack = { git = "https://example.com/bar.git", branch = "main" }
"#;
    let (_dir, path) = write_toml(toml);
    let result = parse_lint_crates(&path);
    assert_eq!(result.len(), 2);
    let names: Vec<&str> = result.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"foo-pack"));
    assert!(names.contains(&"bar-pack"));
    for (_, spec) in &result {
        assert!(spec.starts_with('{') && spec.ends_with('}'), "got: {spec}");
    }
}

#[test]
fn version_string_form() {
    let toml = r#"
[lint-crates]
foo-pack = "0.1.2"
"#;
    let (_dir, path) = write_toml(toml);
    let result = parse_lint_crates(&path);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "foo-pack");
    assert_eq!(result[0].1, "\"0.1.2\"");
}

#[test]
fn sub_table_form_rendered_as_inline() {
    let toml = r#"
[lint-crates.foo-pack]
path = "../foo-pack"
version = "0.1"
"#;
    let (_dir, path) = write_toml(toml);
    let result = parse_lint_crates(&path);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "foo-pack");
    let spec = &result[0].1;
    assert!(spec.starts_with('{'), "got: {spec}");
    assert!(spec.contains("path"), "got: {spec}");
    assert!(spec.contains("version"), "got: {spec}");
}
