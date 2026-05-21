//! v2 mockspace.toml parsing smoke tests (spec §46).

use mockspace_config::{
    parse_mockspace_toml, parse_mockspace_toml_str, BuiltInLiteral, ConfigError, ForgeKind,
    LanguageEntry, MergeStyle, OnDirtyState, Severity,
};

#[test]
fn parse_minimal_v2() {
    let toml = r#"
        [mockspace]
        version = "1.0"
    "#;
    let cfg = parse_mockspace_toml_str(toml).unwrap();
    assert_eq!(cfg.mockspace.version, "1.0");
    assert_eq!(cfg.mockspace.default_profile, "dev");
    assert!(cfg.mockspace.default_one_active_round);
    assert_eq!(cfg.mockspace.verifier_timeout_seconds, 30);
    assert!(cfg.mockspace.mock_bin_path.is_none());
    assert!(cfg.refs.mirror_ext_refs);
    assert!(!cfg.refs.push_mirrors);
    assert!(cfg.refs.fetch_on_reference);
    assert!(cfg.refs.security.require_https);
}

#[test]
fn parse_full_spec_example() {
    // Reduced from the §46 schema example. Covers every section.
    // Top-level root keys (layers, primary_*) must precede any [section]
    // headers per TOML spec.
    let toml = r##"
layers = ["L0", "L1", "L2", "L3"]
primary_domain_macro = "strategy_marker_required"
primary_domain_label = "Strategy axis"
primary_host = "self"

[mockspace]
version = "1.0"
default_profile = "dev"
default_one_active_round = true
verifier_timeout_seconds = 30
mock_bin_path = "target/release/mock"

[refs]
mirror_ext_refs = true
push_mirrors = false
fetch_on_reference = true
task_archive_threshold_days = 90
round_archive_threshold_days = 365

[refs.security]
domain_allowlist = ["github.com", "codeberg.org"]
require_https = true

[hosts.self]
url = "https://codeberg.org/orgrinrt/mockspace.git"
type = "github"
token_env = "MOCK_FORGE_TOKEN"
auto_open_pr = true
auto_push_body = true
auto_merge_on_done = false
merge_style = "squash"
default_base_branch = "dev"
pr_body_managed_section_delimiter_start = "<!-- mockspace-managed -->"
pr_body_managed_section_delimiter_end = "<!-- /mockspace-managed -->"
api_retry_attempts = 3
api_retry_backoff_seconds = [1, 4, 16]

[hosts.mockspace-rs]
url = "https://codeberg.org/mockspace/mockspace-rs.git"

[hosts.arvo]
url = "https://github.com/orgrinrt/arvo.git"
forge_url_template = "https://github.com/orgrinrt/arvo/tree/{ref}"

[imports]
import = [
  "mock://hook/on_custom_doctor.sh",
  "mock://@/export/profile-dev",
]

[imports.ext.mockspace-rs]
include = ["hooks/**/*.rs", "lints/**/*.rs"]
runner = "mock://ext/mockspace-rs/export/runner-rs"

[lint-crates.mockspace-hilavitkutin-stack-lints]
git = "https://codeberg.org/orgrinrt/mockspace-hilavitkutin-stack-lints.git"
rev = "abc123"

[lints.no-bare-numeric]
commit = "error"
build = "warn"
push = "error"

[lints.file-size]
commit = "warn"
build = "off"
push = "error"
max_lines = 500

[lints.forbidden-imports.scope.arvo-strategy]
commit = "error"
forbidden = ["arvo-storage", "arvo-graph"]
reason = "L0 cannot depend on L1+"

[languages]
rust = "built-in"
typescript = { git = "https://codeberg.org/mockspace/mockspace-ts.git", rev = "abc123" }

[profile.dev]
on_dirty_state = "prompt"

[profile.ci]
on_dirty_state = "refuse"

[profile.auto]
on_dirty_state = "auto"

[crate_colors.arvo-bits]
fg = "#ffffff"
bg = "#3f51b5"

[domain_kinds.numeric]
glyph = "n"
label = "Numeric"

[known_macros.strategy_marker_required]
description = "Every public numeric type carries a Strategy marker."
usage = "S: Strategy = Hot"

[transparency]
staleness_threshold_days = 90

[undo]
keep_entries = 50
keep_days = 30
"##;

    let cfg = parse_mockspace_toml_str(toml).unwrap();

    // [mockspace]
    assert_eq!(cfg.mockspace.version, "1.0");
    assert_eq!(
        cfg.mockspace.mock_bin_path.as_deref(),
        Some(std::path::Path::new("target/release/mock"))
    );

    // [refs]
    assert_eq!(cfg.refs.task_archive_threshold_days, 90);
    assert_eq!(
        cfg.refs.security.domain_allowlist,
        vec!["github.com", "codeberg.org"]
    );

    // [hosts.*] + primary host
    assert_eq!(cfg.primary_host.as_deref(), Some("self"));
    assert_eq!(cfg.hosts.len(), 3);
    let primary = &cfg.hosts[cfg.primary_host.as_ref().unwrap()];
    assert_eq!(primary.kind, Some(ForgeKind::Github));
    assert_eq!(primary.merge_style, Some(MergeStyle::Squash));
    assert_eq!(primary.api_retry_backoff_seconds, Some(vec![1, 4, 16]));
    assert_eq!(primary.token_env.as_deref(), Some("MOCK_FORGE_TOKEN"));

    // Secondary hosts have just the import-shape fields.
    assert_eq!(
        cfg.hosts["arvo"].forge_url_template.as_deref(),
        Some("https://github.com/orgrinrt/arvo/tree/{ref}")
    );
    assert!(cfg.hosts["arvo"].kind.is_none());
    assert!(cfg.hosts["mockspace-rs"].token_env.is_none());

    // [imports]
    assert_eq!(cfg.imports.import.len(), 2);
    assert_eq!(cfg.imports.ext["mockspace-rs"].include.len(), 2);

    // [lint-crates]
    assert_eq!(
        cfg.lint_crates["mockspace-hilavitkutin-stack-lints"].rev,
        "abc123"
    );

    // [lints.*]
    let bare = &cfg.lints["no-bare-numeric"];
    assert_eq!(bare.commit, Some(Severity::Error));
    assert_eq!(bare.build, Some(Severity::Warn));
    assert_eq!(bare.push, Some(Severity::Error));
    let file_size = &cfg.lints["file-size"];
    assert_eq!(file_size.build, Some(Severity::Off));
    assert!(file_size.extras.contains_key("max_lines"));
    let scoped = &cfg.lints["forbidden-imports"].scope["arvo-strategy"];
    assert_eq!(scoped.commit, Some(Severity::Error));
    assert!(scoped.extras.contains_key("forbidden"));

    // [languages]
    assert!(matches!(
        cfg.languages["rust"],
        LanguageEntry::BuiltIn(BuiltInLiteral::BuiltIn)
    ));
    assert!(matches!(
        cfg.languages["typescript"],
        LanguageEntry::Host(_)
    ));

    // [profile.*]
    assert_eq!(
        cfg.profile["dev"].on_dirty_state,
        Some(OnDirtyState::Prompt)
    );
    assert_eq!(cfg.profile["ci"].on_dirty_state, Some(OnDirtyState::Refuse));
    assert_eq!(cfg.profile["auto"].on_dirty_state, Some(OnDirtyState::Auto));

    // Doc-gen metadata
    assert_eq!(cfg.crate_colors["arvo-bits"].fg.as_deref(), Some("#ffffff"));
    assert_eq!(cfg.domain_kinds["numeric"].glyph.as_deref(), Some("n"));
    assert_eq!(cfg.layers, vec!["L0", "L1", "L2", "L3"]);
    assert_eq!(cfg.primary_domain_label.as_deref(), Some("Strategy axis"));

    // [transparency]
    assert_eq!(cfg.transparency.staleness_threshold_days, Some(90));
    assert!(cfg.transparency.log_uri.is_none());

    // [undo]
    assert_eq!(cfg.undo.keep_entries, 50);
}

