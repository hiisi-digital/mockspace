//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `each` splits a unit into items, each with a trace of its own, so one item moving rejudges that
//! item alone.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::reading::Item;

struct PerFile {
    items: Arc<AtomicUsize>,
}

impl Lint for PerFile {
    fn name(&self) -> &'static str {
        "per-file"
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn source_only(&self) -> bool {
        false
    }
}

impl RepoLint for PerFile {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        let files = ["a.txt", "b.txt", "c.txt"];
        ctx.view()
            .each(&files, |v, f| {
                self.items.fetch_add(1, Ordering::SeqCst);
                let text = v.text(f).unwrap_or("absent");
                vec![LintError::error("workspace".into(), 1, "per-file", format!("{f}={text}"))]
            })
            .into_iter()
            .flatten()
            .collect()
    }
}

#[test]
fn one_item_changing_rejudges_that_item_alone() {
    let dir = Dir::new("each");
    dir.write("a.txt", "1").write("b.txt", "1").write("c.txt", "1");
    let cache = Mem::default();
    let items = Arc::new(AtomicUsize::new(0));
    let pack = pack_of(PerFile {
        items: Arc::clone(&items),
    });

    let first = run_repo(&dir, &cache, &pack, None);
    assert_eq!(messages(&first.found), ["a.txt=1", "b.txt=1", "c.txt=1"]);
    assert_eq!(items.load(Ordering::SeqCst), 3);

    let _ = run_repo(&dir, &cache, &pack, None);
    assert_eq!(items.load(Ordering::SeqCst), 3, "nothing moved, so the unit replays whole");

    dir.write("b.txt", "2");
    let after = run_repo(&dir, &cache, &pack, None);
    assert_eq!(messages(&after.found), ["a.txt=1", "b.txt=2", "c.txt=1"]);
    assert_eq!(items.load(Ordering::SeqCst), 4, "only `b.txt` was judged again");
}

/// A repo lint splitting itself twice over the same names, each call judging them its own way.
struct Twice;

impl Lint for Twice {
    fn name(&self) -> &'static str {
        "twice"
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn source_only(&self) -> bool {
        false
    }
}

impl RepoLint for Twice {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        let names = ["a.txt", "b.txt"];
        let finding = |how: &str, f: &str, text: &str| LintError::error("workspace".into(), 1, "twice", format!("{how} {f}={text}"));
        let mut out: Vec<LintError> = ctx
            .view()
            .each(&names, |v, f| vec![finding("first", f, v.text(f).unwrap_or("absent"))])
            .into_iter()
            .flatten()
            .collect();
        out.extend(
            ctx.view()
                .each(&names, |v, f| vec![finding("second", f, &v.text(f).unwrap_or("absent").to_uppercase())])
                .into_iter()
                .flatten(),
        );
        out
    }
}

#[test]
fn two_calls_over_the_same_names_keep_their_items_apart() {
    let dir = Dir::new("each-twice");
    dir.write("a.txt", "x").write("b.txt", "y");
    let cache = Mem::default();
    let pack = pack_of(Twice);
    let want = ["first a.txt=x", "first b.txt=y", "second a.txt=X", "second b.txt=Y"];
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), want, "cold");
    dir.write("c.txt", "an unread file, so the unit is asked again and its items are looked up");
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), want, "warm");
}

/// A lint asking the view it splits rather than the item's own, while its items are judged.
struct Stray {
    items: Arc<AtomicUsize>,
}

impl Lint for Stray {
    fn name(&self) -> &'static str {
        "stray"
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn source_only(&self) -> bool {
        false
    }
}

impl RepoLint for Stray {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        let files = ["a.txt", "b.txt", "c.txt"];
        let outer = ctx.view();
        outer
            .each(&files, |v, f| {
                self.items.fetch_add(1, Ordering::SeqCst);
                let text = v.text(f).unwrap_or("absent");
                let side = outer.text("side.txt").unwrap_or("absent");
                vec![LintError::error("workspace".into(), 1, "stray", format!("{f}={text}+{side}"))]
            })
            .into_iter()
            .flatten()
            .collect()
    }
}

/// What an item read through the outer view is in no item's trace, so no item of that call is
/// saved, and one file moving judges every item again. `one_item_changing_rejudges_that_item_alone`
/// is the control: there only the moved item is judged.
#[test]
fn a_question_to_the_outer_view_saves_no_item() {
    let dir = Dir::new("each-stray");
    dir.write("a.txt", "1").write("b.txt", "1").write("c.txt", "1").write("side.txt", "s");
    let cache = Mem::default();
    let items = Arc::new(AtomicUsize::new(0));
    let pack = pack_of(Stray {
        items: Arc::clone(&items),
    });
    let _ = run_repo(&dir, &cache, &pack, None);
    assert_eq!(items.load(Ordering::SeqCst), 3);
    dir.write("b.txt", "2");
    let after = run_repo(&dir, &cache, &pack, None);
    assert_eq!(messages(&after.found), ["a.txt=1+s", "b.txt=2+s", "c.txt=1+s"]);
    assert_eq!(items.load(Ordering::SeqCst), 6, "no item was saved, so all three are judged again");
    dir.write("side.txt", "t");
    let moved = run_repo(&dir, &cache, &pack, None);
    assert_eq!(messages(&moved.found), ["a.txt=1+t", "b.txt=2+t", "c.txt=1+t"], "the outer read is still in the unit's trace");
}

