//! Each migration of a consumer stopped part way and finished by the same call. A `directory-to-ref`
//! run is stopped at every git call it makes, one run for each; a migration that only renames or
//! writes files is stopped after each file. The run that finishes it leaves what a run nobody stopped
//! leaves: the same files, index, hiding of the instances, hooks and ref trees. Nothing is lost on
//! the way, and the call made once more is refused.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use refs_e2e::consumer::{self, Shape};
use refs_e2e::spy;
use refs_e2e::whole::{self, Allowed};
use refs_e2e::{Clone, World};

/// The mock directory is not called `mock`, so every path the engine works out from it is tried.
const S: Shape = consumer::ELSEWHERE;

fn strs(a: &[String]) -> Vec<&str> {
    a.iter().map(String::as_str).collect()
}

/// A remote of a consumer with its first `k` calls made, each committed, and pushed.
fn prepared(k: usize) -> World {
    let w = consumer::remote(&S);
    {
        let c = w.clone_of("prep");
        for args in S.calls().iter().take(k) {
            c.ok(&strs(args));
            c.commit_all("chore: a migration");
        }
        c.push();
    }
    w
}

/// A world of its own whose remote is a copy of `prep`'s.
fn copy_of(prep: &World) -> World {
    let w = World::new();
    std::fs::remove_dir_all(&w.remote).unwrap();
    let o = w.sh(&w.base(), "cp", &["-a", prep.remote.to_str().unwrap(), w.remote.to_str().unwrap()]);
    assert!(o.ok, "{}", o.all());
    w
}

/// A person's clone: the refs it declares instantiated, build output in the directories, and the
/// engine's first run made.
fn seat(w: &World) -> Clone<'_> {
    let c = w.clone_of("a");
    if String::from_utf8_lossy(&c.read("mockspace.toml")).contains("[[data_ref]]") {
        c.ok(&["data-refs", "instantiate"]);
    }
    S.add_ignored_output(&c);
    let _ = c.run(&["status"]);
    c
}

/// What a migration leaves that a person or a later command reads, each part as text, with the
/// world's own directory taken out so two worlds compare.
fn left(c: &Clone) -> Vec<(String, String)> {
    let base = c.w.base().display().to_string();
    let mut out = Vec::new();
    for (p, e) in whole::disk(&c.root).into_iter().filter(|(p, _)| !whole::engine_output(p)) {
        out.push((format!("file {}", whole::shown(&p)), format!("{e:?}").chars().take(200).collect()));
    }
    out.push(("index".into(), c.git(&["ls-files", "-s", "-t"])));
    let git_dir = c.root.join(".git");
    for f in ["info/exclude", "info/sparse-checkout"] {
        out.push((f.into(), String::from_utf8_lossy(&std::fs::read(git_dir.join(f)).unwrap_or_default()).into_owned()));
    }
    for e in std::fs::read_dir(git_dir.join("hooks")).unwrap().flatten() {
        let body = String::from_utf8_lossy(&std::fs::read(e.path()).unwrap()).replace(&base, "<base>");
        out.push((format!("hook {}", e.file_name().to_string_lossy()), body));
    }
    let config = c.git_out(&["config", "--get-regexp", r"^core\.(sparse|hookspath)|^extensions\.|^index\.sparse"]);
    out.push(("config".into(), config.stdout.replace(&base, "<base>")));
    for (side, dir) in [("here", c.root.as_path()), ("remote", c.w.remote.as_path())] {
        let names = c.w.sh(dir, "git", &["for-each-ref", "--format=%(refname)", "refs/heads/mock/"]).stdout;
        for r in names.lines() {
            let tree = c.w.sh(dir, "git", &["rev-parse", &format!("{r}^{{tree}}")]).stdout;
            out.push((format!("{side} {r}"), tree.trim().to_string()));
        }
    }
    out.sort();
    out
}

