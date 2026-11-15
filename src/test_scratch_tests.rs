//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Tests for [`crate::test_scratch`].

use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::test_scratch::{SCRATCH_DIR, reap, root};

#[test]
fn a_stale_root_of_another_process_goes_and_a_fresh_one_and_our_own_stay() {
    let base = root().join("reap-fixture");
    let old = base.join("1");
    let fresh = base.join("2");
    let own = base.join(std::process::id().to_string());
    for d in [&old, &fresh, &own] {
        std::fs::create_dir_all(d.join("x")).unwrap();
    }
    let long_ago = SystemTime::now() - Duration::from_secs(7 * 60 * 60);
    for d in [&old, &own] {
        std::fs::File::open(d).unwrap().set_modified(long_ago).unwrap();
    }
    reap(&base, Duration::from_secs(6 * 60 * 60));
    assert!(!old.exists(), "a stale root of another process is removed");
    assert!(fresh.exists(), "a root added to within the stale time stays");
    assert!(own.exists(), "this process's own root stays whatever its age");
}

#[test]
fn scratch_lands_under_this_process_root() {
    let r = root();
    assert!(r.is_dir());
    assert!(r.ends_with(Path::new(SCRATCH_DIR).join(std::process::id().to_string())));
}