#[test]
fn roundtrip_preserves_values() {
    // Covers MergeStyle::Merge plus a parameter sweep across Rebase to
    // catch silent drops on either variant. Asserts non-trivial field
    // values on `reparsed` directly so a poorly shaped PartialEq cannot
    // mask a round-trip regression.
    for style in ["merge", "rebase"] {
        let toml = format!(
            r#"
primary_host = "origin"

[mockspace]
version = "1.0"
mock_bin_path = "target/release/mock"

[hosts.origin]
url = "https://codeberg.org/example/repo.git"
type = "forgejo"
token_env = "CB_TOKEN"
merge_style = "{style}"

[lints.no-bare-numeric]
commit = "error"
"#
        );
        let parsed = parse_mockspace_toml_str(&toml).unwrap();
        let serialised = toml::to_string(&parsed).unwrap();
        let reparsed = parse_mockspace_toml_str(&serialised).unwrap();
        assert_eq!(parsed, reparsed);
        assert_eq!(reparsed.hosts["origin"].kind, Some(ForgeKind::Forgejo));
        assert_eq!(
            reparsed.lints["no-bare-numeric"].commit,
            Some(Severity::Error)
        );
        let expected_style = match style {
            "merge" => MergeStyle::Merge,
            "rebase" => MergeStyle::Rebase,
            _ => unreachable!(),
        };
        assert_eq!(reparsed.hosts["origin"].merge_style, Some(expected_style));
    }
}

