//! End-to-end agent-render smoke test against ClaudePlatform + CopilotPlatform.

use std::fs;

use mockspace_template::{
    AgentRenderer, ClaudePlatform, CopilotPlatform, Platform, TemplateEnv,
};
use serde::Serialize;
use tempfile::tempdir;

#[derive(Serialize)]
struct Ctx {
    project_name: String,
}

#[test]
fn agent_renderer_writes_claude_rules() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    fs::create_dir_all(src.join("rules")).unwrap();
    fs::write(
        src.join("rules/example.md.tmpl"),
        "# {{ project_name }} rules\n",
    )
    .unwrap();

    let mut env = TemplateEnv::new();
    let mut renderer = AgentRenderer::new(&mut env, ClaudePlatform, src, dst.clone());
    let report = renderer
        .render_all(&Ctx {
            project_name: "test-proj".into(),
        })
        .unwrap();

    assert_eq!(report.entries.len(), 1);
    let written = dst.join(".claude/rules/example.md");
    let contents = fs::read_to_string(&written).unwrap();
    // minijinja trims trailing newlines on render; the substance is what we care about.
    assert!(contents.starts_with("# test-proj rules"));
}

#[test]
fn agent_renderer_writes_copilot_instructions() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    fs::create_dir_all(src.join("rules")).unwrap();
    fs::write(
        src.join("rules/example.md.tmpl"),
        "# {{ project_name }} rules\n",
    )
    .unwrap();

    let mut env = TemplateEnv::new();
    let mut renderer = AgentRenderer::new(&mut env, CopilotPlatform, src, dst.clone());
    renderer
        .render_all(&Ctx {
            project_name: "test-proj".into(),
        })
        .unwrap();

    let written = dst.join(".github/instructions/example.instructions.md");
    assert!(written.exists(), "copilot output should be written");
}

#[test]
fn claude_frontmatter_uses_paths() {
    let claude = ClaudePlatform;
    let fm = claude.frontmatter(&["**/*.rs", "**/*.md"]);
    assert!(fm.contains("paths:"));
    assert!(fm.contains("**/*.rs"));
    assert!(fm.contains("**/*.md"));
}

#[test]
fn copilot_frontmatter_uses_apply_to() {
    let copilot = CopilotPlatform;
    let fm = copilot.frontmatter(&["**/*.rs", "**/*.md"]);
    assert!(fm.contains("applyTo:"));
    assert!(fm.contains("**/*.rs,**/*.md"));
}

#[test]
fn claude_hook_helpers_have_repo_root_substituted() {
    let claude = ClaudePlatform;
    let body = claude.hook_helpers(std::path::Path::new("/tmp/foo"));
    assert!(body.contains("/tmp/foo"));
    assert!(!body.contains("{{REPO_ROOT}}"));
}
