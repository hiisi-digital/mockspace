//! Smoke tests for the template engine.

use mockspace_template::{TemplateEnv, RenderError};
use serde::Serialize;

#[derive(Serialize)]
struct Ctx {
    project_name: String,
    version: String,
}

#[test]
fn render_simple_substitution() {
    let env = TemplateEnv::new();
    let ctx = Ctx {
        project_name: "homma".into(),
        version: "0.1.0".into(),
    };
    let out = env
        .render_str("# {{ project_name }} v{{ version }}", &ctx)
        .unwrap();
    assert_eq!(out, "# homma v0.1.0");
}

#[test]
fn registered_template_renders() {
    let mut env = TemplateEnv::new();
    env.add_template("greeting", "Hello {{ project_name }}!").unwrap();
    let ctx = Ctx {
        project_name: "world".into(),
        version: "1".into(),
    };
    let t = env.get_template("greeting").unwrap();
    assert_eq!(t.render(&ctx).unwrap(), "Hello world!");
}

#[test]
fn missing_template_errors() {
    let env = TemplateEnv::new();
    match env.get_template("nope") {
        Err(RenderError::TemplateNotFound(name)) => assert_eq!(name, "nope"),
        Err(other) => panic!("expected TemplateNotFound, got: {other:?}"),
        Ok(_) => panic!("expected TemplateNotFound, got Ok"),
    }
}

#[test]
fn strict_undefined_errors() {
    let env = TemplateEnv::new();
    let ctx = Ctx {
        project_name: "x".into(),
        version: "1".into(),
    };
    let err = env.render_str("{{ no_such_field }}", &ctx).unwrap_err();
    assert!(matches!(err, RenderError::Minijinja(_)));
}

#[test]
fn loop_and_conditional() {
    let env = TemplateEnv::new();
    #[derive(Serialize)]
    struct ListCtx {
        items: Vec<String>,
        emit: bool,
    }
    let ctx = ListCtx {
        items: vec!["a".into(), "b".into(), "c".into()],
        emit: true,
    };
    let out = env
        .render_str(
            "{% if emit %}{% for x in items %}{{ x }}{% endfor %}{% endif %}",
            &ctx,
        )
        .unwrap();
    assert_eq!(out, "abc");
}
