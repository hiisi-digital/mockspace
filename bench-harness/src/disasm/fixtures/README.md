# Recorded disassembler output

Real output of real disassemblers over linked libraries, read by
`disasm/tests_recorded.rs`, so the normalisation, the symbol lookup and the
duplicate decision are held to each object format and tool flavour on any
host, not only on the one the tests run on.

Recorded on 2026-11-16 on x86_64 Linux, with Ubuntu LLVM 18.1.3
`llvm-objdump` and GNU Binutils 2.42 `objdump`, from `no_std` cdylibs built by
the repository's pinned toolchain (`nightly-2026-05-28`) with `-C panic=abort`.
`no_std` is what lets one host link for every target: the host ELF library is
linked by the system linker, the others by `rust-lld` (`ld.lld` for aarch64
ELF, `ld64.lld` for Mach-O, `lld-link` with `/NODEFAULTLIB /NOENTRY` for PE).
The aarch64 libraries are linked with `--no-relax` (ELF) and
`-ignore_optimization_hints` (Mach-O), so the compiler's `adrp` and `add`
pairs stay as a large library keeps them rather than being relaxed to `adr`.
These are libraries as a linker lays them out, not objects, so branch, call
and data targets are resolved addresses that move when the layout moves.

Four libraries per target:

- `variant_scalar` and `variant_unrolled_x4`, from `probe.rs`: identical work
  under two crate names. `bench_name` returns the crate's name; in the two
  aarch64 libraries the code moves with the name's length, in the x86_64 ELF
  and Mach-O ones it does not and only the salted names of `do_work` differ.
  In a PE image the code comes first and nothing about it differs, so the
  second PE library is linked at another image base (`/BASE:0x190000000`) to
  make its addresses differ.
- `variant_other`, from `probe_other.rs`: different work.
- `variant_shifted`, from `probe_shifted.rs`: `probe.rs` with one more exported
  function placed before the rest, so the same `bench_entry` sits at another
  address with other branch, call and data targets (RIP-relative on x86_64,
  the `add` after an `adrp` on aarch64).

Every target has `llvm-<crate>.txt`, from `llvm-objdump -d --no-show-raw-insn`.
x86_64 ELF and PE also have `gnu-<crate>.txt`, from `objdump -d
--no-show-raw-insn`, the two formats this binutils reads (it has no Mach-O or
aarch64 backend), and PE has `gnu-<crate>.exports.txt` from `objdump -p`: GNU
labels no export in a DLL, so its `bench_entry` is found through the export
table.

The commands, per target `<t>`, crate `<c>` and source `<s>`:

```sh
rustc --target <t> --crate-type cdylib --crate-name <c> -O --edition 2021 \
  -C panic=abort <linker args> -o <library> <s>
llvm-objdump -d --no-show-raw-insn <library> > <t>/llvm-<c>.txt
objdump -d --no-show-raw-insn <library> > <t>/gnu-<c>.txt
objdump -p <library> > <t>/gnu-<c>.exports.txt
```

Not recorded here: `otool` and `dumpbin` output, a 32-bit PE image, and any
library linked against `std`. `tests_live.rs` reads the host's own `std`
libraries with every tool the host has, `otool` on macOS and `dumpbin` on
Windows included.
