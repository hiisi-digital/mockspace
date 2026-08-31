//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A dlopen'd sibling "runtime" cdylib that a boundary bench crosses into.
//!
//! A boundary bench measures the cost of the host-to-runtime C ABI call itself:
//! feeding a batch of W records across the boundary and reading the result back.
//! The load-bearing constraint is that the crossing must be a genuine cross-object
//! call the optimizer cannot inline or devirtualize. If the "runtime" lived in the
//! same crate as the bench cell, the variant's fat-LTO link would inline the
//! per-batch call and "W separate crossings" and "one crossing over W records"
//! would compile to indistinguishable code, so the axis under test (does batching
//! amortize the crossing) would vanish into the optimizer. Loading the runtime as a
//! separate `cdylib` and calling a resolved function pointer keeps every crossing a
//! real `blr` the optimizer never sees through.
//!
//! This is not a new scaffold regime. A boundary bench is an ordinary [`warm`] or
//! [`cold_cycle`] cell whose `St` holds a [`Runtime`] plus the resolved entry-point
//! pointers, and whose body issues the cross-object call. The calibrated loop those
//! regimes already run is what repeats the crossing, so the existing measurement
//! discipline (the S/I split, the reps-invariant digest, the anti-hoist fold)
//! applies unchanged.
//!
//! [`warm`]: crate::scaffold::warm
//! [`cold_cycle`]: crate::scaffold::cold_cycle

use libloading::{Library, Symbol};

/// A loaded runtime `cdylib`, kept alive for as long as any pointer resolved from
/// it is in use.
///
/// A resolved function pointer is only valid while the `Library` that owns it stays
/// loaded, so a boundary bench stores the `Runtime` in its cell's `St` alongside the
/// pointers it resolved, and the whole `St` outlives the calibrated loop. Dropping
/// the `Runtime` unloads the library and invalidates every pointer; do not resolve a
/// pointer, drop the `Runtime`, then call the pointer.
pub struct Runtime {
    // Held to keep the library loaded for the `Runtime`'s whole lifetime. `resolve`
    // reads it to look a symbol up; every later use of a resolved pointer relies on
    // it staying loaded, so it is never dropped before those pointers.
    lib: Library,
}

impl Runtime {
    /// dlopen the runtime `cdylib` at `path`.
    ///
    /// `path` is typically the built `.dylib` path a run passes through an
    /// environment variable, so the bench does not bake an absolute path at compile
    /// time. Open the runtime in the bench's `setup` (timed once as the S term),
    /// never in the timed cell.
    ///
    /// # Safety
    /// dlopen runs the library's initializers; the caller vouches that `path` names
    /// a trusted runtime `cdylib` built for this target.
    pub unsafe fn open(path: &str) -> Result<Self, libloading::Error> {
        let lib = unsafe { Library::new(path)? };
        Ok(Self {
            lib,
        })
    }

    /// Resolve `name` (a nul-terminated symbol) to a value of type `T`, typically an
    /// `extern "C"` function-pointer type.
    ///
    /// The returned value is valid for as long as this `Runtime` is alive. Resolve
    /// in `setup` and store the result in `St`; resolving inside the timed cell would
    /// measure symbol-table lookup, not steady-state per-call cost.
    ///
    /// # Safety
    /// The caller vouches that the symbol `name` in the loaded library actually has
    /// type `T` (the correct `extern "C"` signature). A wrong `T` is undefined
    /// behavior when the pointer is later called. The returned value borrows nothing,
    /// so it must not be used after this `Runtime` is dropped: dropping the `Runtime`
    /// unloads the library and calling a stale pointer is undefined behavior with no
    /// `unsafe` at the drop site. Keep the `Runtime` alive alongside every pointer
    /// resolved from it (store both in the same state).
    pub unsafe fn resolve<T: Copy>(&self, name: &[u8]) -> Result<T, libloading::Error> {
        let sym: Symbol<T> = unsafe { self.lib.get(name)? };
        // Deref copies the function pointer out of the borrow, so the result does not
        // borrow the library; `self.lib` staying alive is what keeps it valid.
        Ok(*sym)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_reports_a_missing_library_rather_than_panicking() {
        // dlopen of a path that does not exist is an `Err`, not a crash: a boundary
        // bench whose runtime artifact was not built must fail its setup cleanly
        // (the caller can skip), never abort the worker.
        let r = unsafe { Runtime::open("/nonexistent/does-not-exist.dylib") };
        assert!(
            r.is_err(),
            "opening a missing runtime dylib must be an error"
        );
    }

    #[test]
    fn resolve_of_a_real_symbol_from_this_process_returns_a_callable_pointer() {
        // Resolve a libc symbol from the already-loaded process image as a stand-in
        // for a runtime entry point: proves the resolve-to-fn-pointer path yields a
        // pointer that actually calls. (RTLD_DEFAULT-style: opening the current
        // executable resolves symbols visible to the process.)
        let this = unsafe { Library::new(current_exe_path()) };
        let Ok(lib) = this else {
            // Some sandboxes forbid dlopening the executable; the missing-library
            // test already covers the error path, so skip rather than fail here.
            return;
        };
        let rt = Runtime {
            lib,
        };
        // where the exe opened, resolving a stable libc symbol MUST succeed and the
        // pointer MUST be callable: a silent skip on the resolve/call path would make
        // this test vacuous (it could pass without ever exercising its contract). The
        // only documented skip is the open failure above (sandboxes that forbid it).
        let abs: unsafe extern "C" fn(i32) -> i32 =
            unsafe { rt.resolve(b"abs\0") }.expect("libc `abs` resolves from the process image");
        assert_eq!(unsafe { abs(-7) }, 7, "resolved pointer must be callable");
    }

    fn current_exe_path() -> std::path::PathBuf {
        std::env::current_exe().expect("test binary has a path")
    }
}