/// Every part where `got` and `want` differ, named.
fn differ(want: &[(String, String)], got: &[(String, String)]) -> Vec<String> {
    let w: std::collections::BTreeMap<_, _> = want.iter().cloned().collect();
    let g: std::collections::BTreeMap<_, _> = got.iter().cloned().collect();
    let mut out = Vec::new();
    for (k, v) in &w {
        match g.get(k) {
            None => out.push(format!("{k}: missing")),
            Some(x) if x != v => out.push(format!("{k}: {v:?} became {x:?}")),
            _ => {},
        }
    }
    out.extend(g.keys().filter(|k| !w.contains_key(*k)).map(|k| format!("{k}: only here")));
    out
}

/// What call `k` may change and still have lost nothing.
fn allowed(k: usize) -> Allowed {
    let calls = S.calls();
    if k == 0 {
        let renamed = S.renames().iter().map(|(f, t)| (f.clone().into_bytes(), t.clone().into_bytes())).collect();
        Allowed { renamed, ..Allowed::default() }
    } else if k == calls.len() - 1 {
        Allowed { rewritten: S.rewritten(), ..Allowed::default() }
    } else {
        Allowed { rewritten: [b"mockspace.toml".to_vec()].into(), ..Allowed::default() }
    }
}

/// Runs `one` for each of `0 .. n` on two threads, a world each, and fails naming every case
/// that failed.
fn each(n: usize, what: &str, one: impl Fn(usize) -> Result<(), String> + Sync) {
    let next = AtomicUsize::new(0);
    let bad = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0 .. 2 {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= n {
                        break;
                    }
                    let r = catch_unwind(AssertUnwindSafe(|| one(i))).unwrap_or_else(|p| {
                        Err(p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default())
                    });
                    if let Err(e) = r {
                        bad.lock().unwrap().push(format!("{what} {i}: {e}"));
                    }
                }
            });
        }
    });
    let mut bad = bad.into_inner().unwrap();
    bad.sort();
    assert!(bad.is_empty(), "{} of {n} failed:\n\n{}", bad.len(), bad.join("\n\n"));
}

/// Call `k` stopped at each git call it makes, then made again.
fn stopped_at_each_git_call(k: usize) {
    let calls = S.calls();
    let args = strs(&calls[k]);
    let prep = prepared(k);
    let clean_w = copy_of(&prep);
    let clean = seat(&clean_w);
    let counted = spy::install(&clean_w);
    counted.clear();
    clean.ok(&args);
    let made = counted.spawns();
    let want = left(&clean);
    assert!(made > 0, "the control: {args:?} makes git calls");
    each(made, "stopped at git call", |i| {
        let n = i + 1;
        let w = copy_of(&prep);
        let c = seat(&w);
        let spy = spy::install(&w);
        let before = whole::capture(&c);
        spy.clear();
        spy.fail_at(n);
        let first = c.run(&args);
        spy.stop_failing();
        let failed = spy.text().lines().find(|l| l.starts_with("INJECTED ")).unwrap_or("none").to_string();
        let finish = c.run(&args);
        let ctx = format!("{failed}\nfirst: {}\nfinish: {}", first.all(), finish.all());
        if first.ok {
            // the stopped call did not stop the run; it must have left what the clean run did
            if finish.ok {
                return Err(format!("made twice; {ctx}"));
            }
        } else if !finish.ok && !(finish.all().contains("made here before") && differ(&want, &left(&c)).is_empty()) {
            // a step stopped at a call whose work an earlier move had done already leaves the move
            // finished, and the same call then says so
            return Err(format!("not finished by the same call; {ctx}"));
        }
        if c.run(&args).ok {
            return Err(format!("made again after it was finished; {ctx}"));
        }
        whole::kept(&c, &before, &whole::capture(&c), &allowed(k)).map_err(|e| format!("lost: {e}\n{ctx}"))?;
        let d = differ(&want, &left(&c));
        if d.is_empty() { Ok(()) } else { Err(format!("left other than a run nobody stopped:\n{}\n{ctx}", d.join("\n"))) }
    });
}

#[test]
fn round_filenames_stopped_at_each_git_call_is_finished_by_the_same_call() {
    stopped_at_each_git_call(0);
}

