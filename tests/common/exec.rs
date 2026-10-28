//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A test's name for the one writer of an executable file.
//!
//! The writer is `mockspace_manifest::write_executable`, which the engine writes
//! its own hooks with, and its header says what race it avoids and how. This is
//! the panicking form a test wants, included by path from the lib tests
//! (`src/test_exec.rs`) and from the integration tests.

#![allow(dead_code)]

use std::path::Path;

/// Write `body` at `path` as an executable file that is safe to exec at once.
pub fn write_executable(path: &Path, body: impl AsRef<[u8]>) {
    mockspace_manifest::write_executable(path, body)
        .unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}