/// A crate lint splitting each of its crates into slow items, every unit doing so at once.
struct Nested {
    working: Arc<AtomicUsize>,
    most:    Arc<AtomicUsize>,
}

impl Lint for Nested {
    fn name(&self) -> &'static str {
        "nested"
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }
}

impl crate::CrateLint for Nested {
    fn check(&self, ctx: &crate::LintContext) -> Vec<LintError> {
        let items: Vec<String> = (0 .. 8).map(|i| format!("{}-{i}", ctx.crate_name)).collect();
        ctx.view()
            .each(&items, |_, item| {
                let now = self.working.fetch_add(1, Ordering::SeqCst) + 1;
                self.most.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(10));
                self.working.fetch_sub(1, Ordering::SeqCst);
                vec![LintError::error(ctx.crate_name.to_string(), 1, "nested", item.clone())]
            })
            .into_iter()
            .flatten()
            .collect()
    }
}

/// Units splitting themselves on a run of four threads still have at most four at work, and the
/// items come back in their order.
#[test]
fn items_split_inside_running_units_share_the_runs_threads() {
    let names = ["a", "b", "c", "d", "e", "f"];
    let crates: Vec<(&str, &[(&str, &str)])> = names.iter().map(|n| (*n, &[("src/lib.rs", "pub fn f() {}\n")][..])).collect();
    let w = Workspace::new("each-nested", &crates);
    let working = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));
    let pack = LintPack {
        crate_lints: vec![Box::new(Nested {
            working: Arc::clone(&working),
            most:    Arc::clone(&most),
        })],
        ..LintPack::default()
    };
    let reading = Reading::new(&w.dir.0).with_threads(4);
    let found = w.run_with(&reading, None, None, Some(&pack));
    let nested: Vec<String> = found.iter().filter(|e| e.lint_name == "nested").map(|e| e.message.clone()).collect();
    let want: Vec<String> = names.iter().flat_map(|n| (0 .. 8).map(move |i| format!("{n}-{i}"))).collect();
    assert_eq!(nested, want);
    let most = most.load(Ordering::SeqCst);
    assert!(most <= 4, "{most} items were judged at once on a run of four threads");
    assert!(most >= 2, "the control: the run used its threads ({most})");
}

/// The items of two `each` calls in one unit, the first made only when `flag` says so.
fn first_and_second(v: &View<'_>) -> String {
    let mut out = Vec::new();
    if v.text("flag") == Some("yes") {
        out.extend(v.each(&["a"], |iv, f| one(format!("first:{}", iv.text(format!("{f}.txt")).unwrap_or("-")))));
    }
    out.extend(v.each(&["a"], |iv, f| one(format!("second:{}", iv.text(format!("{f}.txt")).unwrap_or("-")))));
    out.into_iter().flatten().map(|e| e.message).collect::<Vec<_>>().join(",")
}

fn one(message: String) -> Vec<LintError> {
    vec![LintError::error("workspace".into(), 1, "probe", message)]
}

/// An `each` skipped on one run does not hand its saved items to the call after it: the items
/// are keyed by the place each call is made from, and the second call's place never moved.
#[test]
fn a_skipped_call_does_not_hand_its_items_to_the_next() {
    let dir = Dir::new("each-skipped");
    dir.write("a.txt", "x");
    dir.write("flag", "yes");
    let cache = Mem::default();
    let (probe, _) = Probe::new(first_and_second);
    let pack = pack_of(probe);
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["first:x,second:x"]);
    dir.write("flag", "no");
    let fresh = messages(&run_repo(&dir, &Mem::default(), &pack, None).found);
    assert_eq!(fresh, ["second:x"], "the control: what a run with nothing saved finds");
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), fresh);
}

/// An item that asks the outer view a question and then calls `each` on it is still kept from
/// being saved: the inner call does not clear the mark the question left.
#[test]
fn an_each_inside_an_item_keeps_the_outer_question_marked() {
    fn ask(outer: &View<'_>) -> String {
        outer
            .each(&["a"], |iv, f| {
                let side = outer.text("side.txt").unwrap_or("-").to_string();
                let _ = outer.each(&["z"], |_, _| Vec::new());
                one(format!("{f}:{}+{side}", iv.text(format!("{f}.txt")).unwrap_or("-")))
            })
            .into_iter()
            .flatten()
            .map(|e| e.message)
            .collect()
    }
    let dir = Dir::new("each-stray-nested");
    dir.write("a.txt", "x");
    dir.write("side.txt", "s");
    let cache = Mem::default();
    let (probe, _) = Probe::new(ask);
    let pack = pack_of(probe);
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["a:x+s"]);
    dir.write("side.txt", "t");
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["a:x+t"]);
}

/// Whether `T` is an [`Item`], answered without needing it to be one: the inherent method is
/// taken where the bound holds and the trait's otherwise.
struct IsItem<T: ?Sized>(std::marker::PhantomData<T>);

trait NotItem {
    fn holds(&self) -> bool {
        false
    }
}

impl<T: ?Sized> NotItem for IsItem<T> {}

impl<T: Item + ?Sized> IsItem<T> {
    fn holds(&self) -> bool {
        true
    }
}

/// A pair is no item: keyed on its first half alone, a second half worked out from what the unit
/// read would replay stale. An item carries what tells it apart in its key.
#[test]
fn a_pair_is_not_an_item() {
    assert!(!IsItem::<(&str, usize)>(std::marker::PhantomData).holds());
    assert!(IsItem::<&str>(std::marker::PhantomData).holds(), "the control");
}
