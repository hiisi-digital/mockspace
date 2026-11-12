//! Each migration of a consumer stopped part way and finished by the same call, for the consumer
//! whose mock directory is not called `mock`, the one with every odd kind of file and the one of
//! thousands of files. A `directory-to-ref` run is stopped at every git call it makes, one run for
//! each: once by the call failing, and once by the process that makes it being killed before it
//! runs, with a signal it cannot handle, so none of its own error handling runs; and once by the
//! whole process group, the command and the git call it made, killed while git holds a lock, which
//! git then names and the person removes, as git tells them to. A migration
//! that only renames or writes files is stopped after each file. The run that finishes it leaves what
//! a run nobody stopped leaves: the same files, index, hiding of the instances, hooks and ref trees.
//! Nothing is lost on the way, and the call made once more is refused.

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

fn strs(a: &[String]) -> Vec<&str> {
    a.iter().map(String::as_str).collect()
}

/// A remote of a consumer with its first `k` calls made, each committed, and pushed.
fn prepared(s: &Shape, k: usize) -> World {
    let w = consumer::remote(s);
    {
        let c = w.clone_of("prep");
        for args in s.calls().iter().take(k) {
            c.ok(&strs(args));
            c.commit_all("chore: a migration");
        }
        c.push();
    }
    w
}

/// A world of its own whose remote is a copy of `prep`'s, with the store its filtered files are in.
fn copy_of(prep: &World) -> World {
    let w = World::new();
    std::fs::remove_dir_all(&w.remote).unwrap();
    let o = w.sh(&w.base(), "cp", &["-a", prep.remote.to_str().unwrap(), w.remote.to_str().unwrap()]);
    assert!(o.ok, "{}", o.all());
    let store = prep.base().join("lfs-store");
    if store.exists() {
        let o = w.sh(&w.base(), "cp", &["-a", store.to_str().unwrap(), w.base().join("lfs-store").to_str().unwrap()]);
        assert!(o.ok, "{}", o.all());
    }
    w
}

/// A person's clone: the refs it declares instantiated, build output in the directories, and the
/// engine's first run made.
fn seat<'w>(s: &Shape, w: &'w World) -> Clone<'w> {
    let c = w.clone_of("a");
    if String::from_utf8_lossy(&c.read("mockspace.toml")).contains("[[data_ref]]") {
        c.ok(&["data-refs", "instantiate"]);
    }
    s.add_ignored_output(&c);
    s.add_own_setup(&c);
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

/// How a run is stopped at a git call.
#[derive(Clone, Copy, PartialEq)]
enum Stop {
    /// The call fails, and the engine handles the failure.
    Fail,
    /// The process that makes the call is killed before it runs, and handles nothing.
    Kill,
    /// The call is started, and the whole group, the command and the call, is killed while git holds
    /// a lock: what Ctrl-C at a terminal reaches, with a signal nothing handles, so the lock stays.
    Group,
}

