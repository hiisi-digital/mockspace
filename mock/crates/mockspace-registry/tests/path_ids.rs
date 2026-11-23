//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Path ids through the public surface: declared in a config, folded by the loader, checked,
//! resolved in a document and selected.

#[path = "../../../../tests/common/ambient.rs"]
mod ambient;

use std::path::Path;

use mockspace_registry::{
    Env, RegistryEnv, dangling_references, load_registry_for, resolve_all, resolve_data, validate,
    validate_row_references,
};

const CONFIG: &str = r#"project_name = "fixture"
crate_prefix = "fixture"

[[registry.namespace]]
key = "kind"

[[registry.namespace]]
key = "subject"

[[registry.namespace]]
key = "ticket"
path = ["kind", "subject"]

[[registry.namespace.field]]
name = "kind"
type = "kind"
required = true

[[registry.namespace.field]]
name = "subject"
type = "subject"
required = true

[[registry.namespace.field]]
name = "title"
type = "string"
required = true

[[registry.namespace.field]]
name = "needs"
type = "ticket[]"

[[registry.namespace.field]]
name = "related"
type = "ref[]"
"#;

const ROWS: &str = r#"
[[kind]]
id = "question"

[[kind]]
id = "task"

[[subject]]
id = "engine::internals"

[[subject]]
id = "engine::render"

[[ticket]]
id = "question::engine::internals::deps_allowed"
title = "Which deps are allowed"
needs = ["task::engine::render::split"]
related = ["subject::engine::render"]

[[ticket]]
id = "task::engine::render::split"
title = "Split the pass"

[[ticket]]
id = "task::engine::render::loose"
title = "No needs"
"#;

fn project(rows: &str) -> (tempfile::TempDir, Env) {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let mock = tmp.path().join("mock");
    std::fs::create_dir_all(mock.join("registry")).unwrap();
    std::fs::create_dir(tmp.path().join(".git")).unwrap();
    std::fs::write(mock.join("mockspace.toml"), CONFIG).unwrap();
    std::fs::write(mock.join("registry/rows.toml"), rows).unwrap();
    let env = Env::from_dir(&mock).unwrap();
    (tmp, env)
}

fn resolve(env: &Env, text: &str) -> String {
    let reg = load_registry_for(env).unwrap();
    let (reg, _) = resolve_data(env.namespaces(), &reg, env.roots(), env.repo_root(), Path::new("/nonexistent"), env);
    resolve_all(text, env.namespaces(), &reg, env.roots(), env.repo_root(), Path::new("/nonexistent"), env)
}

#[test]
fn the_path_declared_in_a_config_folds_and_checks_clean() {
    let (_tmp, env) = project(ROWS);
    let reg = load_registry_for(&env).unwrap();
    let found: Vec<_> = validate(env.namespaces(), &reg)
        .into_iter()
        .chain(validate_row_references(&reg, env.namespaces()))
        .chain(mockspace_registry::unfit_path_fields(env.namespaces()))
        .collect();
    assert!(found.is_empty(), "{found:?}");
    let row = reg.get("ticket::question::engine::internals::deps_allowed").unwrap();
    assert_eq!(row.fields["kind"], "question");
    assert_eq!(row.fields["subject"], "engine::internals");
}

#[test]
fn a_typed_reference_and_a_link_take_a_path_id_and_report_a_missing_one() {
    let (tmp, env) = project(&ROWS.replace("needs = [\"task::engine::render::split\"]", "needs = [\"task::engine::render::merge\"]"));
    let reg = load_registry_for(&env).unwrap();
    let kinds: Vec<_> = validate_row_references(&reg, env.namespaces()).into_iter().map(|f| f.kind).collect();
    assert_eq!(kinds, ["unknown-row-reference"]);
    drop(tmp);
    let (_tmp, env) = project(&ROWS.replace("related = [\"subject::engine::render\"]", "related = [\"subject::engine::motor\"]"));
    let reg = load_registry_for(&env).unwrap();
    let kinds: Vec<_> = validate_row_references(&reg, env.namespaces()).into_iter().map(|f| f.kind).collect();
    assert_eq!(kinds, ["unknown-row-reference"]);
}

#[test]
fn a_document_reads_a_path_id_its_fields_and_a_one_name_id_beside_it() {
    let (_tmp, env) = project(ROWS);
    let out = resolve(
        &env,
        "a {{ ticket::question::engine::internals::deps_allowed::title }} b \
         {{ reg::ticket::task::engine::render::loose::title }} c {{ ticket::question::engine::internals::deps_allowed::subject }} \
         d {{ kind::task }}",
    );
    assert!(out.contains("a Which deps are allowed b"), "{out}");
    assert!(out.contains("b No needs c"), "{out}");
    assert!(out.contains("c engine::internals d"), "the folded field reads as a written one: {out}");
    assert!(out.contains("d task"), "{out}");
}

