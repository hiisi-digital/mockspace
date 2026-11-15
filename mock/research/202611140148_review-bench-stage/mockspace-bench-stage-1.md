Source: kaski channel (coord/kaski of clause-dev), reviews/mockspace-bench-stage-1.md, first posted 2026-11-14 01:48 UTC, read 2026-11-15.

# Review: hiisi-digital/mockspace `feat/bench-stage` (80f95b5 over f42844c)

Canon gate passed. I checked it against the workspace rules quoted in the brief (`gate-per-target-and-per-feature`, `mockspace-builtin-rules-reach-every-repo`, writing and commit rules). The v1 tree declares no `canon_paths` and has no design for `bench-harness`. This is the first read, so a second read is still owed.

Test gate: I ran the whole workspace suite on the branch (`CARGO_INCREMENTAL=0 cargo test --workspace --no-fail-fast`) and the five failing tests against a clone at f42844c. All five reds fail on the base too: `disasm::tests::{a_variant_is_named_by_its_own_name_not_by_its_build_directory, identical_work_in_separate_crates_is_flagged_duplicate}`, `the_readme_names_no_pin_key_the_descriptor_does_not_read`, `no_tracked_file_names_a_project_a_reader_cannot_see`, and `bench_generated_e2e::a_config_only_nested_tree_runs_end_to_end_through_the_generated_driver`. The last is flaky on both trees: it failed once in three runs on the base and once in three on the branch. The harness passes 244 tests without the feature and 245 with it, against 236 at the base. The new tests are not tautological, and the digest oracle is independent of the code (the FIPS literal). The gaps are listed below.

## Verdict

MERGE AFTER FIXES

## Findings

1. `bench-harness/src/stage.rs:155-171`, `:258`, `:261`, `:270` [correctness]: `of_executable` never canonicalises. Suppose a driver is started through the member's `target` link, and `current_exe` reports the link path rather than its target (Rust documents that some platforms do this; macOS is in the list `build.rs` names). Then `Stage.dir` becomes the link itself. `link_into` sees that `read_link` differs, removes the link, and makes `member/target -> member/target`. It returns `Ok(())`, and every later read fails with ELOOP. I reproduced this with `of_executable(member/target/release/driver)` (see the appendix, `second_run_through_the_link_makes_a_self_link`). Fix: canonicalise `dir` in `of_executable` (or both sides of the comparison at `:258`), and add the reproduction as a test.
2. `bench-harness/src/stage.rs:232` against `:201` [correctness]: `is_ascii_hexdigit` accepts an uppercase digest, but the comparison is against lowercase `hex()`. An intact file is then reported as `Changed`. PowerShell's `Get-FileHash` writes uppercase, and Windows is a target the PR names, which is exactly where a consumer has no `sha256sum`. Compare with `eq_ignore_ascii_case`, or refuse uppercase as `Malformed`. Add a test either way.
3. `bench-harness/src/stage.rs:192-215`, doc at `:23-24` [correctness]: `verify` checks only what the record names. A record that omits the running driver verifies while the driver is tampered with (reproduced). Fix: `verify` (or `of_executable`) requires the executable's own `release/<name>` to be in the record. Add a test.
4. `bench-harness/src/stage.rs:200` [correctness]: `self.dir.join(file)` takes an absolute path or `..` from the record. `../outside-arm` verified `Ok` (reproduced), so a record can "verify" files in the shared target directory that the stage exists to avoid. The module doc at `:18-19` says the paths are relative to `<dir>`. Enforce that: refuse absolute paths and `..` components as `Malformed`.
5. `bench-harness/src/stage.rs:12-13` against `:255-271` [drift]: the module says a stage is "a directory of its own that nothing else builds into". But `link_into` makes `member/target` point into it and offers no inverse. The next plain `cargo build` in a member whose target dir is `./target` writes straight into the stage. Fix: either a guard that removes the link on drop, or an `unlink` paired with `link_into` and a sentence in the doc stating the consumer's obligation.
6. `bench-harness/src/stage.rs:141` against `lib.rs:155` and `workload.rs:297` [naming]: `mockspace_bench_harness::Stage` is already the workload stage, re-exported at the root. The crate now has two public `Stage` types meaning unrelated things. Rename this one (for example `stage::StagedBuild`, or call the module `staging`), and update `SKILL.md:13` to match.
7. `bench-harness/src/stage_tests.rs:140-141` [tests]: the "kept when it already points here" half cannot fail. If the link were removed and remade, `read_link == dir` would hold just the same. Assert that the link's identity is unchanged (for example `symlink_metadata` inode via `std::os::unix::fs::MetadataExt::ino`).
8. `bench-harness/src/stage.rs:263-267`, variant doc `:68-69` [correctness]: a regular file at `link` is refused as `LinkOverDirectory` with "is a directory, not a link" (reproduced). Either name the variant for "not a link", or match on `is_dir`. Add the case to the link test.
9. `bench-harness/Cargo.toml:9` [public-surface]: `readme = "README.md"` names a file that does not exist in `bench-harness/`. This predates the branch, but the manifest is touched and the checklist blocks on it. Its siblings (`bench-core`, `bench-macro`, `bench-matrix`) each have one. Add the README or drop the key.
10. `bench-harness/src/stage.rs:290-301`, `stage_tests.rs:133` [tests]: nothing in the default `cargo test` compiles `link_into`, and nobody has ever compiled the Windows arm. Only `x86_64-unknown-linux-gnu` is installed here, so I cannot either. At least state which command builds the feature lane. A `cargo check --target x86_64-pc-windows-msvc --features stage-link` run belongs in the PR's record before it lands.

