//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The setup a bench runs under that its own `bench.toml` does not hold: the
//! values of the root `bench.toml` it uses, each resolved and named on a line
//! of its own, so a snapshot's lock freezes them and a tracking arm's source
//! moves with them, while an edit to a part of the root file the bench does
//! not use moves neither.
//!
//! Read: the root `[timing]`, the `opt-level`, `lto` and `codegen-units` of the
//! root `[build]`, the root `[workload.<name>]` of every workload the bench and
//! its sweeps name, and for a root `[bench.<name>]` section that section
//! itself. A value the root does not declare is read as `absent`, which is the
//! framework's default and moves when it is declared.

use std::path::Path;

use sha2::{Digest, Sha256};

use super::plan::Bench;
use crate::View;

/// The keys of the root `[build]` that change the code a build makes. Its
/// `mockspace` key names the instrument, which is not the code measured.
const BUILD_KEYS: [&str; 3] = ["opt-level", "lto", "codegen-units"];

/// The prefix of a setup line's name, which no file's path takes.
pub const SETUP: &str = "setup:";

/// One `<sha256>  setup:<what>` line per value of the root file `bench` uses.
pub fn setup_lines(view: &View<'_>, bench_dir: &Path, bench: &Bench) -> String {
    let root = match view.toml(bench_dir.join("bench.toml")) {
        Some(Ok(t)) => Some(t),
        _ => None,
    };
    let get = |path: &[&str]| -> Option<toml::Value> {
        let mut at = root?.get(path[0])?;
        for key in &path[1 ..] {
            at = at.get(key)?;
        }
        Some(at.clone())
    };
    let mut items: Vec<(String, Option<toml::Value>)> = vec![("[timing]".to_string(), get(&["timing"]))];
    let build = get(&["build"]).and_then(|b| b.as_table().cloned()).map(|b| {
        toml::Value::Table(b.into_iter().filter(|(k, _)| BUILD_KEYS.contains(&k.as_str())).collect())
    });
    items.push(("[build]".to_string(), build));
    for w in &bench.workloads {
        items.push((format!("[workload.{w}]"), get(&["workload", w])));
    }
    if bench.is_root_section(bench_dir) {
        if let Some(name) = &bench.section {
            items.push((format!("[bench.{name}]"), get(&["bench", name])));
        }
    }
    items
        .into_iter()
        .map(|(what, value)| {
            let text = value.as_ref().map_or_else(|| "absent".to_string(), canon);
            let hex: String = Sha256::digest(text.as_bytes()).iter().map(|x| format!("{x:02x}")).collect();
            format!("{hex}  {SETUP}{what}\n")
        })
        .collect()
}

/// A value in one spelling whatever way it was written: keys in order, no
/// whitespace, and no dependence on how any toml crate prints.
fn canon(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => format!("{s:?}"),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => format!("{f:?}"),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Datetime(d) => d.to_string(),
        toml::Value::Array(a) => format!("[{}]", a.iter().map(canon).collect::<Vec<_>>().join(",")),
        toml::Value::Table(t) => {
            let mut kv: Vec<(&String, &toml::Value)> = t.iter().collect();
            kv.sort_by(|a, b| a.0.cmp(b.0));
            format!(
                "{{{}}}",
                kv.into_iter().map(|(k, v)| format!("{k:?}={}", canon(v))).collect::<Vec<_>>().join(",")
            )
        },
    }
}