#[test]
fn the_operations_take_a_path_id() {
    let (_tmp, env) = project(ROWS);
    let out = resolve(&env, "{{ pathof(ticket::task::engine::render::split) }}");
    assert!(out.ends_with("mock/registry/rows.toml"), "{out}");
    let out = resolve(&env, "{{ refsto(ticket::task::engine::render::split) }}");
    assert_eq!(out, "question::engine::internals::deps_allowed");
}

#[test]
fn a_branch_resolves_as_its_rows_and_a_missing_row_is_left_and_reported() {
    let (_tmp, env) = project(ROWS);
    let reg = load_registry_for(&env).unwrap();
    let keys = env.namespaces().iter().map(|n| n.key.clone()).collect();
    let text = "{{ ticket::question::engine }} {{ ticket::task::engine::render::loose::nothing }} \
                {{ ticket::task::engine::render::loose::title }} {{ ticket::idea }}";
    let d = dangling_references(text, &reg, &keys);
    assert_eq!(
        d.into_iter().collect::<Vec<_>>(),
        ["ticket::idea", "ticket::task::engine::render::loose::nothing"],
        "a branch stands for its rows and is no dangling reference"
    );
    let out = resolve(&env, "{{ ticket::question::engine.count() }}");
    assert_eq!(out, "1");
}

#[test]
fn a_widening_reference_resolves_through_the_one_reader() {
    let (_tmp, env) = project(ROWS);
    // Rows of one namespace are its table, holding just them, and every table method narrows it.
    let out = resolve(&env, "{{ ticket::**::render::**.count() }} {{ ticket::task::*.select(id) }}");
    assert!(out.starts_with("2 "), "{out}");
    assert!(out.contains("task::engine::render::loose") && out.contains("task::engine::render::split"), "{out}");
    assert!(!out.contains("deps_allowed"), "{out}");
    // A field step after a wildcard is each row's value of it.
    let out = resolve(&env, "{{ ticket::{task, question}::engine::*::*::title }}");
    assert_eq!(out, "No needs, Split the pass, Which deps are allowed");
    // A wildcard matching nothing is an empty table, and a name naming nothing fails and is left.
    assert_eq!(resolve(&env, "{{ ticket::*::nothing.count() }}"), "0");
    assert_eq!(resolve(&env, "{{ ticket::idea::** }}"), "{{ ticket::idea::** }}");
    // The operations take a selection where it means something.
    let out = resolve(&env, "{{ pathof(ticket::**) }}");
    assert!(out.ends_with("mock/registry/rows.toml") && !out.contains(','), "each file once: {out}");
    let out = resolve(&env, "{{ refsto(ticket::task::**) }}");
    assert_eq!(out, "question::engine::internals::deps_allowed");
}

#[test]
fn the_check_holds_the_path_declaration() {
    let (_tmp, env) = project(ROWS);
    let unfit = CONFIG.replace("name = \"subject\"\ntype = \"subject\"\nrequired = true\n", "name = \"subject\"\ntype = \"subject\"\n");
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let mock = tmp.path().join("mock");
    std::fs::create_dir_all(mock.join("registry")).unwrap();
    std::fs::create_dir(tmp.path().join(".git")).unwrap();
    std::fs::write(mock.join("mockspace.toml"), unfit).unwrap();
    let bad = Env::from_dir(&mock).unwrap();
    let reg = load_registry_for(&bad).unwrap();
    let kinds = |env: &Env, reg: &mockspace_registry::Registry| -> Vec<&'static str> {
        mockspace_registry::check(env, reg, &[], None, None).findings.iter().map(|f| f.kind).collect()
    };
    assert!(kinds(&bad, &reg).contains(&"unfit-path-field"), "{:?}", kinds(&bad, &reg));
    let good = load_registry_for(&env).unwrap();
    assert!(!kinds(&env, &good).contains(&"unfit-path-field"));
}

#[test]
fn selection_reads_the_tree_the_folded_ids_make() {
    let (_tmp, env) = project(ROWS);
    let reg = load_registry_for(&env).unwrap();
    let ids = |q: &str| -> Vec<String> {
        reg.select_rows(env.namespaces(), q).unwrap().iter().map(|r| r.qualified()).collect()
    };
    assert_eq!(ids("ticket::*::engine"), [
        "ticket::question::engine::internals::deps_allowed",
        "ticket::task::engine::render::loose",
        "ticket::task::engine::render::split"
    ]);
    assert_eq!(ids("ticket::**::render::**"), ["ticket::task::engine::render::loose", "ticket::task::engine::render::split"]);
    assert_eq!(ids("subject::engine"), ["subject::engine::internals", "subject::engine::render"]);
}
