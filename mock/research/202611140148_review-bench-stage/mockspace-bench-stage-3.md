Source: kaski channel (coord/kaski of clause-dev), reviews/mockspace-bench-stage-3.md, first posted 2026-11-14 02:17 UTC, read 2026-11-19.

# Review 3: hiisi-digital/mockspace `feat/bench-stage` (615659f over 710c140)

Canon gate passed, checked against the same workspace rules as rounds 1 and 2. A second read is still owed.

Test gate: `cargo test -p mockspace-bench-harness --no-fail-fast` gives 249 passed and 2 failed with `stage-link`, and 247 passed and 2 failed without it. The two failures are the `disasm` pair that also fails on `dev` at f42844c. I ran `cargo check -Zbuild-std=std,panic_abort --lib --tests --features stage-link --target x86_64-pc-windows-msvc`: exit 0, no warnings. That means the `cfg(windows)` test compiles. Nothing here can run it.

## Verdict

MERGE AFTER FIXES. Finding 1 is one line. Under the three-round cap it becomes a ticket if it is not done before landing.

## Round 2 findings

1. Guard removing a link it did not make: fixed. `StageLink.made` (`staging.rs:301-303`) is set only after `link_os::make` (`:360`), and `Drop` requires it (`:311`). Both `mem::forget` calls are now `drop` followed by an assertion that the link survives (`staging_tests.rs:238-243`, `:300-305`). Against the 710c140 `Drop`, `read_link(...).unwrap()` panics, so the test fails without the fix.
2. Driver matched by string: fixed. `driver` is a `PathBuf` compared by `components()` (`staging.rs:238`). `the_driver_is_found_by_its_path_not_by_its_spelling` uses `release//driver` and fails on the old string compare. The `cfg(windows)` backslash test compiles.
3. Doc on record paths: fixed at `staging.rs:19-21`.

## Findings

1. `bench-harness/src/staging_tests.rs:111` [tests]: `assert_eq!(driver, "release/driver")`. The error now carries `self.driver.display()` (`staging.rs:250`), which is `release\driver` on Windows. So this test is red on a target the PR names, and since no lane runs Windows tests, nobody will see it. Compare against `Path::new(RELEASE).join("driver").display().to_string()`, or make the variant carry a `PathBuf` and compare paths.

## Notes

The ownership rule leaves two cases, and both should become one ticket. The `made` flag is the shape I asked for in round 2, so neither is new breakage. First, when a run that made the link finishes while a run that found it is still going, the link is removed from under the second run. The doc at `staging.rs:330-332` states this. Second, a link left by a run that died is now kept by the next run and removed by nobody, so a plain `cargo build` in that member writes into the staged build until someone deletes it. Before this round the next run cleaned it up. Closing both needs the link to carry its owner (a marker naming the holding process, or a per-run link name). That is a design change, not a fix for this branch.

Commit `fix: keep a link another run made and name the driver by its path` is 66 characters, a subject only, with no trailers. The diff has no em-dashes, bold, or suppressions.
