//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock registry-append <namespace>::<id> <field> <value> [--local]`: one string added to a
//! `string[]` field of a registry row.
//!
//! This is the one place a registry file is written to add an answer, a note or a link to a row,
//! so nothing else carries a text edit of its own. The namespace and the field must be declared,
//! the row must exist, a value the field already holds is not added twice, and the file keeps its
//! layout outside the row. Success prints nothing.
//!
//! A workspace may place a delegate that takes the step over whole (see [`hook`]). `--local`
//! skips it, which is how the delegate itself writes the row.

mod edit;
mod hook;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::process::ExitCode;

use crate::config::Config;
use crate::registry::load_registry;

const USAGE: &str = "usage: cargo mock registry-append <namespace>::<id> <field> <value> [--local]";

/// What was asked.
struct Ask<'a> {
    ns:    &'a str,
    id:    &'a str,
    field: &'a str,
    value: &'a str,
    local: bool,
}

fn parse<'a>(args: &[&'a str]) -> Result<Ask<'a>, String> {
    let local = args.contains(&"--local");
    let rest: Vec<&str> = args.iter().copied().filter(|a| *a != "--local").collect();
    let [target, field, value] = rest.as_slice() else {
        return Err(USAGE.to_string());
    };
    let (ns, id) = target
        .split_once("::")
        .filter(|(n, i)| !n.is_empty() && !i.is_empty())
        .ok_or_else(|| format!("`{target}` is not <namespace>::<id>\n{USAGE}"))?;
    Ok(Ask {
        ns,
        id,
        field,
        value,
        local,
    })
}

/// The file that holds the row, after the namespace and the field are checked against what the
/// project declares.
fn file_of(cfg: &Config, ask: &Ask<'_>) -> Result<PathBuf, String> {
    let namespace = cfg
        .registry_namespaces
        .iter()
        .find(|n| n.key == ask.ns)
        .ok_or_else(|| format!("no registry namespace `{}` is declared", ask.ns))?;
    let declared = namespace
        .fields
        .iter()
        .find(|f| f.name == ask.field)
        .ok_or_else(|| format!("`{}` declares no field `{}`", ask.ns, ask.field))?;
    if declared.r#type != "string[]" {
        return Err(format!(
            "`{}.{}` is a {}, and only a string[] is appended to",
            ask.ns, ask.field, declared.r#type
        ));
    }
    let reg = load_registry(&cfg.mock_dir, &cfg.registry_namespaces);
    reg.get(&format!("{}::{}", ask.ns, ask.id))
        .map(|r| r.source.clone())
        .ok_or_else(|| format!("no row {}::{}", ask.ns, ask.id))
}

/// The subcommand. `args` follow `registry-append`.
pub(crate) fn run(cfg: &Config, args: &[&str]) -> ExitCode {
    match step(cfg, args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mock registry-append: {e}");
            ExitCode::FAILURE
        },
    }
}

fn step(cfg: &Config, args: &[&str]) -> Result<(), String> {
    let ask = parse(args)?;
    let path = file_of(cfg, &ask)?;
    if !ask.local {
        if let Some(delegate) = hook::find(&cfg.repo_root) {
            let target = format!("{}::{}", ask.ns, ask.id);
            let taken = hook::run(
                &delegate,
                &cfg.repo_root,
                &cfg.mock_dir,
                &target,
                ask.field,
                ask.value,
            )?;
            if taken == hook::Taken::Whole {
                return Ok(());
            }
        }
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    match edit::append(&text, ask.ns, ask.id, ask.field, ask.value)? {
        Some(next) => {
            std::fs::write(&path, next).map_err(|e| format!("{}: {e}", path.display()))
        },
        None => Ok(()),
    }
}
