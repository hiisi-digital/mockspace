//! A builtin text names this project's directories through `{mock_dir}`-style tokens, and one
//! that is not run through `substitute_builtin_vars` ships the token to the agent as written.

use super::*;

#[test]
fn no_builtin_rule_or_skill_ships_a_template_token() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = tmp.path().join("mock");
    std::fs::create_dir_all(&mock).unwrap();
    std::fs::write(mock.join("mockspace.toml"), "project_name = \"probe\"\n").unwrap();
    let mut cfg = crate::config::Config::from_dir(&mock);
    cfg.agent.panel_discipline = true;
    cfg.agent.accelerated_interactive_design_talks = true;
    let built = generate_builtin_templates(&cfg, &LintPack::default());
    assert!(
        built.skills.iter().any(|s| s.dir_name == "design-talk")
            && built.skills.iter().any(|s| s.dir_name == "sketching")
            && built.skills.iter().any(|s| s.dir_name == "benchmarking"),
        "control: the knobs turned the skills on"
    );
    let tokens = ["{mock_dir}", "{project_name}", "{crate_prefix}", "{src_dirs}"];
    let texts = built
        .rules
        .iter()
        .map(|r| (r.name.clone(), r.body.clone()))
        .chain(built.skills.iter().map(|s| (s.dir_name.clone(), s.body.clone())))
        .chain(built.skills.iter().map(|s| (s.dir_name.clone(), s.skill_description.clone())));
    for (name, text) in texts {
        for t in tokens {
            assert!(!text.contains(t), "`{name}` ships the token {t}");
        }
    }
}
