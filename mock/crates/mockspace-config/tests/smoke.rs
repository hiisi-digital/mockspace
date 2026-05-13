//! Smoke tests for mockspace.toml parsing + IntoMockspaceConfig.

use mockspace_config::{
    parse_mockspace_toml_str, Config, ConfigError, InstallMode, IntoMockspaceConfig,
    MappingError,
};

#[test]
fn parse_minimal_toml() {
    let toml = r#"
        project_name = "demo"
        crate_prefix = "demo-"
    "#;
    let cfg = parse_mockspace_toml_str(toml).unwrap();
    assert_eq!(cfg.project_name, "demo");
    assert_eq!(cfg.crate_prefix, "demo-");
    assert_eq!(cfg.abi_version, 1);
}

#[test]
fn parse_with_install_modes() {
    let toml = r#"
        project_name = "demo"
        install_git_hooks = "merge-append"
        install_cargo_config = "skip"
    "#;
    let cfg = parse_mockspace_toml_str(toml).unwrap();
    assert_eq!(cfg.install_git_hooks, InstallMode::MergeAppend);
    assert_eq!(cfg.install_cargo_config, InstallMode::Skip);
}

#[test]
fn parse_with_lint_overrides() {
    let toml = r#"
        project_name = "demo"
        [lint_overrides]
        no-bare-numeric = "error"
        no-alloc = "warn"
    "#;
    let cfg = parse_mockspace_toml_str(toml).unwrap();
    assert_eq!(cfg.lint_overrides.get("no-bare-numeric"), Some(&"error".to_string()));
    assert_eq!(cfg.lint_overrides.get("no-alloc"), Some(&"warn".to_string()));
}

#[test]
fn parse_rejects_invalid_install_mode() {
    let toml = r#"
        project_name = "demo"
        install_git_hooks = "frobnicate"
    "#;
    let err = parse_mockspace_toml_str(toml).unwrap_err();
    assert!(matches!(err, ConfigError::Parse(_)));
}

#[test]
fn config_identity_into_mockspace_config() {
    let cfg = Config {
        project_name: "demo".into(),
        ..Config::default()
    };
    let out = cfg.clone().into_mockspace_config().unwrap();
    assert_eq!(out.project_name, "demo");
    assert_eq!(out.abi_version, cfg.abi_version);
}

#[test]
fn consumer_impl_can_map_to_config() {
    struct FakeConsumer {
        name: String,
    }

    impl IntoMockspaceConfig for FakeConsumer {
        fn into_mockspace_config(self) -> Result<Config, MappingError> {
            if self.name.is_empty() {
                return Err(MappingError::MissingField { name: "name" });
            }
            Ok(Config {
                project_name: self.name,
                ..Config::default()
            })
        }
    }

    let out = FakeConsumer {
        name: "homma".into(),
    }
    .into_mockspace_config()
    .unwrap();
    assert_eq!(out.project_name, "homma");

    let err = FakeConsumer { name: String::new() }
        .into_mockspace_config()
        .unwrap_err();
    assert!(matches!(err, MappingError::MissingField { name: "name" }));
}
