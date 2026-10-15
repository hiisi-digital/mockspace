//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What registers the driver and the hooks, and when it does not.

mod common;

use common::{BIN, Repo};

#[test]
fn running_mockspace_registers_the_driver_and_the_hooks() {
    let r = Repo::untouched();
    r.generate();
    assert!(r.config("merge.mockspace-generated.driver").is_some());
    assert!(r.config("core.hooksPath").is_some());
}

#[test]
fn the_opt_out_registers_no_driver_and_no_hooks() {
    let r = Repo::untouched();
    let out = r
        .cmd(BIN)
        .current_dir(r.root.join("mock"))
        .env("MOCKSPACE_NO_AUTO_ACTIVATE", "1")
        .arg("--generate-only")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", Repo::failure(&out));
    assert_eq!(r.config("merge.mockspace-generated.driver"), None);
    assert_eq!(r.config("merge.mockspace-generated.name"), None);
    assert_eq!(r.config("core.hooksPath"), None);
}
