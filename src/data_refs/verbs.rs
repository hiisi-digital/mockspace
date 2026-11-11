//! `cargo mock data-refs`, and what the phase gates read of the refs.
//!
//! Every verb but `instantiate` looks at the instance before it does anything else: the files
//! edited there are copied into the backup ref, and the files git wrote over since the last look
//! are found (see `mockspace_store::Instance::look`). A copy that cannot be made is said and fails
//! no command that only reads; `commit` alone refuses where the copy cannot be read, since it would
//! push bytes git wrote without knowing it.

use std::process::ExitCode;

use mockspace_store::{Instance, Look, Refuse, Store};

use super::{DataRefDecl, WHOLE, landing, sparse};

/// What one verb was given besides its namespace.
struct Given<'a> {
    verb:     &'a str,
    wanted:   Option<&'a str>,
    paths:    Vec<String>,
    /// `commit --accept-overwritten`: the bytes on disk are meant, where git wrote over a file.
    accept:   bool,
}

const VERBS: &str = "status, verify, instantiate, update, restore, commit, snapshot, trailers or paired";

fn given<'a>(verb: &'a str, rest: &[&'a str]) -> Result<Given<'a>, String> {
    let mut g = Given {
        verb,
        wanted: None,
        paths: Vec::new(),
        accept: false,
    };
    for arg in rest {
        match *arg {
            "--accept-overwritten" if verb == "commit" => g.accept = true,
            flag if flag.starts_with("--") => return Err(format!("data-refs: `{verb}` takes no `{flag}`")),
            ns if g.wanted.is_none() => g.wanted = Some(ns),
            path if verb == "restore" => g.paths.push(path.to_owned()),
            extra => return Err(format!("data-refs: `{verb}` takes one namespace, and `{extra}` is a second")),
        }
    }
    Ok(g)
}

/// `cargo mock data-refs <status|verify|instantiate|update|restore|commit|snapshot|trailers>
/// [namespace]`, `cargo mock data-refs restore <namespace> <path>...`, `cargo mock data-refs commit
/// [namespace] --accept-overwritten` and `cargo mock data-refs paired <commit>`. Prints one line per
/// ref it touched, and a refusal on stderr; exits non-zero when any ref could not be done.
pub fn run(cfg: &crate::config::Config, args: &[&str]) -> ExitCode {
    let Some((verb, rest)) = args.split_first() else {
        eprintln!("data-refs: status, verify, instantiate, update, restore, commit, snapshot or trailers [namespace], or paired <commit>");
        return ExitCode::FAILURE;
    };
    match *verb {
        "trailers" => {
            return match landing::trailer_lines(cfg, rest.first().copied()) {
                Ok(lines) => {
                    lines.iter().for_each(|l| println!("{l}"));
                    ExitCode::SUCCESS
                },
                Err(why) => {
                    eprintln!("{why}");
                    ExitCode::FAILURE
                },
            };
        },
        "paired" => {
            let Some(rev) = rest.first() else {
                eprintln!("data-refs: `paired` takes the commit to check");
                return ExitCode::FAILURE;
            };
            return match landing::paired(cfg, rev) {
                Ok(lines) => {
                    lines.iter().for_each(|l| println!("{l}"));
                    ExitCode::SUCCESS
                },
                Err(why) => {
                    why.iter().for_each(|l| eprintln!("{l}"));
                    ExitCode::FAILURE
                },
            };
        },
        "status" | "verify" | "instantiate" | "update" | "restore" | "commit" | "snapshot" => {},
        _ => {
            eprintln!("data-refs: `{verb}` is none of {VERBS}");
            return ExitCode::FAILURE;
        },
    }
    let g = match given(verb, rest) {
        Ok(g) => g,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        },
    };
    // a branch cut before a migration declares none of the refs its worktree still holds, so the copy of
    // the files goes by the declarations this worktree last installed as well
    let remembered = if *verb == "snapshot" { sparse::remembered(cfg) } else { Vec::new() };
    let decls: Vec<&DataRefDecl> = cfg
        .data_refs
        .iter()
        .chain(remembered.iter().filter(|r| !cfg.data_refs.iter().any(|d| d.namespace() == r.namespace())))
        .filter(|d| g.wanted.is_none_or(|w| d.namespace() == w))
        .collect();
    if decls.is_empty() {
        if let Some(w) = g.wanted {
            eprintln!("data-refs: no [[data_ref]] has the namespace `{w}`");
            return ExitCode::FAILURE;
        }
        // nothing is declared, and a block left by a ref that was is taken out, or named
        if *verb != "snapshot" {
            println!("data-refs: no [[data_ref]] is declared");
        }
        return if protect(cfg, verb, false) { ExitCode::FAILURE } else { ExitCode::SUCCESS };
    }
    let mut failed = false;
    for d in decls {
        let ns = d.namespace();
        let outcome = (|| -> Result<Vec<String>, String> {
            let store = d.open(&cfg.repo_root)?;
            let inst = d.instance(&store, &cfg.mock_dir)?;
            one(&g, ns, &store, &inst)
        })();
        match outcome {
            Ok(lines) => lines.iter().for_each(|l| println!("{ns}: {l}")),
            Err(why) => {
                eprintln!("{ns}: {why}");
                failed = true;
            },
        }
    }
    if protect(cfg, verb, failed) {
        failed = true;
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

/// The look every verb but `instantiate` takes first. `None` where it could not be taken and the
/// verb goes on without it; the reason is said.
fn look_first(g: &Given<'_>, ns: &str, inst: &Instance<'_>) -> Result<Option<Look>, String> {
    let look = match g.verb {
        "instantiate" => return Ok(None),
        "snapshot" => inst.guard(),
        _ => inst.look(),
    };
    match look {
        Ok(look) => {
            if let Err(e) = &look.copied {
                if g.verb == "snapshot" {
                    return Err(format!("the files of the instance could not be copied: {e}"));
                }
                eprintln!("{ns}: the copy of the files of the instance could not be made, and the {} goes on: {e}", g.verb);
            }
            Ok(Some(look))
        },
        Err(e) if matches!(g.verb, "commit" | "snapshot") => {
            // FIXME(the_way_past_a_copy_a_git_gc_pruned_is_provisional): a copy `git gc` pruned
            // fails every `commit` of this instance, `--accept-overwritten` included, as the design
            // has it; whether a way past it is given is op's call
            let lost = if inst.pending_is_lost().unwrap_or(false) {
                "; a `git gc` pruned that copy before an engine command took it, and no command of the engine goes past it"
            } else {
                ""
            };
            Err(format!("the copy of the files of the instance could not be read, so what git wrote over is not known: {e}{lost}"))
        },
        Err(e) => {
            eprintln!("{ns}: the copy of the files of the instance could not be made, and the {} goes on: {e}", g.verb);
            Ok(None)
        },
    }
}

/// The line that names a file git wrote over and the two ways on.
fn written_over(ns: &str, path: &str) -> String {
    format!(
        "written over: {path}; git wrote over it, the copy keeps the bytes from before, `cargo mock data-refs restore {ns} \
         {path}` writes them back, and `cargo mock data-refs commit {ns} --accept-overwritten` commits the bytes on disk"
    )
}

/// One verb on one ref: the lines it prints, prefixed with the namespace by the caller.
fn one(
    g: &Given<'_>,
    ns: &str,
    store: &mockspace_store::RefStore,
    inst: &Instance<'_>,
) -> Result<Vec<String>, String> {
    let look = look_first(g, ns, inst)?;
    let over: Vec<String> = look.as_ref().map(|l| l.written_over.clone()).unwrap_or_default();
    let e = |e: mockspace_store::StoreError| e.to_string();
    match g.verb {
        "snapshot" => Ok(Vec::new()),
        "status" => {
            let tip = store.local_tip().map_err(e)?;
            let Some(base) = inst.base().map_err(e)? else {
                return Err("has no instance, so `instantiate` it".into());
            };
            // a clean of the work tree, or a worktree made again at the same path, takes the
            // files and leaves the record; said apart from an edit, since the two are mended apart
            if let (0, held) = inst.present().map_err(e)? {
                if held > 0 {
                    // what git wrote over is said too, since a plain `restore` writes the base's bytes
                    let then = match over.as_slice() {
                        [] => String::new(),
                        over => format!(
                            ", then `cargo mock data-refs restore {ns} <path>` for each file git wrote over, which the \
                             copy keeps as it was: {}",
                            over.join(", ")
                        ),
                    };
                    return Err(format!("has none of its {held} file(s), so `restore` it{then}"));
                }
            }
            let mut lines: Vec<String> = inst.ignored().map_err(e)?.iter().map(|p| format!("ignored, not held: {p}")).collect();
            lines.extend(over.iter().map(|p| written_over(ns, p)));
            let dirty = inst.is_dirty().map_err(e)?;
            lines.push(format!(
                "tip {} base {} {}",
                tip.as_deref().unwrap_or("none"),
                base.commit().map_or("none", |c| c.as_str()),
                if dirty { "changed" } else { "clean" }
            ));
            Ok(lines)
        },
        "verify" => {
            let tip = store.local_tip().map_err(e)?;
            let Some(base) = inst.base().map_err(e)? else {
                return Err("has no instance, so `instantiate` it".into());
            };
            if let Some(path) = over.first() {
                return Err(written_over(ns, path));
            }
            if inst.is_dirty().map_err(e)? {
                return Err("has changes the ref does not, so `commit` them".into());
            }
            match (tip, base.commit()) {
                (None, None) => Ok(vec!["clean, the ref is not born yet".into()]),
                (Some(tip), Some(at)) if tip == at.as_str() => Ok(vec![format!("clean at {at}")]),
                (tip, _) => Err(format!("is at {base} and the ref is at {}", tip.as_deref().unwrap_or("none"))),
            }
        },
        "instantiate" => {
            let fresh = store.refresh().map_err(e)?;
            // a declared ref the remote does not have yet is a ref not born yet: its instance
            // holds no commit, and the first `commit` makes the ref
            if matches!(fresh, mockspace_store::Freshness::Empty) && store.local_tip().map_err(e)?.is_none() {
                if !inst.is_unborn().map_err(e)? {
                    inst.begin().map_err(e)?;
                }
                return Ok(vec!["not born yet, the first `commit` makes the ref".into()]);
            }
            let tip = inst.materialize().map_err(e)?;
            Ok(vec![format!("instantiated at {tip}")])
        },
        "update" => {
            if inst.base().map_err(e)?.is_none() {
                return Err("has no instance, so `instantiate` it".into());
            }
            inst.update().map_err(e)?;
            let tip = store.local_tip().map_err(e)?;
            Ok(vec![format!("at {}", tip.as_deref().unwrap_or("none"))])
        },
        "restore" if !g.paths.is_empty() => {
            inst.restore_written_over(&g.paths).map_err(e)?;
            Ok(vec![format!("{} file(s) written back as they were before git wrote over them", g.paths.len())])
        },
        "restore" => {
            let written = inst.restore().map_err(e)?;
            Ok(vec![format!("{written} file(s) written again")])
        },
        _ => commit(g, ns, store, inst, &over),
    }
}

fn commit(
    g: &Given<'_>,
    ns: &str,
    store: &mockspace_store::RefStore,
    inst: &Instance<'_>,
    over: &[String],
) -> Result<Vec<String>, String> {
    if !over.is_empty() && !g.accept {
        return Err(format!(
            "has files written over by git, whose bytes on disk are not committed without `--accept-overwritten`: {}; \
             `cargo mock data-refs restore {ns} <path>` writes back the bytes from before",
            over.join(", ")
        ));
    }
    let e = |e: mockspace_store::StoreError| e.to_string();
    let message = format!("data: {ns}");
    let first = inst.base().map_err(e)?.is_none() && store.local_tip().map_err(e)?.is_none();
    let applied = if first { inst.seed(&message, &Refuse) } else { inst.apply(&message, &Refuse) }.map_err(|e| {
        let text = e.to_string();
        // the way on where the ref moved: also where this instance's own commit landed on
        // the remote and the local ref was not moved after it
        if text.contains("merge hook refused") {
            format!("{text}; `data-refs update` brings the instance to the ref, and a commit follows")
        } else {
            text
        }
    })?;
    if !over.is_empty() {
        if let Err(why) = inst.accept_written_over().and_then(|look| look.copied) {
            eprintln!("{ns}: committed, and the copy still names the files git wrote over: {why}");
        }
    }
    Ok(vec![match applied.commit {
        Some(c) if applied.changed => format!("committed {c}"),
        _ => "nothing changed".to_string(),
    }])
}

/// The files the declared refs hold, and which of them differ from the commit their instance is
/// at, each as a path relative to the mock directory. The phase gates judge these files by it,
/// since git tracks none of them and so says nothing of what changed. Each instance is looked at
/// first, so its edits are copied; a copy that cannot be made is said and stops no gate.
pub fn ref_files(cfg: &crate::config::Config) -> Result<mockspace_lint_rules::src_layout::RefFiles, String> {
    let mut out = mockspace_lint_rules::src_layout::RefFiles::default();
    for d in &cfg.data_refs {
        let ns = d.namespace();
        let store = d.open(&cfg.repo_root).map_err(|e| format!("the data ref `{ns}`: {e}"))?;
        let inst = d.instance(&store, &cfg.mock_dir).map_err(|e| format!("the data ref `{ns}`: {e}"))?;
        match inst.look() {
            Ok(Look { copied: Ok(()), .. }) => {},
            Ok(Look { copied: Err(e), .. }) | Err(e) => {
                eprintln!("the data ref `{ns}`: the copy of the files of the instance could not be made: {e}");
            },
        }
        let (held, changed) = inst.files().map_err(|e| format!("the data ref `{ns}`: {e}"))?;
        let rel = d.instance.as_deref().or(d.path.as_deref()).unwrap_or("").trim_matches('/');
        let lead = if rel == WHOLE || rel.is_empty() { String::new() } else { format!("{rel}/") };
        out.held.extend(held.into_iter().map(|p| format!("{lead}{p}")));
        out.changed.extend(changed.into_iter().map(|p| format!("{lead}{p}")));
    }
    Ok(out)
}

/// The instances are kept from git's own writes by the sparse checkout of this worktree and the hook
/// that copies their files: set up by `instantiate` and the first `commit`, and checked by `verify`.
/// Says whether the run failed, `failed` being whether it already had.
fn protect(cfg: &crate::config::Config, verb: &str, failed: bool) -> bool {
    let done = match verb {
        "instantiate" => sparse::install(cfg),
        "commit" if !failed => sparse::install(cfg),
        "verify" if !failed => sparse::check(cfg),
        _ => Ok(()),
    };
    if let Err(why) = done {
        eprintln!("{why}");
        return true;
    }
    failed
}