#[test]
fn a_whole_directory_moved_to_a_ref_stopped_at_each_git_call_is_finished_by_the_same_call() {
    stopped_at_each_git_call(1);
}

#[test]
fn a_directory_moved_by_pattern_stopped_at_each_git_call_is_finished_by_the_same_call() {
    let last = S.calls().len() - 2;
    assert_eq!(S.calls()[last][3], "templates", "the control: the last move is the one by pattern");
    stopped_at_each_git_call(last);
}

#[test]
fn the_reference_rewrite_stopped_at_each_git_call_is_finished_by_the_same_call() {
    stopped_at_each_git_call(S.calls().len() - 1);
}

// Mutation: plan the renames from a list the run keeps of its own, or refuse a tree where some of
// the dated files are renamed already; the call then cannot finish what a stopped run began.
#[test]
fn round_filenames_stopped_after_each_rename_is_finished_by_the_same_call() {
    let prep = prepared(0);
    let renames = S.renames();
    let clean_w = copy_of(&prep);
    let clean = seat(&clean_w);
    clean.ok(&["migrate", "round-filenames"]);
    let want = left(&clean);
    each(renames.len(), "stopped after rename", |done| {
        let w = copy_of(&prep);
        let c = seat(&w);
        for (f, t) in &renames[.. done] {
            std::fs::rename(c.path(f), c.path(t)).unwrap();
        }
        let before = whole::capture(&c);
        let finish = c.run(&["migrate", "round-filenames"]);
        if !finish.ok {
            return Err(format!("not finished by the same call: {}", finish.all()));
        }
        if c.run(&["migrate", "round-filenames"]).ok {
            return Err("made again after it was finished".into());
        }
        let rest = renames[done ..].iter().map(|(f, t)| (f.clone().into_bytes(), t.clone().into_bytes())).collect();
        whole::kept(&c, &before, &whole::capture(&c), &Allowed { renamed: rest, ..Allowed::default() })?;
        let d = differ(&want, &left(&c));
        if d.is_empty() { Ok(()) } else { Err(d.join("\n")) }
    });
}

// Mutation: rewrite a file whose references are in the standard spelling already, or refuse a tree
// where some are; the call then cannot finish what a stopped run began, or writes a file twice.
#[test]
fn the_reference_rewrite_stopped_after_each_file_is_finished_by_the_same_call() {
    let k = S.calls().len() - 1;
    let prep = prepared(k);
    let clean_w = copy_of(&prep);
    let clean = seat(&clean_w);
    let run = clean.ok(&["migrate", "references-to-letkajenkka"]);
    let want = left(&clean);
    // a file is named once for each reference it rewrites, and written once
    let mut order: Vec<String> = Vec::new();
    for p in run.stdout.lines().filter_map(|l| Some(l.strip_prefix("rewrite ")?.split(": ").next()?.to_string())) {
        if !order.contains(&p) {
            order.push(p);
        }
    }
    assert_eq!(order.len(), S.rewritten().len(), "the control: the run names each file it writes\n{}", run.stdout);
    let written: Vec<Vec<u8>> = order.iter().map(|p| clean.read(p)).collect();
    each(order.len(), "stopped after file", |done| {
        let w = copy_of(&prep);
        let c = seat(&w);
        for (p, b) in order.iter().zip(&written).take(done) {
            c.write(p, b);
        }
        let before = whole::capture(&c);
        let finish = c.run(&["migrate", "references-to-letkajenkka"]);
        if !finish.ok {
            return Err(format!("not finished by the same call: {}", finish.all()));
        }
        if c.run(&["migrate", "references-to-letkajenkka"]).ok {
            return Err("made again after it was finished".into());
        }
        let rest = Allowed { rewritten: order[done ..].iter().map(|p| p.clone().into_bytes()).collect(), ..Allowed::default() };
        whole::kept(&c, &before, &whole::capture(&c), &rest)?;
        let d = differ(&want, &left(&c));
        if d.is_empty() { Ok(()) } else { Err(d.join("\n")) }
    });
}
