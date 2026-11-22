# Review 2 of `work/disasm-cross-platform`

Head e6503ec against `origin/dev` at 2f11e87, mockspace v1, which declares no canon by design; the oracle is op's line of 16 November, "I need the disasm working across all platforms", and the tickets the brief names. The branch is aligned with that intent in shape, and the ten findings of review 1 are each addressed in code and tests. What remains is one public-surface blocker the round-1 fix missed by fixing the instance rather than the class, one Windows path that rests on debug information nothing provides, and one new test that cannot pass on Windows.

Verdict: merge after fixes.

## Gates

Run one crate at a time on x86_64 Linux (LLVM 18.1.3 `llvm-objdump`, GNU Binutils 2.42): `mockspace-bench-harness` 279 passed, 3 ignored, smoke 6 passed; the ignored `linked_libraries_of_every_target_are_read_and_compared_live` passes with `--ignored`; `mockspace-bench-layout` 13 passed; `tests/bench_disasm_e2e.rs` passed; the root `mockspace` lib 1836 passed, 4 failed, and the four are exactly the known reds (three `bench_builds` sweep tests and `entry::clean_tests::suggest_subcommand_declines_far_words`). Every test body in the touched surface was read: `disasm/tests.rs`, `tests_live.rs`, `tests_recorded.rs`, `tests_tool.rs`, `artifact.rs`, `library.rs`, `bench_disasm.rs`, `bench_variants_tests.rs`, `dylib_check.rs`, `clean_tests.rs`, `bench-layout/src/tests.rs`, the e2e test. No tautological test was found.

The public-surface checklist is not in this worktree, which carries no `.claude/`; it was read from the workspace's `pr-review` skill, `reference/public-surface.md`.

## Round 1, claim by claim

1. Dumpbin immediates: `fold_dumpbin_operands` (`normalize.rs:282-335`) folds a long hex token only as a branch or call operand or as a lone `[...]` address; the large-immediate negatives are in `tests.rs:240-258`. Mutation m13 replayed red. Fixed.
2. Recorded fixtures: linked libraries now, four per target, with a `variant_shifted` per target. The guard (`tests_recorded.rs:178-206`) holds each identical pair to differ in its instruction lines and the shifted entry to differ with the address column off. Diffed by hand: aarch64 pairs differ in branch, call and the `add` after `adrp`; PE pairs in every target through the image base; x86_64 ELF and Mach-O pairs only in the salted `do_work` annotation, which the fixtures README says plainly. The shifted entries carry the RIP and `adrp` moves. Fixed.
3. `tools()` adds `otool` on macOS and `dumpbin` on Windows (`tests_live.rs:118-123`). Fixed, subject to finding 2.
4. Nothing skips; no tool or no `rustc` fails (`tests_live.rs:124-127`, `43`). The cross-linking test is ignored with a ticket. Fixed.
5. `mock help bench` names `disasm`, and `mock_help_bench_names_every_bench_subcommand` holds it to `bench::SUBCOMMANDS`. Fixed in `help.rs`; the class is not, see finding 1.
6. A 32-bit PE image tries the bare export name, then the underscored one (`format.rs:97-116`), and the report scopes it to synthetic bytes. Fixed as asked.
7. `create_new`, the next `-<k>` on `AlreadyExists`, the header names the run it took (`artifact.rs:61-89`). Fixed.
8. `<arm>/<symbol>.txt`, with the dotted-name case in `bench-layout/src/tests.rs`. Fixed.
9. The arm build moved to `src/bench_arms.rs`; `bench.rs` went from 2302 to 2271 lines. Fixed.
10. Twenty-three mutations, each a patch and an output with the failing test names. Six replayed here (m04, m07, m10, m13, m14, m15) on an extract of the head: each failed exactly the tests its `.out` names. Fixed, with finding 5.

## Findings

1. `docs/USAGE_GUIDE.md:136` [public-surface, blocker]. The usage guide's command table still lists `mock bench` as `init`, `add`, `run`, `report`, `list`, `test`, `migrate`, with no `disasm`. It is hand-authored (`config.rs:1922` includes it), so nothing regenerates it, and the new test reads only `help.rs`. Failure: a reader of the guide never learns the command exists, which is the stale-surface case the checklist blocks on. This is round-1 finding 5 fixed at the one place named. Fix: add `disasm <bench> <arm> <symbol>` to the row, and grep every surface listing the bench subcommands (the test could hold the guide's row to `SUBCOMMANDS` as it does the help).

