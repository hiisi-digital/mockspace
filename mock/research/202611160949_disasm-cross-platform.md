# Disassembly across platforms

The bench harness's disassembly (the duplicate check before timing, the
`bench_entry` comparison, the names the warnings print, and now the
disassembly of one named function as an artifact) assumed one platform: an
LLVM-flavoured `objdump`, Mach-O symbol spelling as a fallback, and a library
name read with the host's prefix and suffix. This note records what was broken
on which platform, what the branch `work/disasm-cross-platform` changes, and
what was proven where.

## What was broken, per platform

On Linux with GNU binutils, `objdump_symbol` passed `--disassemble-symbols=`,
which only `llvm-objdump` reads; GNU exits non-zero, so `bench_entry` was never
extracted and the cheap comparison never ran. The whole-file comparison was
broken too: the normaliser kept everything after the first tab of a line, and
GNU's file header (`libvariant_scalar.so:     file format elf64-x86-64`) has
no tab, so the header stayed whole with the library's file name in it, and two
variants of identical work always differed. That is why
`identical_work_in_separate_crates_is_flagged_duplicate` was red on dev. GNU
also writes a branch target as bare hex (`call 11860 <f>`), which the `0x`
folding never saw, puts the raw bytes after the first tab, pads with spaces
where LLVM uses a tab, and writes `# 4c6a8 <sym>` comments. A RIP-relative
displacement (`0x3af2d(%rip)`) is layout, the distance to the data, and was
kept as if it were a field offset, on every x86_64 host and tool.

`short_name` stripped the host's `DLL_PREFIX` and `DLL_SUFFIX`, so a `.dylib`
path on Linux printed `alpha.dylib`, the other red test. The same host-based
reading sat in `entry_display` of `mock bench list` and in `build_for` of the
variant build plan, where a manifest's `libX.dylib` entry (accepted on every
host by `config.rs`) found no crate on Windows, whose prefix is empty.
`dylib_check` looked for `lib<x>.dylib` on every host, so it found nothing on
Linux or Windows.

On macOS arm64, LLVM writes comments with `;` (`mov x0, #0x0 ; =0`), which
nothing stripped; harmless only while both sides carried the same comment.
`otool` writes an `adrp` page as a bare decimal (`adrp x8, 1 ; 0x1000`), which
no fold covered. `extract_symbol_range` matched any line containing the
symbol, so `_bench_entry_helper:` would start the block. A symbol was looked
up bare first and underscored second on every format.

When `objdump` ran for one variant and the `otool` fallback for the other, the
two were compared across different byte ranges (a gap the old module named and
left). And a tool that ran and printed only headers gave an empty string, which
two variants then shared as a false duplicate.

On Windows nothing was looked for at all: no `dumpbin`, no `llvm-objdump.exe`.

## What the branch changes

- `bench-harness/src/library.rs`: `arm_name` reads the arm off the file's own
  suffix through one table (`.so` and `.dylib` take `lib`, `.dll` none), and
  either path separator ends a component. Used by the warnings, `mock bench
  list` and the variant build plan. `dylib_check` builds the host's name, since
  it loads the library on the host.
- `disasm/tool.rs`: the disassembler is found once per process, in the order
  `llvm-objdump` on `PATH` (or the highest `llvm-objdump-<N>`), the active
  toolchain's `llvm-tools` one, `objdump` classified by its `--version` (LLVM,
  Apple's included, or GNU), `otool` on macOS, `dumpbin` on Windows. One tool
  reads every variant of a run. Each flavour gets its own flags (`--disassemble=`
  for GNU, `--disassemble-symbols=` for LLVM, `--no-show-raw-insn` for both,
  `/DISASM:NOBYTES` for dumpbin); otool and dumpbin, which cannot filter by
  symbol, are cut to the symbol by an exact label match.
- `disasm/format.rs`: the object format is read off the file's leading bytes
  (ELF, Mach-O in all four magics, PE through `e_lfanew`, a bare COFF object by
  its machine), and the symbol is spelled as that format spells it: underscored
  on Mach-O and 32-bit COFF, bare on ELF and 64-bit COFF.
- `disasm/normalize.rs`: header, section and label lines are recognised in
  every tool's shape, the address column comes off whether it is `addr:` or
  otool's bare address, comments come off (`#`, `//`, `;`), every annotated
  target folds whether written `0x` or bare, spacing is made canonical, and
  RIP-relative displacements, any `adrp` page and dumpbin's full-width
  addresses and mangled call targets fold. An empty result is no evidence.
- `mock bench disasm <bench> <arm> <symbol>` builds the arm as `run` does
  (`build_arm`, shared with `run`) and writes the function's disassembly to
  `results/<bench>/disasm/<machine class>/<run>/<arm>.<symbol>.txt`, which
  `bench-layout` now places in the layout and no run reader takes for a cell.
  Written once; a second one of the same second takes a `-<k>` suffix.