#[test]
fn rejects_retired_primitive_introductions_table() {
    // The legacy `[primitive-introductions]` v1 table is retired in v2.
    // Schema is strict: `deny_unknown_fields` on `Config` surfaces it
    // as a parse error naming the retired key directly. Consumers are
    // redirected to the at-site `lint:prop(source_wrapper)` form.
    let toml = r#"
[mockspace]
version = "1.0"

[primitive-introductions]
arvo-bits = ["bit-storage"]
"#;
    let err = parse_mockspace_toml_str(toml).unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("primitive-introductions"),
        "error should name the retired table directly, got: {msg}"
    );
}

#[test]
fn rejects_arbitrary_unknown_top_level_field() {
    // Schema strictness extends to any top-level field not declared on
    // `Config`, not only the named retired table. The unknown key has
    // to live above any section header so TOML routes it to the root.
    let toml = r#"
unknown_field_consumer_made_up = "value"

[mockspace]
version = "1.0"
"#;
    let err = parse_mockspace_toml_str(toml).unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("unknown_field_consumer_made_up"),
        "error should name the offending field, got: {msg}"
    );
}

#[test]
fn rejects_empty_version_string() {
    let toml = r#"
        [mockspace]
        version = ""
    "#;
    let err = parse_mockspace_toml_str(toml).unwrap_err();
    match err {
        ConfigError::Validation { rule, .. } => assert_eq!(rule, "mockspace.version"),
        other => panic!("expected validation error, got {other:?}"),
    }
}

#[test]
fn rejects_unsupported_major_version() {
    let toml = r#"
        [mockspace]
        version = "99.0"
    "#;
    let err = parse_mockspace_toml_str(toml).unwrap_err();
    match err {
        ConfigError::Validation { rule, details } => {
            assert_eq!(rule, "mockspace.version");
            assert!(
                details.contains("99"),
                "details should mention major: {details}"
            );
        }
        other => panic!("expected validation error, got {other:?}"),
    }
}

#[test]
fn rejects_invalid_enum_values() {
    // Severity, ForgeKind, MergeStyle, and OnDirtyState all route
    // through serde's enum dispatch. The error surface for each is
    // identical (`ConfigError::Parse`); a single parameterised test
    // covers the four variants and asserts the offending value appears
    // in the displayed error so a future enum addition that silently
    // accepts the value would surface here.
    let cases = [
        (
            r#"
                [mockspace]
                version = "1.0"
                [lints.foo]
                commit = "frobnicate"
            "#,
            "frobnicate",
        ),
        (
            r#"
                [mockspace]
                version = "1.0"
                [hosts.origin]
                url = "https://example.com/x.git"
                type = "bitbucket"
            "#,
            "bitbucket",
        ),
        (
            r#"
                [mockspace]
                version = "1.0"
                [hosts.origin]
                url = "https://example.com/x.git"
                merge_style = "fast-forward-only"
            "#,
            "fast-forward-only",
        ),
        (
            r#"
                [mockspace]
                version = "1.0"
                [profile.dev]
                on_dirty_state = "explode"
            "#,
            "explode",
        ),
    ];
    for (toml, offending) in cases {
        let err = parse_mockspace_toml_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
        let display = format!("{err}");
        assert!(
            display.contains(offending),
            "error for `{offending}` should mention the value, got: {display}"
        );
    }
}

#[test]
fn parses_severity_info_variant() {
    // Severity has four variants (Error, Warn, Info, Off). The
    // spec-example fixture covers Error / Warn / Off; this test pins
    // Info so a future change to the enum or its serde representation
    // surfaces here rather than silently dropping the variant.
    let toml = r#"
        [mockspace]
        version = "1.0"
        [lints.foo]
        commit = "info"
    "#;
    let cfg = parse_mockspace_toml_str(toml).unwrap();
    assert_eq!(cfg.lints["foo"].commit, Some(Severity::Info));
}