/// Call `k` stopped at each git call it makes, then made again.
fn stopped_at_each_git_call(s: &Shape, k: usize, stop: Stop) {
    let calls = s.calls();
    let args = strs(&calls[k]);
    let prep = prepared(s, k);
    let clean_w = copy_of(&prep);
    let clean = seat(s, &clean_w);
    let counted = spy::install(&clean_w);
    counted.clear();
    clean.ok(&args);
    let made = counted.spawns();
    let want = left(&clean);
    assert!(made > 0, "the control: {args:?} makes git calls");
    // the number of calls is not fixed from run to run (a file's stamp is trusted or not by the
    // clock), so a run may not reach a call the clean run made; it must then end as a run nobody
    // stopped
    let stopped = AtomicUsize::new(0);
    let locked = AtomicUsize::new(0);
    each(made, "stopped at git call", |i| {
        let n = i + 1;
        let w = copy_of(&prep);
        let c = seat(s, &w);
        let spy = spy::install(&w);
        let before = whole::capture(&c);
        spy.clear();
        match stop {
            Stop::Fail => spy.fail_at(n),
            Stop::Kill => spy.kill_at(n),
            Stop::Group => spy.group_kill_at(n, &c.root.join(".git")),
        }
        let first = c.run(&args);
        spy.stop_failing();
        let mark = if stop == Stop::Fail { "INJECTED " } else { "KILLED " };
        let failed = spy.text().lines().find(|l| l.starts_with(mark)).unwrap_or("none").to_string();
        if failed != "none" {
            stopped.fetch_add(1, Ordering::SeqCst);
        } else if !first.ok {
            return Err(format!("no call {n} was reached, and the run failed\nfirst: {}", first.all()));
        }
        let left_behind = locks_in(&c.root.join(".git"));
        if !left_behind.is_empty() {
            locked.fetch_add(1, Ordering::SeqCst);
        }
        if stop != Stop::Fail && failed != "none" && first.ok {
            return Err(format!("the control: a killed run reads as done; {failed}\nfirst: {}", first.all()));
        }
        let mut finish = c.run(&args);
        // git names a lock a killed call left and says to remove it; the person does, and makes the
        // same call again, as often as a new lock is named
        let mut cleared: Vec<String> = Vec::new();
        while !finish.ok {
            let fresh: Vec<String> =
                locks_named(&finish.all()).into_iter().filter(|p| !cleared.contains(p) && std::path::Path::new(p).exists()).collect();
            if fresh.is_empty() {
                break;
            }
            for p in &fresh {
                std::fs::remove_file(p).unwrap();
            }
            cleared.extend(fresh);
            finish = c.run(&args);
        }
        let ctx = format!("{failed}\nleft: {left_behind:?}\ncleared: {cleared:?}\nfirst: {}\nfinish: {}", first.all(), finish.all());
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
        whole::kept(&c, &before, &whole::capture(&c), &s.allowed(k)).map_err(|e| format!("lost: {e}\n{ctx}"))?;
        let d = differ(&want, &left(&c));
        if d.is_empty() { Ok(()) } else { Err(format!("left other than a run nobody stopped:\n{}\n{ctx}", d.join("\n"))) }
    });
    let stopped = stopped.into_inner();
    assert!(stopped > 0, "the control: no run of {made} was stopped at its call");
    // the moves hand git their files on its input while it holds the index or a ref, so a kill there
    // leaves the lock; the renames and the reference rewrite write the files themselves, and their
    // only git writes are the gate's `config` sets, which take no input to hold them at the lock
    let (whole, by_pattern, _) = moves_of(s);
    if stop == Stop::Group && (k == whole || k == by_pattern) {
        assert!(locked.into_inner() > 0, "the control: no run of {made} killed with its group left a lock behind");
    }
}

/// The lock files under `git_dir`, the objects left out.
fn locks_in(git_dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut dirs = vec![git_dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() && p != git_dir.join("objects") {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "lock") {
                out.push(p.display().to_string());
            }
        }
    }
    out.sort();
    out
}

/// The paths of the lock files a message quotes, as git quotes one it could not take.
fn locks_named(text: &str) -> Vec<String> {
    text.split('\'').filter(|p| p.starts_with('/') && p.ends_with(".lock")).map(str::to_owned).collect()
}

/// The calls of `s`: the renames, each move and the reference rewrite, by their place in the list.
fn moves_of(s: &Shape) -> (usize, usize, usize) {
    let calls = s.calls();
    let by_pattern = calls.len() - 2;
    assert_eq!(calls[by_pattern][3], "templates", "the control: the last move is the one by pattern");
    (1, by_pattern, calls.len() - 1)
}

