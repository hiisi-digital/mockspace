# BUG: bootstrap doesn't auto-activate on first build

## What's wrong

Intended design: `cargo mock activate` is the manual fallback for
pre-first-build, before there's anything to run `cargo check` on. Once
`build.rs` has fired once, the bootstrap should have already wired up
git's `core.hooksPath` so pre-commit / pre-push hooks are active.

Current behaviour: `bootstrap::bootstrap_from_buildscript` does NOT
auto-activate. It calls `check_activation()` (src/bootstrap.rs:147),
which only emits the message:
```
mockspace hooks not active (run `cargo mock activate` to enable)
```
…and moves on. The hooks are generated under `mock/target/hooks/` but
`git config core.hooksPath` stays unset, so `git commit` / `git push`
never invoke them.

Net effect: anyone relying on cargo-driven bootstrap gets the `.cargo/
config.toml` alias and the hook scripts on disk, but commits bypass
validation unless they know to run `cargo mock activate` manually.
Defeats the whole "auto-initialise on first cargo check" ergonomics.

## Fix

In `bootstrap_from_buildscript` (or `bootstrap_from_buildscript_ext`,
whichever the build-script path flows through), after `check_activation`
discovers hooks are inactive:

1. Call `activate(repo_root, mock_dir)` directly.
2. On success, push a `"activated mockspace hooks (core.hooksPath set)"`
   action so the build-script output is transparent about what happened.
3. On failure (e.g. not a git repo, permission denied), fall back to
   the current "run `cargo mock activate`" message — don't fail the build.

Edge cases:
- Not a git repo → skip silently (same as current `is_active` returning
  false with no `.git` present — maybe gate on `.git` existing before
  attempting activation).
- CI environments where `core.hooksPath` mutations are unwanted →
  respect an env var, something like `MOCKSPACE_NO_AUTO_ACTIVATE=1`, so
  CI can opt out without hand-editing cargo config.
- `core.hooksPath` already set to a non-mockspace value (user has their
  own hooks) → don't clobber; emit the "not active, run activate
  manually" message like today. Only auto-activate when it's unset or
  already points at the mockspace hooks dir.

## Test plan

- Fresh clone, no prior `cargo check`: run `cargo check` inside `mock/`.
  Verify `git config --local --get core.hooksPath` returns the path to
  `mock/target/hooks/` after bootstrap fires.
- `cargo check` a second time: verify no duplicate activation attempts,
  no error.
- Manual `cargo mock deactivate` then `cargo check`: verify it re-
  activates (or at least doesn't fight the user — we probably want the
  user's deactivate to stick until they run `cargo mock activate`
  again; revisit).
- `MOCKSPACE_NO_AUTO_ACTIVATE=1 cargo check`: verify no activation.
- `core.hooksPath=/some/other/path` + `cargo check`: verify mockspace
  leaves it alone and prints the manual-activate hint.

## Severity

Medium. Not a correctness bug — everything still compiles, lints still
run via `cargo mock`'s own pipeline. But it silently demotes the
belt-and-suspenders story: pre-commit / pre-push become advisory
instead of enforcing, which is exactly the opposite of what a
"belt-and-suspenders" framework promises.

## Relationship to other bugs

`BUG_LOCK_SEMANTICS.md` is a workflow-semantics bug; this one is a
setup/ergonomics bug. They're independent. Fix whichever is faster.
