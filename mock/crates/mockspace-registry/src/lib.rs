//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A registry of terms and concepts, resolved at doc-generation time.
//!
//! A project accumulates identifiers for the things its documents refer to:
//! probes, measurements, constants, invariants, vocabulary. Without somewhere
//! to hold them, each document family invents its own convention and the
//! conventions drift. Worse, nothing can answer "what are all the X", and a
//! gap is only visible against an enumeration.
//!
//! Data lives under `<mock>/registry/`, arbitrarily nested, every `*.toml`
//! beneath it loaded. **A file's namespace is the array-of-tables key it
//! declares, never its path.** That is what makes the nesting free: a project
//! may file by subject (`registry/water.toml` holding `[[spike]]`, `[[bench]]`,
//! and `[[constant]]` rows) and still query by kind.
//!
//! # Identity
//!
//! A row is identified by its namespace and a slug: `vocab::xpbd`,
//! `spike::actuator_fit_converges`. Slugs are snake_case and unique within
//! their namespace.
//!
//! That pair is the row's key path, and it is the whole of the row's identity.
//! The files a registry is written in are no part of it: any number of
//! `*.toml` files under any names and directories make one registry, and
//! splitting a file, merging two, renaming one or moving a row to another
//! changes nothing a reader, a check or a relation sees, except what `pathof`
//! answers, which is that file. A namespace lists its
//! rows by key path, never by the order of the files that hold them. One key
//! path declared in two files is refused, with both files named, and where a
//! row lives is only ever given as a place to look.
//!
//! Slugs rather than numbers because a number carries no meaning, and an
//! identifier carrying no meaning has to be *managed*: never reused, never
//! renumbered, never reordered, since any of those silently repoints every
//! reference to it. A slug needs none of that discipline. It says what it
//! refers to, it survives reordering, and it stays readable in prose.
//!
//! # References
//!
//! One syntax, `root::selector...`, covers both kinds:
//!
//! - `reg::vocab::xpbd` selects a row; `reg::vocab::xpbd::what` selects a field.
//! - `seed::DESIGN::844` selects a line in a file, under a declared root.
//!
//! Both resolve at generation and both are checked, so a reference pointing
//! nowhere is reported rather than rendered as something that looks fine.
//!
//! Designed for v2, prototyped here in v1. See
//! `docs/REFERENCE-SYNTAX.md`.

//! # Layout
//!
//! Split along the path a row takes rather than by size: it is loaded, its
//! references are found, those are resolved, the result is rendered, and what
//! a schema structurally cannot check is validated.

pub mod append;
mod bounds;
mod check;
mod env;
mod load;
pub mod merge;
mod model;
mod refpath;
mod refs;
mod render;
mod resolve;
mod resolved;
mod rowref;
mod schema_replay;
mod stores;
#[cfg(any(test, feature = "test-support"))]
pub mod test_world;
pub mod validate;
mod writer;

pub use bounds::*;
pub use check::*;
pub use env::*;
pub use load::*;
pub use model::*;
pub use refpath::*;
pub use refs::*;
pub use render::*;
pub use resolve::*;
pub use resolved::*;
pub use rowref::*;
pub use schema_replay::input_key;
pub use stores::*;
pub use validate::*;
pub use writer::*;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// The line a generated file carries, by which a later run knows the file as its own.
pub const GENERATED_MARKER: &str = "AUTO-GENERATED: DO NOT EDIT DIRECTLY";

/// Answer one reference expression from the command line.
///
/// The same syntax documents use, resolved by the same code, shown for a
/// terminal instead of for a markdown parser. That identity is the point: an
/// expression worked out at a prompt while thinking about a design pastes into
/// a document unchanged, and a reference in a document can be run to see what
/// it will say.
///
/// The alternative, a query path with a syntax of its own, would be two
/// languages to learn and two resolvers to keep in agreement.
pub fn cmd_query(cfg: &dyn RegistryEnv, expr: &str) -> std::process::ExitCode {
    use std::process::ExitCode;

    if expr.trim().is_empty() {
        eprintln!("usage: cargo mock query '<expression>'");
        eprintln!();
        eprintln!("  law                              a namespace, as a table");
        eprintln!("  law::keys                        one row");
        eprintln!("  law::keys::statement             one field");
        eprintln!("  law.where(crates~store)          rows whose field contains a value");
        eprintln!("  law.where(status=settled)        rows whose field is exactly a value");
        eprintln!("  law.select(id,statement)         only these columns");
        eprintln!("  law.where(...).count()           how many");
        eprintln!("  pathof(law::keys)                where it is declared");
        eprintln!("  sourcesof(law::keys)             what it rests on");
        eprintln!();
        eprintln!("Every one of these is valid in a document template too.");
        return ExitCode::FAILURE;
    }

    let namespaces = cfg.namespaces();
    let registry = match cfg.registry_stores().and_then(|s| load_registry_for_expr_from(&s, namespaces, expr)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };
    let (registry, _) = resolve_data(
        namespaces,
        &registry,
        cfg.roots(),
        cfg.repo_root(),
        cfg.docs_dir(),
        cfg,
    );
    let by_key: std::collections::BTreeMap<&str, &RegistryNamespace> =
        namespaces.iter().map(|n| (n.key.as_str(), n)).collect();

    match resolve_typed(
        expr.trim(),
        &by_key,
        &registry,
        cfg.roots(),
        cfg.repo_root(),
        cfg.docs_dir(),
        cfg,
    ) {
        Some(found) => {
            print!("{}", found.to_terminal());
            ExitCode::SUCCESS
        },
        None => {
            // The same answer a document gets, said out loud. A reference that
            // resolves to nothing is reported rather than rendered as something
            // plausible, and that holds here too.
            eprintln!("no result: `{}` does not resolve", expr.trim());

            // Say what was available rather than only that something was not.
            // A query naming a column that does not exist is the common
            // mistake, and the columns are right there to list.
            let base = expr.trim().split(['.', ':']).next().unwrap_or("").trim();
            if let Some(ns) = namespaces.iter().find(|n| n.key == base) {
                let (columns, rows) = table_cells(ns, &registry);
                eprintln!("`{base}` has {} rows and these columns:", rows.len());
                eprintln!("  {}", columns.join(", "));
            } else {
                let known: Vec<&str> = namespaces.iter().map(|n| n.key.as_str()).collect();
                if !known.is_empty() {
                    eprintln!("namespaces: {}", known.join(", "));
                }
            }
            ExitCode::FAILURE
        },
    }
}

/// What these tests take out of the environment before they run.
#[cfg(test)]
#[path = "../../../../tests/common/ambient.rs"]
mod test_ambient;