2. `bench-harness/src/disasm/tool.rs:109-149` with `pe_exports.rs` [correctness, medium, uncertain]. For `dumpbin` the entry is found only by a `bench_entry:` label in its output, and dumpbin labels functions from a PDB; it has no export-table fallback, while GNU `objdump` gets one. The committed `x86_64-pc-windows-msvc/variant_scalar.dll` carries no PDB. Failure: on a Windows bench machine with `dumpbin` on `PATH`, `tests_live.rs:341` makes `every_tool_here_finds_bench_entry_in_a_linked_library_of_every_format_it_reads` ask dumpbin for `bench_entry` in that DLL, `symbol_block` finds no label, and the test panics; on a Windows host where dumpbin is the detected tool, `mock bench disasm` fails for any arm whose PDB is not where the DLL's debug directory says, and the cheap entry comparison silently drops out. What to check: `dumpbin /DISASM:NOBYTES variant_scalar.dll` on Windows. Fix if confirmed: cut dumpbin's output by export RVA as `pe_exports.rs` does for GNU, from `dumpbin /EXPORTS`, as one export-range abstraction with a reader per flavour rather than a second copy.

3. `tests/bench_disasm_e2e.rs:51` [tests, medium]. The repository path goes into a TOML basic string, `path = "{repo}"`. On Windows `CARGO_MANIFEST_DIR` is `C:\...`, and `\U`, `\a` and the rest are invalid or wrong escapes, so the bench manifest does not parse and the e2e test fails on the platform the feature exists for. The shape is copied from older tests (`tests/mock_test_benches_when_moved.rs:81` and five others), so it is a class, but op's intent includes the tests passing on each platform and this test is new. Fix: write the path with forward slashes or escape it, and file the class.

4. `bench-harness/src/disasm/normalize.rs:221-236` [correctness, low]. `fold_rip_relative` folds the digits and keeps the sign: `leaq -0x123a(%rip),%rax` becomes `leaq -ADDR(%rip),%rax`, `leaq 0x13(%rip),%rax` becomes `leaq ADDR(%rip),%rax`. Checked by running both lines through `normalize_disasm` on a scratch extract of the head. Failure: where a layout change moves the data from before an instruction to after it, two crates doing identical work compare different and a duplicate goes unreported. This is the safe direction, so low, but the sign is layout exactly as the magnitude is. Fix: take a leading `-` or `+` into the fold, and commit those two lines as a test. Uncertain and lower: `[rip+1234h]` from dumpbin is not folded either, which matters only if dumpbin prints a RIP operand unresolved; it usually resolves it to an absolute `[...h]`, which is folded.

5. `mock/research/202611160949_disasm-cross-platform-mutations/*.out` [evidence, low]. The outputs were recorded on a tree one test smaller than the head (for example m04's line reads 40 passed, the replay here 41) while the README calls each patch a diff from the branch. The failing tests match in all six replays, so the claim stands; say which commit the outputs were recorded at.

6. `bench-harness/src/disasm/fixtures/README.md:44-56` [evidence, low]. The commands say `<linker args>` and leave the per-target arguments to be found in `tests_live.rs:240-257`, which in turn omits the `-install_name @rpath/<library>` the committed Mach-O libraries were linked with. Rebuilt here from the stated arguments: the aarch64 ELF library is byte-identical; the x86_64 Mach-O and PE libraries disassemble to identical text and differ only in a UUID and a timestamp. No machine path is in any committed library or text. Fix: spell each target's command in full.

7. `src/bench_disasm.rs:226-261` [correctness, low]. `is_name` on the bench, arm and symbol runs inside `write_symbol_artifact`, after the arm is built; a bad symbol name costs a full release build before the refusal. The member-path case is ticketed; the cheap fix is to check all three in `parse`.

8. `src/bench.rs:20-42` [drift, low]. Each match arm carries `if listed` against a list that contains its own literal, so the dispatch and `SUBCOMMANDS` are two lists kept in step by hand; a subcommand added to the match and not the list is silently refused as unknown. Dispatch from the list, or derive the list from the dispatch.

## Verified

- Detection order matches the builtins and the report (`tool.rs:270-302`); flags per flavour (`tool.rs:58-82`).
- Symbol spelling from the file's own bytes, PE through `e_lfanew`, all four Mach-O magics (`format.rs`).
- `arm_name` from the file's own suffix, used by the warnings, `entry_display` and `build_for`; `dylib_check` builds the host's name.
- `pe_exports.rs` maps names to RVAs by ordinal index, which is what binutils prints.
- Builtins in `src/render_agent/` (`templates.rs`, `skills/benchmarking/SKILL.md`) describe only what the source has.
- Artifact layout `results/<bench>/disasm/<machine class>/<run>/<arm>/<symbol>.txt` is placed in the layout and no run reader takes it.
- Fourteen commit subjects: `type: lowercase imperative`, under 72, no bodies, no trailers, no bylines. No em-dash in any added line.
- New files under 500 lines; `bench.rs`, `help.rs` and `templates.rs` were over before and are not grown.
- The report's "not proven" paragraph and its four tickets exist and match the code.

## Notes

Everything proven still ran on x86_64 Linux only, and the report says so. Finding 1 is the blocker. Finding 2 is the one to check on a real Windows machine before the report's claim that each bench machine proves its own platform can hold there: as written, a Windows machine with dumpbin would fail a test for a reason in the harness rather than in the platform. Single reviewer; no canon call is made here.