#[test]
fn rejects_lint_crate_missing_rev() {
    // `LintCrateRef` requires both `git` and `rev`. Omitting `rev`
    // should fail at parse with serde naming the missing field.
    let toml = r#"
[mockspace]
version = "1.0"

[lint-crates.stack-lints]
git = "https://example.com/x.git"
"#;
    let err = parse_mockspace_toml_str(toml).unwrap_err();
    assert!(matches!(err, ConfigError::Parse(_)));
    let display = format!("{err}");
    assert!(
        display.contains("rev"),
        "error should name the missing field, got: {display}"
    );
}

#[test]
fn rejects_host_missing_url() {
    // `HostSection.url` is non-optional and lacks `#[serde(default)]`.
    // Omitting it should fail at parse with serde naming the missing
    // field.
    let toml = r#"
[mockspace]
version = "1.0"

[hosts.origin]
type = "forgejo"
"#;
    let err = parse_mockspace_toml_str(toml).unwrap_err();
    assert!(matches!(err, ConfigError::Parse(_)));
    let display = format!("{err}");
    assert!(
        display.contains("url"),
        "error should name the missing field, got: {display}"
    );
}

#[test]
fn parse_mockspace_toml_reads_file() {
    // Cover the file-path variant alongside the string variant.
    // Writes a fixture to a tempdir, reads back through the disk
    // entry point, and confirms the parsed values match.
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("mockspace.toml");
    std::fs::write(
        &path,
        r#"
[mockspace]
version = "1.0"
default_profile = "ci"
"#,
    )
    .unwrap();
    let cfg = parse_mockspace_toml(&path).unwrap();
    assert_eq!(cfg.mockspace.version, "1.0");
    assert_eq!(cfg.mockspace.default_profile, "ci");
}

#[test]
fn parse_mockspace_toml_missing_file_returns_io_error() {
    // The disk variant surfaces missing files as `ConfigError::Io`
    // (distinct from the string variant's `Parse` errors), keeping
    // the two failure modes routable separately by consumers.
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("does-not-exist.toml");
    let err = parse_mockspace_toml(&missing).unwrap_err();
    assert!(
        matches!(err, ConfigError::Io(_)),
        "expected Io error variant, got: {err:?}"
    );
}

#[test]
fn extras_capture_unknown_lint_keys() {
    let toml = r#"
[mockspace]
version = "1.0"

[lints.file-size]
commit = "warn"
max_lines = 500
allow_test_files = true
"#;
    let cfg = parse_mockspace_toml_str(toml).unwrap();
    let lint = &cfg.lints["file-size"];
    assert!(lint.extras.contains_key("max_lines"));
    assert!(lint.extras.contains_key("allow_test_files"));
}

// ---- preset infrastructure schema (#537) ---------------------------------

#[test]
fn import_accepts_bare_uri_string() {
    // Legacy form: a bare URI string. Loader treats this as the
    // default executable trust tier.
    let toml = r#"
[mockspace]
version = "1.0"

[imports]
import = ["mock://ext/stack-lints/export/lint-preset/no-heap"]
"#;
    let cfg = parse_mockspace_toml_str(toml).unwrap();
    assert_eq!(cfg.imports.import.len(), 1);
    match &cfg.imports.import[0] {
        mockspace_config::ImportEntry::Uri(s) => {
            assert_eq!(s, "mock://ext/stack-lints/export/lint-preset/no-heap");
        }
        other => panic!("expected Uri, got {other:?}"),
    }
}

#[test]
fn import_accepts_typed_entry_with_kind() {
    let toml = r#"
[mockspace]
version = "1.0"

[[imports.import]]
uri = "mock://ext/stack-lints/export/lint-preset/no-heap"
kind = "config"
"#;
    let cfg = parse_mockspace_toml_str(toml).unwrap();
    assert_eq!(cfg.imports.import.len(), 1);
    match &cfg.imports.import[0] {
        mockspace_config::ImportEntry::Typed(t) => {
            assert_eq!(t.uri, "mock://ext/stack-lints/export/lint-preset/no-heap");
            assert_eq!(t.kind, mockspace_config::ImportKind::Config);
        }
        other => panic!("expected Typed, got {other:?}"),
    }
}

#[test]
fn import_typed_entry_defaults_to_executable_kind() {
    let toml = r#"
[mockspace]
version = "1.0"

[[imports.import]]
uri = "mock://ext/some-pack/export/hook/pre-commit"
"#;
    let cfg = parse_mockspace_toml_str(toml).unwrap();
    match &cfg.imports.import[0] {
        mockspace_config::ImportEntry::Typed(t) => {
            assert_eq!(t.kind, mockspace_config::ImportKind::Executable);
            assert_eq!(t.uri, "mock://ext/some-pack/export/hook/pre-commit");
        }
        other => panic!("expected Typed, got {other:?}"),
    }
}

