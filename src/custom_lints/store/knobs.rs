//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Hooks the end-to-end tests use to make a race happen on purpose instead of hoping for one,
//! keyed by directory so a test that sets one cannot disturb another running beside it. Absent
//! from a build that is not a test.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

struct Knob {
    under:     PathBuf,
    skip_lock: bool,
    pause:     Duration,
}

static KNOBS: Mutex<Vec<Knob>> = Mutex::new(Vec::new());

/// For every crate directory under `under`: optionally take no lock, and
/// sleep `pause` between writing the crate and building it.
pub(crate) fn set(under: &Path, skip_lock: bool, pause: Duration) {
    KNOBS.lock().unwrap().push(Knob {
        under: under.to_path_buf(),
        skip_lock,
        pause,
    });
}

pub(crate) fn skips_the_lock(dir: &Path) -> bool {
    KNOBS
        .lock()
        .unwrap()
        .iter()
        .any(|k| dir.starts_with(&k.under) && k.skip_lock)
}

type Hook = std::sync::Arc<dyn Fn() + Send + Sync>;

static HOOKS: Mutex<Vec<(PathBuf, Hook)>> = Mutex::new(Vec::new());

/// For every crate directory under `under`: run `hook` once the identity is computed and
/// before the build starts, which is where an edit lands in the window the identity left open.
pub(crate) fn after_identity(under: &Path, hook: impl Fn() + Send + Sync + 'static) {
    HOOKS.lock().unwrap().push((under.to_path_buf(), std::sync::Arc::new(hook)));
}

pub(crate) fn run_after_identity(dir: &Path) {
    let hooks: Vec<Hook> = HOOKS
        .lock()
        .unwrap()
        .iter()
        .filter(|(under, _)| dir.starts_with(under))
        .map(|(_, h)| h.clone())
        .collect();
    for hook in hooks {
        hook();
    }
}

static BUILDS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Record that a build of the crate in `dir` ran.
pub(crate) fn note_build(dir: &Path) {
    BUILDS.lock().unwrap().push(dir.to_path_buf());
}

/// How many builds ran for crates under `base`, so a case can show that a load built nothing.
pub(crate) fn builds_under(base: &Path) -> usize {
    BUILDS.lock().unwrap().iter().filter(|d| d.starts_with(base)).count()
}

type FileHook = Box<dyn Fn(&Path)>;

thread_local! {
    static BETWEEN_READ_AND_STAT: std::cell::RefCell<Option<FileHook>> = const { std::cell::RefCell::new(None) };
}

/// Clears the hook it set when dropped.
pub(crate) struct FileHookGuard;

impl Drop for FileHookGuard {
    fn drop(&mut self) {
        BETWEEN_READ_AND_STAT.with(|h| *h.borrow_mut() = None);
    }
}

/// For this thread: run `hook` with the path of each source `depinfo::record` has read and not
/// yet asked the change time of, which is where an edit lands in the window the order of the
/// two calls closes. Per thread, so a test that sets one cannot disturb another beside it.
pub(crate) fn between_read_and_stat(hook: impl Fn(&Path) + 'static) -> FileHookGuard {
    BETWEEN_READ_AND_STAT.with(|h| *h.borrow_mut() = Some(Box::new(hook)));
    FileHookGuard
}

pub(crate) fn run_between_read_and_stat(file: &Path) {
    BETWEEN_READ_AND_STAT.with(|h| {
        if let Some(hook) = h.borrow().as_ref() {
            hook(file);
        }
    });
}

pub(crate) fn pause_between_write_and_build(dir: &Path) {
    let pause = KNOBS
        .lock()
        .unwrap()
        .iter()
        .filter(|k| dir.starts_with(&k.under))
        .map(|k| k.pause)
        .max();
    if let Some(pause) = pause {
        std::thread::sleep(pause);
    }
}
