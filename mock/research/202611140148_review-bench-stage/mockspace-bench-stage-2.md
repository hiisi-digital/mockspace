Source: kaski channel (coord/kaski of clause-dev), reviews/mockspace-bench-stage-2.md, first posted 2026-11-14 02:12 UTC, read 2026-11-15.

# Review 2: hiisi-digital/mockspace `feat/bench-stage` (710c140 over 80f95b5)

Canon gate passed, checked against the same workspace rules as round 1 (`gate-per-target-and-per-feature`, `mockspace-builtin-rules-reach-every-repo`, writing and commit rules). No `canon_paths` in v1. A second read is still owed.

Test gate: `cargo test -p mockspace-bench-harness --no-fail-fast` gives 248 passed and 2 failed with `stage-link`, and 246 passed and 2 failed without it. The two failures are `disasm::tests::a_variant_is_named_by_its_own_name_not_by_its_build_directory` and `identical_work_in_separate_crates_is_flagged_duplicate`, the same pair that failed on `dev` at f42844c in round 1. For each new test I checked that the assertion fails against the 80f95b5 code: the capital digest, `..`/`.`/absolute/`release/../`, the driver missing from the record, a file at the link path, the inode identity, and the driver reached through the link. None is tautological. Two tests dodge a defect with `mem::forget` (finding 1).

## Verdict

MERGE AFTER FIXES

## Round 1 findings

1. Self-link: fixed. `staging.rs:191` canonicalises. `staging_tests.rs:257-276` fails on the old code at `:264`.
2. Capital digest: fixed. `staging.rs:238`, test at `staging_tests.rs:50-61`.
3. Driver not recorded: fixed. `staging.rs:236`, `:246-250`, test at `:103-115`. See finding 2 for how the match is made.
4. Escaping the record: fixed. `staging.rs:276-280` accepts only `Component::Normal`. The test at `:143-172` covers `..`, absolute, `release/../` and `./`.
5. Link outliving the run: addressed with `StageLink` at `staging.rs:289-315`. This introduced finding 1.
6. Name collision: fixed. `staging::StagedBuild`, and the crate root no longer has two `Stage` types.
7. "Kept" assertion unable to fail: fixed with the inode check at `staging_tests.rs:208-214`.
8. A file called a directory: fixed. `NotALink`, file case at `staging_tests.rs:239-248`.
9. README: added, and it follows the sibling crates' shape. Its claims match `driver::DriverRegistry` and `drive`.
10. Windows arm: compiles. The command the commit names cannot run in this container, since only `x86_64-unknown-linux-gnu` and `wasm32-unknown-unknown` are installed (E0463). I checked it with `cargo check -Zbuild-std=std,panic_abort -p mockspace-bench-harness --features stage-link --target x86_64-pc-windows-msvc` instead: exit 0, no warnings.

## Findings

1. `bench-harness/src/staging.rs:338-339` with `:302-314`; tests at `staging_tests.rs:215` and `:272` [correctness, tests]: `link_into` hands out a removing guard for a link it did not make. When the link already points at this build, the second call returns a `StageLink` too. Dropping that guard removes the link while the first holder is still running. Reproduced: `first = link_into`, `second = link_into`, `drop(second)`, and the link is gone with `first` alive. Two runs of one staged build into one member are enough to hit it. Both tests that make a second guard call `std::mem::forget(again)` before the first is dropped. That is the setup stepping around the defect rather than catching it. Fix: the guard records whether this call made the link, and only a guard that made it removes it. The doc at `:326-330` then states that a kept link belongs to whoever made it. Replace the `forget` calls with an assertion that the link survives the second guard's drop.
2. `bench-harness/src/staging.rs:236` [correctness]: the driver is matched by string (`file == self.driver`, which is always `release/<name>` with a forward slash). A record naming the driver by any other spelling of the same path is refused as `DriverNotRecorded`. Reproduced on linux with `release//driver`. On Windows the same check refuses a record written with `release\driver.exe`, and the parser accepts that spelling because `components()` splits on both separators there. That is the PowerShell-written record that round 1 finding 2 was about. Compare `Path::new(file).components()` against the driver's components, and add the `release//driver` case as a test.
3. `bench-harness/src/staging.rs:19` [naming]: the module doc says the path has "no `..`", but `parse_line` also refuses `.` and absolute paths. Say "only plain components, no `.`, `..` or root".

## Verified

- The round 1 fixes listed above, each against its test.
- `SKILL.md:13` describes `staging::StagedBuild`, the driver requirement, the paths inside `<dir>`, and `StageLink` dropping the link, all as the source does.
- Commit `fix: refuse unsafe staged build records and self-pointing links` is 63 characters, with no trailers and no narration. The diff has no em-dashes, bold, or `allow`/`expect` attributes.
- The error paths in `link_into` drop the guard harmlessly. `NotALink`, a failed remove and a failed make each leave no link pointing at this build, so `Drop` removes nothing.

## Notes

Findings 1 and 2 were reproduced in an ad-hoc scratch crate, since deleted. Its source is below so the cases can be committed as tests. This is review round 2 of at most 3.

```rust
use mockspace_bench_harness::staging::{StagedBuild, CHECKSUMS};
use std::path::PathBuf;

const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn stage_at(name: &str, record: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("probe2-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("stage/release")).unwrap();
    std::fs::write(root.join("stage/release/driver"), b"abc").unwrap();
    std::fs::write(root.join("stage").join(CHECKSUMS), record).unwrap();
    std::fs::canonicalize(root).unwrap()
}

/// Two holders of one link: the second call keeps the link, and dropping its
/// guard removes the link the first holder still holds.
#[test]
fn a_second_guard_removes_the_first_holders_link() {
    let root = stage_at("twoguards", &format!("{ABC}  release/driver\n"));
    let b = StagedBuild::of_executable(&root.join("stage/release/driver")).unwrap();
    let link = root.join("target");
    let first = b.link_into(&link).unwrap();
    let second = b.link_into(&link).unwrap();
    drop(second);
    let alive = link.symlink_metadata().is_ok();
    drop(first);
    assert!(alive, "the link went while the first run still held it");
}

/// A record naming the driver by a path that resolves to it but is not
/// spelled `release/<name>`.
#[test]
fn a_record_naming_the_driver_by_another_spelling_is_refused() {
    let root = stage_at("spelling", &format!("{ABC}  release//driver\n"));
    let b = StagedBuild::of_executable(&root.join("stage/release/driver")).unwrap();
    let r = b.verify();
    println!("{r:?}");
    assert!(r.is_ok(), "{r:?}");
}
```