#[test]
fn import_mixes_bare_and_typed_entries() {
    let toml = r#"
[mockspace]
version = "1.0"

[imports]
import = [
  "mock://ext/old/export/hook/pre-commit",
  { uri = "mock://ext/stack-lints/export/lint-preset/no-heap", kind = "config" },
]
"#;
    let cfg = parse_mockspace_toml_str(toml).unwrap();
    assert_eq!(cfg.imports.import.len(), 2);
    assert!(matches!(
        cfg.imports.import[0],
        mockspace_config::ImportEntry::Uri(_)
    ));
    assert!(matches!(
        cfg.imports.import[1],
        mockspace_config::ImportEntry::Typed(_)
    ));
}

#[test]
fn lint_config_accepts_extends_shorthand() {
    let toml = r#"
[mockspace]
version = "1.0"

[lints.my-no-heap]
extends = "stack-lints::no-heap"
commit = "error"
"#;
    let cfg = parse_mockspace_toml_str(toml).unwrap();
    let lint = &cfg.lints["my-no-heap"];
    assert_eq!(lint.extends.as_deref(), Some("stack-lints::no-heap"));
    assert_eq!(lint.commit, Some(mockspace_config::Severity::Error));
}

#[test]
fn preset_file_parses_all_fields() {
    use mockspace_config::{PresetFile, Severity};

    let toml = r#"
schema_version = "1.0"
name = "no-heap"
primitive = "forbidden_imports"
description = "Forbid alloc::* and std::vec usage in no-heap codebases."
extends = "mockspace::no-alloc"

[config]
forbidden = ["alloc::*", "std::vec::*"]
reason = "no-heap discipline"

[severity]
commit = "warn"
build = "error"
push = "error"

[scope]
exempt_paths = ["**/tests/**"]
"#;
    let preset: PresetFile = toml::from_str(toml).unwrap();
    assert_eq!(preset.schema_version, "1.0");
    assert_eq!(preset.name, "no-heap");
    assert_eq!(preset.primitive, "forbidden_imports");
    assert_eq!(
        preset.description.as_deref(),
        Some("Forbid alloc::* and std::vec usage in no-heap codebases.")
    );
    assert_eq!(preset.extends.as_deref(), Some("mockspace::no-alloc"));
    assert!(preset.config.contains_key("forbidden"));
    assert!(preset.config.contains_key("reason"));
    assert_eq!(preset.severity.commit, Some(Severity::Warn));
    assert_eq!(preset.severity.build, Some(Severity::Error));
    assert_eq!(preset.severity.push, Some(Severity::Error));
    let exempt = preset
        .scope
        .get("exempt_paths")
        .expect("scope should carry exempt_paths");
    assert!(
        exempt.is_array(),
        "exempt_paths should deserialise as array, got: {exempt:?}"
    );
}

#[test]
fn preset_file_omits_optional_fields() {
    use mockspace_config::PresetFile;

    let toml = r#"
schema_version = "1.0"
name = "no-bare-numeric"
primitive = "ast_type_position"
"#;
    let preset: PresetFile = toml::from_str(toml).unwrap();
    assert_eq!(preset.name, "no-bare-numeric");
    assert!(preset.description.is_none());
    assert!(preset.extends.is_none());
    assert!(preset.config.is_empty());
    assert!(preset.severity.commit.is_none());
    assert!(preset.scope.is_empty());
}

#[test]
fn typed_import_rejects_unknown_field() {
    use mockspace_config::TypedImport;
    // Typo: `knd` instead of `kind`. deny_unknown_fields catches the
    // typo at load time rather than silently dropping it.
    let toml = r#"
uri = "mock://ext/foo/export/lint-preset/bar"
knd = "config"
"#;
    let result: Result<TypedImport, _> = toml::from_str(toml);
    assert!(result.is_err(), "expected typo to be rejected");
}

#[test]
fn preset_file_rejects_unknown_field() {
    use mockspace_config::PresetFile;
    // Typo: `extens` instead of `extends`. Catches at load.
    let toml = r#"
schema_version = "1.0"
name = "no-heap"
primitive = "forbidden_imports"
extens = "mockspace::no-alloc"
"#;
    let result: Result<PresetFile, _> = toml::from_str(toml);
    assert!(result.is_err(), "expected typo to be rejected");
}
