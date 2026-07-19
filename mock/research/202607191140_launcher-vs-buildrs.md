# The thin-launcher alternative to build.rs bootstrapping

**Date:** 2026-07-19
**Status:** research note, not a decision. Raised by op 2026-07-19 ("make mockspace trigger without the
build.rs bootstrapping").

## The constraint op correctly identified

Cargo provides exactly two automatic hooks into a build: `build.rs` (per-crate, at build time) and
proc-macros (per-invocation). Neither is a clean "run once on project setup". So `build.rs` is the only
zero-touch auto-trigger, and the current bootstrap uses it: a consumer adds mockspace as a build-dependency
and calls `bootstrap_from_buildscript()`. This is why op found nothing cleaner that keeps the
add-a-dep-and-done ergonomics.

## The one architecture not in the explored set: a thin global launcher

The rustup / cargo-binstall / cargo-dist pattern. A tiny `cargo-mock` binary installed once on PATH that
does nothing itself but:

1. resolve the repo (walk up for `mockspace.toml` / `.git`),
2. read the repo`s pinned mockspace version (from a manifest or a lockfile the repo commits),
3. build or fetch that exact version into a shared cache on first use, and run it.

### What it dissolves

- **The build.rs bootstrap goes away.** The launcher is the trigger; no build-dependency, no compile-graph
  coupling.
- **The cwd problem (round topic `cargo-mock-cwd`) vanishes.** A launcher on PATH runs from any working
  directory; the `--manifest-path` relative-to-cwd fragility that the two-config redirect works around does
  not exist. The redirect becomes obsolete.
- **The cargo-cache poisoning surface disappears at the root.** Nothing runs a bootstrap during dependency
  compilation, because there is no build-dependency bootstrap. The cache-guard and sudo-repair rounds guard
  a surface that would no longer exist.
- **Per-repo version pinning is preserved.** The launcher reads the pin; it does not hardcode a version. This
  is the property the generated proxy crate exists to provide, kept intact.

### The cost, which makes it op`s call

It trades zero-install-per-repo for one-time-install-per-machine (`cargo install` or a curl script for the
launcher). That is exactly the rustup tradeoff. It is a philosophical choice about whether mockspace is
zero-touch-per-repo (current) or one-touch-per-machine (launcher), not a purely technical one.

### Why it is a separate round, not folded into the CLI-ergonomics work

The launcher is a distribution-model change touching bootstrap, the proxy, activation, and the install
documentation. The immediate cwd redirect and the unknown-subcommand fix relieve today`s pain and are
independently correct. If the launcher is adopted later, the redirect is one PR to remove; nothing is lost by
shipping the redirect first.

## Open questions for the launcher round, if it happens

- Where the per-repo version pin lives (a committed `mockspace.toml` field, a lockfile, a `rust-toolchain`
  sibling).
- How the launcher is distributed and updated (crates.io, a release binary, a curl script).
- Whether the git hooks call the launcher directly (removing the `cargo check` self-heal in favour of the
  launcher building the pinned version on demand).
- Migration: consumers currently on the build.rs bootstrap keep working until they adopt the launcher.
