//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a test must take out of a child's environment so the machine it runs on
//! cannot answer for the repository or the identity under test.
//!
//! The lists and the scrubs live in `mockspace_manifest::ambient`, the one
//! package every target here can reach and the one that publishes its tests, so
//! there is a single copy and a package that ships scrubs from inside itself.
//! This file is what a target includes by path: it re-exports them and expands
//! the scrub that runs before `main`, in the including crate.

#![allow(dead_code, unused_imports)]

pub use mockspace_manifest::ambient::*;

mockspace_manifest::scrub_before_main!();
