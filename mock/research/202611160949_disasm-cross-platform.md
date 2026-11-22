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
A linked DLL also carries no COFF symbol table, so GNU `objdump` labels none of
its exports and could never find `bench_entry` in one.

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
- `disasm/pe_exports.rs`: for GNU `objdump` and for `dumpbin` on a DLL, neither
  of which labels an export without a symbol table or a PDB, the function is
  the stretch of the whole disassembly from its export's RVA to the next
  export's. One `Exports` holds the table, read by one reader per tool
  (`objdump -p`, `dumpbin /HEADERS /EXPORTS`), and cuts the stretch the same
  way for both. The last export runs to the end of its section.
- `disasm/format.rs`: the object format is read off the file's leading bytes
  (ELF, Mach-O in all four magics, PE through `e_lfanew`, a bare COFF object by
  its machine), and the symbol is spelled as that format spells it: underscored
  on Mach-O and in a 32-bit COFF object, bare on ELF and 64-bit COFF. A 32-bit
  PE image tries the bare export name first and the underscored one second,
  since no i686 DLL has been read here.
- `disasm/normalize.rs`: header, section and label lines are recognised in
  every tool's shape, the address column comes off whether it is `addr:` or
  otool's bare address, comments come off (`#`, `//`, `;`), every annotated
  target folds whether written `0x` or bare, spacing is made canonical, and
  RIP-relative displacements with their sign, any `adrp` page and the `add`
  after it fold. From `dumpbin`, a long hex token folds only as a branch or call target or as
  a whole `[...]` absolute address; every other one is an immediate and stays.
  An empty result is no evidence.
- `mock bench disasm <bench> <arm> <symbol>` builds the arm as `run` does
  (`src/bench_arms.rs`, shared with `run`) and writes the function's
  disassembly to `results/<bench>/disasm/<machine class>/<run>/<arm>/<symbol>.txt`,
  which `bench-layout` places in the layout and no run reader takes for a
  cell. The file is created exclusively; where one is there the next `-<k>`
  run is taken, and the header names the run directory the file sits in. The
  bench, arm and symbol names are refused before the arm is built where they
  cannot name a results file.
- `mock bench` dispatches from one table, `bench::SUBCOMMANDS`, of each name
  and what runs it, and a test holds every listing of the subcommands to it:
  `mock help bench`, the help `mock bench` prints, and the `cargo mock bench`
  row of `docs/USAGE_GUIDE.md`. The `benches` rule card and the
  `benchmarking` skill name the command too.
- Every path the tool and its tests write into a TOML document goes through
  one helper, `mockspace_manifest::toml_text::basic_string`, which quotes and
  escapes it, so a Windows path's backslashes are not read as escapes. The
  `[build] mockspace` spec's path is read back as TOML rather than sliced out
  of the text. The class is counted at the end.

## What was proven, where, and how

Every proof below ran on x86_64 Linux (Ubuntu, LLVM 18.1.3 `llvm-objdump`, GNU
Binutils 2.42 `objdump`, toolchain `nightly-2026-05-28`).

- Both reds reproduced on `origin/dev` at 2f11e87 before any change, and green
  after.
- Live, on `std` cdylibs built by `rustc` for the host, read by both
  `llvm-objdump` and GNU `objdump` (`disasm/tests_live.rs`): `bench_entry` is
  found and a missing symbol is an error, identical work in two crates is one
  duplicate pair, different work behind an identical dispatcher is not.
- Recorded, on any host (`disasm/tests_recorded.rs`, fixtures and the exact
  commands in `disasm/fixtures/README.md`): linked `no_std` libraries of
  x86_64 and aarch64 ELF, x86_64 and arm64 Mach-O, and x86_64 PE, linked here
  by the system linker and by `rust-lld`, read by `llvm-objdump` for all five
  and by GNU `objdump` for x86_64 ELF and PE. A guard holds every identical-work
  pair to differ in its instruction lines before normalisation. What the
  recorded output exercises: branch and call targets, RIP-relative
  displacements, the `add` after an `adrp`, salted names and image bases that
  move with the layout; the duplicate decision; the entry under the format's
  spelling and none under the other; GNU's export lookup in a DLL. What it does
  not: an `adrp` page that itself moves, otool and dumpbin output, a 32-bit PE
  image, and libraries linked against `std` on any target but the host's.
- The recorded `variant_scalar` library of each format is read live by every
  tool on the host that reads that format, and its format read off its real
  bytes, PE included.
- The `dumpbin` export reader, on hand-written `dumpbin /HEADERS /EXPORTS`
  output for the recorded PE library in dumpbin's documented layout: it reads
  the same table as GNU's reader of the real `objdump -p`, and both cut the
  same `bench_entry` out of the recorded disassembly. A real dumpbin read on
  Windows, of `/DISASM:NOBYTES` and of `/HEADERS /EXPORTS`, is still owed,
  under `prove_the_otool_and_dumpbin_disassembly_paths_on_real_hosts`.
- `mock bench disasm` end to end (`tests/bench_disasm_e2e.rs`): a tree with one
  generated-manifest arm, the artifact placed in the layout, a second beside the
  first, a missing function failing with nothing written.
- Every fix was taken back out of the code one at a time, 30 mutations
  recorded on a1d1fd3 (each `.out` names it), and each turned red the tests
  named against it in
  `mock/research/202611160949_disasm-cross-platform-mutations/README.md`; the
  directory holds each mutation as a patch and the test lines its run printed,
  with the command that replays it.

Not proven on a real host: `otool` and `dumpbin` output (their rules follow the
documented line shapes and are held by synthetic lines only), Windows and macOS
hosts as such, aarch64 Linux as a host, and a 32-bit PE image, whose path in
`format.rs` is held by synthetic header bytes only. `tests_live.rs` reads the
host's libraries with every tool the host has, `otool` on macOS and `dumpbin`
on Windows included, and fails on a host with no disassembler or no `rustc`,
so each bench machine proves its own platform when the suite runs there. The
live test that links every target with `rust-lld` is ignored under
`install_the_rustup_targets_the_cross_linked_disasm_test_need`, since no bench
machine carries those rustup targets; it passes here with `--ignored`.

## What is left

- `prove_the_otool_and_dumpbin_disassembly_paths_on_real_hosts`: run the live
  tests on a mac and on Windows, and record both tools' output as fixtures.
- `install_the_rustup_targets_the_cross_linked_disasm_test_need`: the ignored
  cross-linking test.
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

The second class, found by review: a filesystem path written into a TOML
string unescaped, which a Windows path's backslashes break (`C:\Users` reads
as the invalid escape `\U`). The search was
`grep -rn 'path = \\"{\|path = "{\|path = {:?}\|Value::from(.*to_string_lossy'`
over the Rust sources outside `mock/research/` and v2's `mock/crates/`. It
found 39 writes in 25 files, of which 8 are the tool's own (four in the
generated driver and arm manifests of `bench_gen.rs`, the lint-rules patch,
the lint crate manifest, `cargo-mock`'s `--engine` dependency, the nextest
junit path) and 31 are tests. Four mechanisms were in use (none, `{:?}`, a `\`
to `/` rewrite, and `toml_edit::Value`), and all 39 now go through
`basic_string`. The one read of such a
path, the `[build] mockspace` spec's in `dep_for_crate`, parses the spec as
TOML. Left alone: `#[path = ...]` in generated Rust, which is Rust's escaping
rather than TOML's and already right, and a `file://` URL built from a path in
`patch_tests.rs`, a URL rather than a TOML path.
