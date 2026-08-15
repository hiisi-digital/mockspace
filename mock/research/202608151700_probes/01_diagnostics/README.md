# Probe 01: what the framework says when a person breaks a bench

Builds a minimal VALID benchspace, applies exactly one plausible authoring
mistake, calls `mockspace_bench_harness::tree::load`, prints the diagnostic.

Run against `origin/feat/bench-consolidation` (PR #21), because that branch
is the one that owns `tree.rs`.

## Negative controls, both present and both fired

- `00-control-valid` must load clean. **On the first run it did not**, because
  my scaffold's `[timing]` used `warmup_ms`/`measure_ms`, which the schema does
  not have. Every one of the fifteen cases then reported that same scaffold
  error instead of the mistake under test. The control caught a probe that
  would otherwise have "measured" fifteen identical diagnostics and concluded
  the framework reports one message for everything. `output-first-run-void.txt`
  is that void run, kept deliberately.
- `00b-control-must-fail` is a tree broken in a way the loader is known to
  refuse, so a clean sheet cannot be a probe that stopped erroring.

## Reproduction

```
cargo run -q          # from this directory, with the path dep repointed
```

`Cargo.toml` carries an absolute path dependency on the checkout it ran
against; repoint it before rerunning.