- The `benches` rule card and the `benchmarking` skill name the command and the
  tool order.

## What was proven, where, and how

Every proof below ran on x86_64 Linux (Ubuntu, LLVM 18.1.3 `llvm-objdump`, GNU
Binutils 2.42 `objdump`, toolchain `nightly-2026-05-28`).

- Both reds reproduced on `origin/dev` at 2f11e87 before any change, and green
  after.
- Live, on real cdylibs built by `rustc` for the host, read by both
  `llvm-objdump` and GNU `objdump` (`disasm/tests_live.rs`): `bench_entry` is
  found and a missing symbol is an error, identical work in two crates is one
  duplicate pair, different work behind an identical dispatcher is not.
- Live, on objects cross-built for `x86_64-apple-darwin`,
  `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`
  and `aarch64-unknown-linux-gnu` (rustup targets installed for the purpose),
  read by `llvm-objdump`: the format is detected, the entry spelled as the
  format spells it, and the duplicate decision is right. The test skips a target
  whose standard library is not installed and says so; `aarch64-pc-windows-msvc`
  was skipped here.
- Recorded, on any host (`disasm/tests_recorded.rs`, fixtures and the exact
  commands in `disasm/fixtures/README.md`): the same three objects per target,
  `llvm-objdump` for all five targets and GNU `objdump` for ELF x86_64 and COFF
  x86_64, the two formats this binutils reads. The duplicate decision, the
  entry's block under the format's spelling and the wrong spelling finding
  nothing are held for each.
- `mock bench disasm` end to end (`tests/bench_disasm_e2e.rs`): a tree with one
  generated-manifest arm, the artifact placed in the layout, a second beside the
  first, a missing function failing with nothing written.
- Each behaviour's test was shown failing with that behaviour taken back out of
  the code, one mutation at a time: the GNU header rule (six tests red, the
  live and recorded duplicate tests among them), the GNU symbol flag, the
  host-based naming, the RIP fold, the bare-hex fold, the `;` comment rule, the
  Mach-O spelling, the empty-result rule, the `adrp` page fold, the exact label
  match, the dumpbin fold, `dylib_check`'s name, `build_for` with an empty host
  prefix (as on Windows), and the layout's placement of an artifact.

Not proven on a real host: `otool` and `dumpbin` output (their rules follow the
documented line shapes and are held by synthetic unit tests only; on a stock mac
Apple's LLVM `objdump` is found first, so otool runs only without it), Windows
and macOS hosts as such, aarch64 Linux as a host, 32-bit COFF's underscore
against a real object, and `aarch64-pc-windows-msvc`. `tests_live.rs` is not
gated to a platform, so it runs on every bench machine with whatever tools that
machine has and proves it there.

## What is left

- `prove_the_otool_and_dumpbin_disassembly_paths_on_real_hosts`: run the live
  tests on a mac without `objdump` and on Windows with only `dumpbin`, and
  record both tools' output as fixtures.
- `mock_bench_disasm_refuses_a_bench_named_by_a_member_path`: a composed-member
  bench named `a/b` gets no artifact until the layout says where its results
  sit.
- `custom_lint_e2e_tests_hard_code_the_so_suffix_and_unix_inode`: the same class
  outside the harness, in the custom-lint end-to-end tests.

## The class, counted

The search was for every place that reads or writes a library name, a symbol
spelling, a disassembler flag or a section name for one platform:
`grep -rn 'DLL_PREFIX\|DLL_SUFFIX\|DLL_EXTENSION\|"so"\|"dylib"\|"dll"\|lib{.*}\.\(so\|dylib\)\|objdump\|otool\|__TEXT'`
over the Rust sources of `origin/dev` at 2f11e87 outside the disassembly
module and `mock/research/`. It found 22 lines naming the host constants
and 7 hard-coded suffixes. Three of the 22, in two functions, read an arm's name
off a path with the host's constants (`entry_display`, `build_for`), the same
fault as `short_name` inside the module, and all three functions are fixed. The
other 19 build a path for the host that loads it (`resolve_variant_path`,
`arm_dylib_name`, the uplifted-cdylib lookup), test those, or are comments
saying why a name is not read with them, and are right. Of the hard-coded suffixes, `dylib_check` was wrong and is fixed, the
three that accept all three suffixes are right, `built.rs` defaulting a
suffixless file to `so` names nothing on disk, and the two in the custom-lint
end-to-end tests are the ticket above. Every disassembler flag, symbol spelling and
section name lived in `bench-harness/src/disasm.rs`, which this branch rewrote.
