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

use std::process::ExitCode;

use crate::RegistryEnv;
use crate::{RegistryStores, RegistryWriter, load_registry_from};

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
    let &[ns, id] = crate::names(target).unwrap_or_default().as_slice() else {
        return Err(format!("`{target}` is not <namespace>::<id>\n{USAGE}"));
    };
    Ok(Ask {
        ns,
        id,
        field,
        value,
        local,
    })
}

/// The file that holds the row, as a path under the registry root, after the namespace and
/// the field are checked against what the project declares.
fn file_of(cfg: &dyn RegistryEnv, stores: &RegistryStores, ask: &Ask<'_>) -> Result<String, String> {
    let namespace = cfg
        .namespaces()
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
    let reg = load_registry_from(stores, cfg.namespaces(), None).map_err(|e| e.to_string())?;
    let source = reg
        .get(&format!("{}::{}", ask.ns, ask.id))
        .map(|r| r.source.clone())
        .ok_or_else(|| format!("no row {}::{}", ask.ns, ask.id))?;
    stores
        .rel_of(&source)
        .ok_or_else(|| format!("{} lies outside the registry", source.display()))
}

/// The subcommand. `args` follow `registry-append`.
pub fn run(cfg: &dyn RegistryEnv, args: &[&str]) -> ExitCode {
    match step(cfg, args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mock registry-append: {e}");
            ExitCode::FAILURE
        },
    }
}

fn step(cfg: &dyn RegistryEnv, args: &[&str]) -> Result<(), String> {
    let ask = parse(args)?;
    let stores = cfg.registry_stores().map_err(|e| e.to_string())?;
    let rel = file_of(cfg, &stores, &ask)?;
    // A delegate edits the tree, and the tree's copy of a directory kept on a ref is hidden, so
    // what it wrote there would be lost: the write goes through the writer.
    let kept_on_a_ref = stores.route(&rel).1 != ".";
    if !ask.local && !kept_on_a_ref {
        if let Some(delegate) = hook::find(cfg.repo_root()) {
            let target = format!("{}::{}", ask.ns, ask.id);
            let taken = hook::run(
                &delegate,
                cfg.repo_root(),
                cfg.mock_dir(),
                &target,
                ask.field,
                ask.value,
            )?;
            if taken == hook::Taken::Whole {
                return Ok(());
            }
        }
    }
    let text = stores
        .read(&rel)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("{rel}: the file is gone"))
        .and_then(|b| String::from_utf8(b).map_err(|_| format!("{rel} is not UTF-8")))?;
    match edit::append(&text, ask.ns, ask.id, ask.field, ask.value)? {
        Some(next) => {
            let message = format!("registry: add to `{}` of {}::{}", ask.field, ask.ns, ask.id);
            RegistryWriter::new(cfg, &stores)
                .write(&[(rel, Some(next.into_bytes()))], &message)
                .map(|_| ())
                .map_err(|e| e.to_string())
        },
        None => Ok(()),
    }
}
