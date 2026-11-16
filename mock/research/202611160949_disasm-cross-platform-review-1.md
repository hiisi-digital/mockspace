# Review 1 of `work/disasm-cross-platform`

Head 387919b against `origin/dev` at 2f11e87, mockspace v1. The governing intent is op's line of 16 November, "I need the disasm working across all platforms", and the tickets the brief names; v1 declares no canon, so that intent is the oracle. The branch is aligned with it in shape: one detected tool, flags per flavour, symbol spelling from the file's own format, naming from the file's own suffix, and the named-symbol artifact the ticket asked for. Where it falls short is proof, and in one place correctness on Windows.

Verdict: merge after fixes.

## Gates

Suites run one at a time on this host (x86_64 Linux, llvm-objdump 18, GNU objdump): `mockspace-bench-harness` 276 passed, 2 ignored; its 40 disasm and library tests all green; `mockspace-bench-layout` 13 passed; `tests/bench_disasm_e2e.rs` green; the root lib's touched modules (`bench_disasm`, `bench_variants`, `dylib_check`) green. The root lib has the three known `bench_builds` sweep reds, and a fourth red not in the brief's list, `entry::clean_tests::suggest_subcommand_declines_far_words` (suggests `rows-gate` for `frobnicate`); `src/entry/` is untouched by the branch, so it is red on dev and not this branch's, but it is not the known set and wants its own ticket.

Every test body in the touched surface was read. The test-gate failure is finding 2: the recorded-fixture suite is set up so the property it is named for cannot fail.

## Findings

1. `bench-harness/src/disasm/normalize.rs:283-285` [correctness, high]. `fold_dumpbin_operands` folds every operand token of eight or more hex digits to `ADDR`, and dumpbin writes immediates in exactly that shape. Failure: on a Windows host where dumpbin is the detected tool, `mov rax,0CBF29CE484222325h` and `mov rax,0100000001B3h`, or `imul eax,eax,9E3779B9h` against any other 32-bit multiplier, both become `mov rax,ADDR`; two variants differing only in a constant are reported as identical machine code. This is the dangerous direction the module's own doc (`normalize.rs:161-168`, `334-340`) says it refuses to take. `dumpbin_full_width_addresses_and_mangled_names_fold` (`tests.rs:221`) has one negative, `cmp rdx,40h`, a two-digit immediate, so it is setup that only feeds what works. Fix: fold a long hex token only as a branch or call target (`j*`, `call`, `loop*`) or inside `[...]` as an absolute address, keep every other immediate, and add the large-immediate negative.

2. `bench-harness/src/disasm/tests_recorded.rs:78-88`, fixtures under `disasm/fixtures/` [tests, high]. In all seven recorded pairs the instruction lines of `variant_scalar` and `variant_unrolled_x4` are byte-identical; the pair differs only in the header (`variant_scalar.o: file format ...`), the section title and the label lines (counted per pair with `diff | grep -v 'file format\|Disassembly of section\|>:$'`: zero lines in each). These are unlinked objects, so every call and branch is an unresolved relocation (`callq 0xc <bench_entry+0xc>` in `x86_64-unknown-linux-gnu/llvm-variant_scalar.txt:55`), and no layout shift exists for the normaliser to fold. `the_recorded_pair_differs_before_normalisation` is satisfied by the file name in the header line alone, so the guard written to stop the duplicate test passing "on two identical files" passes on two files identical in everything it was meant to cover. Consequence: the recorded suite stays green with the RIP fold, the bare-hex fold, the `;` rule, the `adrp` folds and the mangled-name handling all removed; for Mach-O, aarch64 and COFF it proves header and label dropping and the symbol spelling, nothing else. The fixtures README's sentence that "an object carries the same section names, symbol spellings and instruction syntax a library of that target does" is wrong for a linked PE image, which carries no COFF symbol table (its names come from the export table or a PDB), and wrong for addresses on every format. Fix: make the guard assert the pair differs after header, section and label lines are dropped (it will go red on these fixtures, which is the point); record linked libraries where a linker exists (host ELF both flavours now; Mach-O and PE from the bench machines under the existing ticket) and scope the report and `normalize.rs:15-24` to what the objects actually show.

3. `bench-harness/src/disasm/tests_live.rs:102-120` [tests, medium-high]. `tools()` adds the detected tool plus `objdump` and `llvm-objdump` by name, never `otool` or `dumpbin`. On a stock mac Apple's `objdump` is detected first (the report says so at line 142), and a Windows bench machine with LLVM installed detects `llvm-objdump`, so the otool and dumpbin paths run on no bench machine at all. The report's "proven only by the live tests on a host that has them" describes a host unlike the bench fleet. Fix: add `otool` under `cfg(target_os = "macos")` and `dumpbin` under `cfg(windows)` to `tools()` when they run, so every bench machine proves both flavours it has. That is brief step 3.