## Verified

- Gating: the feature and the explicit `target_os` list in `build.rs:8-15` are both required. `rustc-check-cfg` declares the values, nothing in `stage.rs` names a target, and a driver calling `link_into` without both fails to build, as the commit says.
- Builtin text `src/render_agent/skills/benchmarking/SKILL.md:13` matches the source (layout, `of_current_exe`, `verify`, the feature, the `build.rs` list, consumer writes the stage).
- Commit subject is 62 characters, `type: lowercase imperative`, no parentheses, no trailers, no narration. The diff has no em-dashes, bold, or `allow`/`expect` attributes.
- The `sha2 = "0.10"` dependency is needed, and the lockfile change is the one line.
- The positive and negative controls in `stage_tests.rs` (replaced, gone, empty, malformed, unstaged, outside `release/`) each fail when the property breaks.

## Notes

The rule the brief quotes names `src/agent_builtin/`, and that directory does not exist. The builtins live under `src/render_agent/skills/`, so the rule text is stale and should be ticketed. `perf.rs` gates inline with `cfg(all(feature, target_os, target_arch))`, while this PR introduces a `build.rs` list for the same feature-and-target job in the same crate. That is two idioms for one gate and should become one. `bench_generated_e2e` is flaky on the trunk and wants a ticket. My reproductions were an ad-hoc spike in a scratch crate outside the repository, since deleted. The source is below so it can be committed as the missing tests.

## Appendix: reproduction (ad-hoc spike, not committed)

Run from a crate depending on `mockspace-bench-harness` with `features = ["stage-link"]`. All five failed on 80f95b5.

```rust
use mockspace_bench_harness::stage::{Stage, StageError, CHECKSUMS};
use std::path::PathBuf;

const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn stage_at(name: &str, record: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("probe-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("stage/release")).unwrap();
    std::fs::write(root.join("stage/release/driver"), b"abc").unwrap();
    std::fs::write(root.join("stage").join(CHECKSUMS), record).unwrap();
    root
}

/// A driver started through the member's `target` link, as current_exe reports on
/// platforms that do not resolve the link.
#[test]
fn second_run_through_the_link_makes_a_self_link() {
    let root = stage_at("selflink", &format!("{ABC}  release/driver\n"));
    let s = root.join("stage");
    std::fs::create_dir_all(root.join("member")).unwrap();
    let link = root.join("member/target");
    Stage::of_executable(&s.join("release/driver")).unwrap().link_into(&link).unwrap();
    let via_link = Stage::of_executable(&link.join("release/driver")).unwrap();
    println!("stage via link: {}", via_link.dir().display());
    let r = via_link.link_into(&link);
    println!("link_into -> {r:?}; read_link = {:?}", std::fs::read_link(&link));
    println!("verify after -> {:?}", via_link.verify().map_err(|e| e.to_string()));
    assert_eq!(std::fs::read_link(&link).unwrap(), s, "link no longer points at the stage");
}

#[test]
fn uppercase_digest_is_reported_changed() {
    let root = stage_at("upper", &format!("{}  release/driver\n", ABC.to_uppercase()));
    let st = Stage::of_executable(&root.join("stage/release/driver")).unwrap();
    let r = st.verify();
    println!("uppercase -> {r:?}");
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn regular_file_at_link_is_called_a_directory() {
    let root = stage_at("file", &format!("{ABC}  release/driver\n"));
    let st = Stage::of_executable(&root.join("stage/release/driver")).unwrap();
    let f = root.join("plainfile");
    std::fs::write(&f, b"x").unwrap();
    let e = st.link_into(&f).unwrap_err();
    println!("{e}");
    assert!(!matches!(e, StageError::LinkOverDirectory { .. }));
}

#[test]
fn record_reaches_outside_the_stage() {
    let root = stage_at("outside", "");
    std::fs::write(root.join("outside-arm"), b"abc").unwrap();
    std::fs::write(root.join("stage").join(CHECKSUMS), format!("{ABC}  ../outside-arm\n")).unwrap();
    let st = Stage::of_executable(&root.join("stage/release/driver")).unwrap();
    let r = st.verify();
    println!("outside -> {r:?}");
    assert!(r.is_err(), "a record naming a file outside the stage verified");
}

#[test]
fn driver_absent_from_record_verifies() {
    let root = stage_at("nodriver", "");
    std::fs::write(root.join("stage/release/arm.so"), b"abc").unwrap();
    std::fs::write(root.join("stage/release/driver"), b"tampered").unwrap();
    std::fs::write(root.join("stage").join(CHECKSUMS), format!("{ABC}  release/arm.so\n")).unwrap();
    let st = Stage::of_executable(&root.join("stage/release/driver")).unwrap();
    let r = st.verify();
    println!("nodriver -> {r:?}");
    assert!(r.is_err(), "the running driver is not covered by verify");
}
```