// Mutation: plan the renames from a list the run keeps of its own, or refuse a tree where some of
// the dated files are renamed already; the call then cannot finish what a stopped run began.
fn round_filenames_stopped_after_each_rename(s: &Shape) {
    let prep = prepared(s, 0);
    let renames = s.renames();
    let clean_w = copy_of(&prep);
    let clean = seat(s, &clean_w);
    clean.ok(&["migrate", "round-filenames"]);
    let want = left(&clean);
    each(renames.len(), "stopped after rename", |done| {
        let w = copy_of(&prep);
        let c = seat(s, &w);
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
fn the_reference_rewrite_stopped_after_each_file(s: &Shape) {
    let k = s.calls().len() - 1;
    let prep = prepared(s, k);
    let clean_w = copy_of(&prep);
    let clean = seat(s, &clean_w);
    let run = clean.ok(&["migrate", "references-to-letkajenkka"]);
    let want = left(&clean);
    // a file is named once for each reference it rewrites, and written once
    let mut order: Vec<String> = Vec::new();
    for p in run.stdout.lines().filter_map(|l| Some(l.strip_prefix("rewrite ")?.split(": ").next()?.to_string())) {
        if !order.contains(&p) {
            order.push(p);
        }
    }
    assert_eq!(order.len(), s.rewritten().len(), "the control: the run names each file it writes\n{}", run.stdout);
    let written: Vec<Vec<u8>> = order.iter().map(|p| clean.read(p)).collect();
    each(order.len(), "stopped after file", |done| {
        let w = copy_of(&prep);
        let c = seat(s, &w);
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

/// The same scenarios for each shape, as tests of their own so they run beside each other.
macro_rules! for_shape {
    ($m:ident, $s:expr) => {
        mod $m {
            use super::*;

            const S: Shape = $s;

            #[test]
            fn round_filenames_stopped_at_each_git_call_is_finished_by_the_same_call() {
                stopped_at_each_git_call(&S, 0, Stop::Fail);
            }

            #[test]
            fn round_filenames_killed_at_each_git_call_is_finished_by_the_same_call() {
                stopped_at_each_git_call(&S, 0, Stop::Kill);
            }

            #[test]
            fn a_whole_directory_moved_to_a_ref_stopped_at_each_git_call_is_finished_by_the_same_call() {
                stopped_at_each_git_call(&S, moves_of(&S).0, Stop::Fail);
            }

            #[test]
            fn a_whole_directory_moved_to_a_ref_killed_at_each_git_call_is_finished_by_the_same_call() {
                stopped_at_each_git_call(&S, moves_of(&S).0, Stop::Kill);
            }

            #[test]
            fn a_whole_directory_moved_to_a_ref_killed_with_its_group_at_each_git_call_is_finished_by_the_same_call() {
                stopped_at_each_git_call(&S, moves_of(&S).0, Stop::Group);
            }

            #[test]
            fn a_directory_moved_by_pattern_killed_with_its_group_at_each_git_call_is_finished_by_the_same_call() {
                stopped_at_each_git_call(&S, moves_of(&S).1, Stop::Group);
            }

            #[test]
            fn round_filenames_killed_with_its_group_at_each_git_call_is_finished_by_the_same_call() {
                stopped_at_each_git_call(&S, 0, Stop::Group);
            }

            #[test]
            fn the_reference_rewrite_killed_with_its_group_at_each_git_call_is_finished_by_the_same_call() {
                stopped_at_each_git_call(&S, moves_of(&S).2, Stop::Group);
            }

            #[test]
            fn a_directory_moved_by_pattern_stopped_at_each_git_call_is_finished_by_the_same_call() {
                stopped_at_each_git_call(&S, moves_of(&S).1, Stop::Fail);
            }

            #[test]
            fn a_directory_moved_by_pattern_killed_at_each_git_call_is_finished_by_the_same_call() {
                stopped_at_each_git_call(&S, moves_of(&S).1, Stop::Kill);
            }

            #[test]
            fn the_reference_rewrite_stopped_at_each_git_call_is_finished_by_the_same_call() {
                stopped_at_each_git_call(&S, moves_of(&S).2, Stop::Fail);
            }

            #[test]
            fn the_reference_rewrite_killed_at_each_git_call_is_finished_by_the_same_call() {
                stopped_at_each_git_call(&S, moves_of(&S).2, Stop::Kill);
            }

            #[test]
            fn round_filenames_stopped_after_each_rename_is_finished_by_the_same_call() {
                round_filenames_stopped_after_each_rename(&S);
            }

            #[test]
            fn the_reference_rewrite_stopped_after_each_file_is_finished_by_the_same_call() {
                the_reference_rewrite_stopped_after_each_file(&S);
            }
        }
    };
}

// the mock directory is not called `mock`, so every path the engine works out from it is tried
for_shape!(elsewhere, consumer::ELSEWHERE);
// symlinks, executables, binaries, CRLF, a name that is not UTF-8, a filter, and a sparse checkout
for_shape!(odd, consumer::ODD);
// a mock directory of thousands of files
for_shape!(large, consumer::LARGE);
