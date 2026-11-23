//! Support for the data-ref end-to-end scenarios: a bare remote, clones of it that run the real
//! `mockspace` binary on real git, a generated consumer fixture, and the checks that no byte of
//! tracked data is lost or silently changed.
//!
//! Nothing here is mocked. A scenario file includes this with `mod refs_e2e;`.

#![allow(dead_code, unused_imports)]

#[path = "../common/ambient.rs"]
mod ambient;

pub mod check;
pub mod consumer;
pub mod daily;
pub mod fixture;
pub mod setups;
pub mod spy;
pub mod whole;
pub mod world;

pub use check::{strays, Held, Manifest, content_only, held_on_remote, instance_matches, no_loss};
pub use world::{Clone, Out, World};

/// The state line of `status` for one namespace, which ends in `clean` or `changed`. It is the last
/// line of the namespace, after any that name a file.
pub fn status_of(c: &Clone, ns: &str) -> String {
    let out = c.run(&["data-refs", "status", ns]);
    assert!(out.ok, "status {ns}: {}", out.all());
    out.stdout.lines().filter(|l| l.starts_with(&format!("{ns}:"))).last().unwrap_or_default().to_string()
}

/// The commit a ref is at on the remote.
pub fn tip(w: &World, ns: &str) -> String {
    w.remote_tip(&format!("refs/heads/mock/{ns}")).expect("the ref is on the remote")
}
