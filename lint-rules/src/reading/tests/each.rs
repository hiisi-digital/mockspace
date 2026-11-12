//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `each` splits a unit into items, each with a trace of its own, so one item moving rejudges that
//! item alone.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

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
