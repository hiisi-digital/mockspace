# Sketch findings: a lint that judges a crate edge by the source directories at its two ends

**Date:** 2026-11-16
**Outcome:** WORKS for the arm that reads the manifests itself. The arm that reads the engine's extracted dependency list misses an edge written under a `[dependencies.<name>]` header, so that arm is INCONCLUSIVE as a vehicle until the extraction is repaired.
**Unblocks:** a repository that splits its crates over several source directories can state which directory may take which, in its `mockspace.toml`, and have the statement checked on every commit.

The sketch was written unattended by a dispatched seat on the night of 15 to 16 November 2026. The dispatcher opened `out/10_lint_fixture-red.out`, `out/11_lint_fixture-green.out`, `out/13_lint_fixture-header-form.out` and `out/14_lint_fixture-empty.out` before writing this file, and did not rerun the driver. Nothing here is authority.

## Hypothesis

A mockspace that declares several source directories can carry a lint that reads every crate's dependencies, places both ends of each edge in their directories, and refuses an edge between two directories no declared rule allows. The rules are parameters of the lint in `mockspace.toml`, so a repository states its own directions and the lint holds none of them.

It is settled by a driver that loads fixtures through the real configuration reader and the real crate discovery, runs the lint over them, and shows one fixture refused, one passed, one passed once a rule is added, and one where nothing was read.

## The vehicle

`src/lib.rs` holds two arms of one lint, both written against the lint contract the tool ships. Arm A, `an-edge-goes-where-its-directories-allow`, is a lint over the repository that reads each manifest under the declared source directories itself and reports at the manifest line of the edge. Arm B, the same name with `-by-engine-deps`, judges the dependency list the engine's own discovery extracted and reports at the crate.

The fixtures declare three source directories, `base`, `sim` and `render`, and two rules: `sim` takes `base`, `render` takes `base`. An edge inside one directory is allowed without a rule. `src/main.rs` is the driver; it prints the configuration as read, the crates found, each edge with its verdict, and each arm's findings.

No shipped source is edited. The fixtures are whole small workspaces under the sketch directory.

## What was tried, in order

1. `fixture-red`: a crate in `render` takes a crate in `sim`. Both arms report one finding and the driver exits 1. Arm A names `render/fx-draw/Cargo.toml:9`; arm B names the crate and says the section was not kept (`out/10_lint_fixture-red.out`).

2. `fixture-green`: the same tree without that edge. Three edges are listed, one inside a directory and two allowed by a named rule; both arms report nothing and the driver exits 0 (`out/11_lint_fixture-green.out`).

3. `fixture-rule-added`: the red tree with a third rule allowing `render` onto `sim` (`out/12_lint_fixture-rule-added.out`, with the difference against the red run in `out/21_diff_red_rule-added.out`). Not opened by the dispatcher; the seat reports it green.

4. `fixture-header-form`: the refused edge written as a `[dependencies.fx-ground]` table. Arm A reports it at `Cargo.toml:11`. Arm B reports nothing: the engine's extracted list for the crate holds `fx-ids` alone (`out/13_lint_fixture-header-form.out`). This is the case that separates the arms.

5. `fixture-empty`: the directories are declared and hold no crate. Both arms report that no crate was read and that a pass would be a pass over nothing, and the driver exits 1 (`out/14_lint_fixture-empty.out`).

6. The cross-directory dependency builds in one cargo workspace (`out/30` to `out/35`: builds, `cargo tree` and the workspace members of the fixtures). Not opened by the dispatcher.

Two output files were edited by hand after their runs, and are not raw: in `out/30_build_fixture-red.out` a toolchain path was folded to a placeholder, and in `out/21_diff_red_rule-added.out` one word of a comment line was changed.

## The result

A lint over the repository, reading the manifests under the declared source directories and taking its rules as parameters, refuses an edge no rule allows, passes one a rule allows, reports the manifest line, and refuses to pass over nothing.

The engine's dependency extraction does not see an edge written under its own table header, and keeps no record of which section an edge came from. A lint built on that list passes a tree it should refuse.

Holds for: mockspace `dev` at `0120ed8`, the version one tree; toolchain as in `out/00_toolchain.out`; x86_64 linux; three source directories and two or three rules; fixtures of four crates with path dependencies in one cargo workspace; edges in `[dependencies]`, inline and as a header table; the lint run through the sketch's driver, not through `cargo mock`.

## What is NOT established

- The lint run by `cargo mock` in a consumer. The seat reports that the entry which runs a passed lint pack runs it with the constructors' defaults, so the parameters would not reach it by that route as it stands. Not opened by the dispatcher.
- Dev, build and target-specific dependency sections, renamed dependencies, optional ones, and features.
- Edges reached through other crates. Against the lint kaski carries over its `sides.toml`, the direct arms map onto this one; its arm over the closure of edges and its arm over one named external crate are not covered.
- A git or registry dependency on a crate that lives in another declared directory of the same repository.
- The version two tree, which has its own lint contract.
- The lint's own tests in the tool's fixture harness: the harness cannot place crates in different source directories, so the sketch uses whole fixture workspaces.
- Cost on a large workspace.
