//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------


//! The generated crate's lock follows the consumer's.
//!
//! The generated crate is its own workspace root under a shared key directory, so cargo writes
//! `Cargo.lock` there on the first build and nothing updates it afterwards. A git dependency on a
//! branch (a tool crate's own) was then pinned to whatever the tip was when that directory was first
//! built, differently per directory, and never followed `<mock>/Cargo.lock`. So before every build
//! the consumer's lock is put in place, and the pins it holds are the pins the pack builds.
//!
//! The identity reads the resolution through `cargo metadata`, which reads this lock, so a moved
//! pin is a moved member source and so another identity, never a library built against the old pin.
//!
//! holds for: a consumer with a `Cargo.lock` beside its workspace. With none, the lock already in the
//! generated directory is left as it is, and cargo resolves as it always did.

use std::path::Path;

use super::identity::digest_bytes;

/// Names the consumer lock the generated lock was last seeded from, so cargo's own trimmed lock is
/// not overwritten on every run while the consumer's has not moved.
const SEEDED: &str = ".seeded-lock";

/// Put `consumer_lock` in `gen_dir/Cargo.lock` unless that was already done for this content.
/// Cargo drops the entries the pack does not need and keeps the pins of those it does.
pub(crate) fn follow_consumer_lock(gen_dir: &Path, consumer_lock: &Path) -> Result<(), String> {
    let bytes = match std::fs::read(consumer_lock) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("could not read {}: {e}", consumer_lock.display())),
    };
    let digest = digest_bytes(&bytes);
    let (lock, marker) = (gen_dir.join("Cargo.lock"), gen_dir.join(SEEDED));
    if lock.exists() && std::fs::read_to_string(&marker).is_ok_and(|seen| seen == digest) {
        return Ok(());
    }
    std::fs::write(&lock, &bytes).map_err(|e| format!("could not write {}: {e}", lock.display()))?;
    std::fs::write(&marker, &digest).map_err(|e| format!("could not write {}: {e}", marker.display()))
}

#[cfg(test)]
#[path = "seed_tests.rs"]
mod tests;