4. `bench-harness/src/disasm/tests_live.rs:116-119`, `142-145`, `269-271` [tests, medium]. No disassembler, no `rustc`, or a missing target's std each print "skipping" and return green. A bench machine with no tool reports the same green as one that proved everything, while the report (line 145) presents these as what "proves it there". Fix: on the platforms the benches run on, fail when `Disassembler::detected()` is `None`, or mark the case `#[ignore = "<ticket>: ..."]` so the gap is counted rather than greened.

5. `src/entry/help.rs:270-271` [public-surface, blocker per the checklist]. `mock help bench` lists `init`, `add`, `run`, `report`, `list`, `test`, `migrate`; `disasm` is missing. The subcommand help in `src/bench.rs:133-134` has it, the top-level help does not. Fix: add it.

6. `bench-harness/src/disasm/format.rs:91-101` [correctness, medium, uncertain]. The underscore is applied to every 32-bit COFF, image or object. For a linked i686 DLL the names llvm-objdump shows come from the export table, which link.exe writes undecorated for a `cdecl` export, so `_bench_entry` likely finds nothing and the entry comparison silently drops to the whole-text one, and `mock bench disasm` fails. What to check: link `i686-pc-windows-msvc` cdylib on a Windows machine and read it. Until then, for `ObjectFormat::Coff` from the `MZ` branch, try the bare spelling as well, and say in the report the PE image path (`format.rs:45-55`) is held by the synthetic `pe_image` only.

7. `bench-harness/src/disasm/artifact.rs:129`, `145-149`, `167` [correctness, low]. "Written once and never replaced" rests on `exists()` then a truncating `fs::write`; it holds only because `bench_disasm.rs:57` takes the tree lock. Use `OpenOptions::create_new` and move to the next suffix on `AlreadyExists`. The header's `run:` line prints `place.run`, so an artifact under `<run>-2/` names a different run than its directory. The `.unwrap_or_else` on an infinite iterator is dead.

8. `bench-layout/src/lib.rs:47-53` [naming, low]. `<arm>.<symbol>.txt` with `is_name` admitting `.` lets arm `a.b` with symbol `c` and arm `a` with symbol `b.c` share a file. Refuse `.` in the arm for this path or use a separator `is_name` excludes.

9. `src/bench.rs:806-855` [rules, low]. The file was 2302 lines and is now 2314; the extracted `run_rev_of`, `aligned`, `build_arm` were placed back into it rather than into a module of their own, which was the natural seam the refactor exposed.

10. Report, `mock/research/202611160949_disasm-cross-platform.md:133-139` [evidence]. "Each behaviour's test was shown failing with that behaviour taken back out" names no test per mutation and has no committed trail, so it is an unverifiable claim. Finding 2 shows the recorded suite could not have been the one that went red for any instruction-level mutation. Name the failing test per mutation, or drop the claim. Lines 117-129 ("the duplicate decision is right" for every cross-built target) need the scoping of finding 2.

## Verified

- `--disassemble=` for GNU and `--disassemble-symbols=` for LLVM, `--no-show-raw-insn` for both (`tool.rs:57-81`).
- Detection order matches the builtins and the report (`tool.rs:252-284`); `--version` classification reads "LLVM" before "GNU".
- Exact label match: `bench_entry_helper` no longer starts the block (`tool.rs:142-149`).
- An empty normalised result is `None`, never a shared duplicate (`disasm.rs:65-67`).
- `arm_name` reads the file's own suffix with either separator, used by the warnings, `entry_display` and `build_for`; `dylib_check` builds the host's name, which is right since it loads on the host.
- `mock bench disasm` end to end on the host, the second artifact beside the first, nothing written for a missing function; the layout places the artifact and `runs()` ignores it.
- The `run` refactor keeps every error path and message.
- Builtins in `src/render_agent/` describe only what the source has.
- Six commit subjects: `type: lowercase imperative`, under 72, no trailers, no bylines. No em-dashes in added lines. Files under `bench-harness/src/disasm/` all under 500 lines.

## Notes

What was proven runs on x86_64 Linux only, and the report mostly says so (lines 108, 141-147). The two places it says more than it shows are the cross-built objects, which prove format detection and symbol spelling and nothing about normalisation, and the otool and dumpbin paths, which the live tests as written will not reach on the machines the report hands them to. Findings 1 to 5 are the fixes; 6 is a check to make or to scope honestly.
