# Recorded disassembler output

Real output of real disassemblers, read by `disasm/tests_recorded.rs`, so the
normalisation and the symbol lookup are held to each object format and each
tool flavour on any host, not only on the one the tests run on.

Recorded on 2026-11-16 on x86_64 Linux, with Ubuntu LLVM 18.1.3
`llvm-objdump` and GNU Binutils 2.42 `objdump`, from objects compiled by the
repository's pinned toolchain (`nightly-2026-05-28`) for each target directory
here. `probe.rs` was compiled twice, as `variant_scalar` and
`variant_unrolled_x4`, and `probe_other.rs` once, as `variant_other`:

```sh
rustc --target <target> --crate-type cdylib --crate-name <crate> -O \
  --edition 2021 --emit=obj -o <crate>.o <probe.rs|probe_other.rs>
llvm-objdump -d --no-show-raw-insn <crate>.o > <target>/llvm-<crate>.txt
objdump -d --no-show-raw-insn <crate>.o > <target>/gnu-<crate>.txt
```

GNU output exists only for the two formats this host's binutils reads (ELF
x86_64 and COFF x86_64); it has no Mach-O or aarch64 backend. These are
objects, not linked libraries, because no linker for Mach-O or for MSVC
exists on a Linux host; an object carries the same section names, symbol
spellings and instruction syntax a library of that target does. `otool` and
`dumpbin` output is not recorded here; those two are proven only by the live
tests on a host that has them.
