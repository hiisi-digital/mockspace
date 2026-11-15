Source: kaski channel (coord/kaski of clause-dev), briefs/review-2-mockspace-pack-identity.md, first posted 2026-10-31 07:53 UTC, read 2026-11-15.

# Review 2 of mockspace `fix/pack-identity-by-content` at 1b9a467: merge after fixes

By `pr-reviewer-senior`, 31 October. Whole workspace suite: 2627 passed, 7 failed, none of them in a touched file. `an_executable_is_written_by_the_one_writer` is fixed on dev by 394cb7e, so take `dev` in; the merge is clean. `custom_lints::` with `--include-ignored`: 107 passed.

## Blockers

1. Stale replay, reproduced. `src/custom_lints.rs:231-241` and `identity.rs:214`. Review 1's finding 1 asked for the identity to be computed again after the build, and only the files were covered: dep-info and change times. The key-only inputs are read once, before the build: the cargo config files, `rustc -vV` and the resolved members. Probe: a lint returning `cfg!(review_probe_flag)`. The existing `after_identity` knob writes `./.cargo/config.toml` with `[target.x86_64-unknown-linux-gnu] rustflags = ["--cfg","review_probe_flag"]`. The probe loads, deletes the config and loads again. It got `("on","on")` where `("on","off")` is right, because a library built under the new config was kept under the old config's key and replayed. Fix: after the build, compute `config_digest` and the toolchain again (better, all of `collect_inputs`) and keep nothing if they moved. Commit the probe as a red test in `identity_e2e_tests.rs` first. It is a small `#[ignore]`-free test using `tree`, `lint_named_by`, `name_in` and `store::knobs::after_identity`, with the config written under `std::env::current_dir()` and removed before the second load.
2. Same class, found by reading. `depinfo.rs:243-251` checks a file's change time, then hashes the file. An edit between the two stores the new content's digest beside the old library. Read the bytes first, then check the change time.
3. Stale public rustdoc. `lint-rules/src/lib.rs:1832` says `LintPack::identity` "is the digest of the library's bytes". It is now `<input key>-<list digest>`, with the bytes only as a fallback. Correct the doc.
4. README overclaims. `README.md:143` says the paths in "the lint library itself" read by label. A tool's or path package's own `file!()` still names the tree (ticket 3f67a1d5a1d0). Narrow it to "the lints and the generated crate", as `USAGE_GUIDE.md:526` already does. Put `FIXME(<that ticket's id>): ...` at the remap site, `build.rs:142`. The ticket should also list cargo's `trim-paths` (nightly, needs vetting) as an option.
5. Missing tests, found by a mutation pass.
   - Removing `.env_clear()` (`build.rs:150`) survives, because the `--config` pin covers `CARGO_PROFILE_*`. Add an e2e case with `RUSTFLAGS="--cfg x"`.
   - Disabling `find()` (`built.rs:91`) leaves every e2e test green, so nothing end to end shows a second tree loading without a build, which is the point of the branch. `the_same_sources_in_two_trees_are_one_identity` (`identity_e2e_tests.rs:71`) passes even with nothing kept. Assert that tree B ran no build.
6. Tautology. `build_tests.rs:28` asserts the fixture contains its own substring, and line 29 repeats test 1. Delete the test.

## Non-blockers, to fix where cheap or to ticket

- `custom_lints.rs` is 762 lines and `tests.rs` is 525, both over 500. Split along seams.
- `custom_lints.rs:37` has a FIXME with no ticket, and it says "cargo build" where the build is now `cargo rustc`.
- Review 1's finding 7 is still open: untracked files are hashed.
- The kaski timings count as unmeasured until committed in the bench or research tree.
