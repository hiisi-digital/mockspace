# Probes for the profile dimension and the two driver paths

Findings: `mock/research/202608180830_bench-profile-and-tree-assumptions-findings.md`.

Every script here states the case that must fail before it runs, and suppresses its result if the
control does not hold. Each is runnable on its own from this directory and cleans up after itself.
`ARVO_ROOT` overrides where a real consumer tree is looked for; the two scripts that need one skip
cleanly when it is absent rather than passing having done nothing.

| script | what it establishes |
|---|---|
| `01_build_override_dropped.sh` | A `[build]` override reaches the builds on both driver paths. Originally asserted the defect: the consumer-owned path dropped it. |
| `02_bench_test_argument_handling.sh` | `mock bench test` discarded a crate name silently, so a typo ran the whole tree and reported a pass. |
| `03_the_profile_gap_through_the_command.sh` | What the profile is worth on a real crate, through the command. Three arms, target dirs warmed. |
| `04_flags_never_reach_bench.sh` | A flag now reaches the bench subcommand. Originally asserted the defect: every flag was dropped at the dispatcher. |
| `05_bench_test_cannot_run_arvo.sh` | `mock bench test` does not terminate on arvo's tree (arvo's bug), and the workaround is now expressible through the tool. |

**`*_BEFORE_FIX.out` files are the discovery runs**, kept beside the current ones because the
scripts were flipped from asserting the defect to asserting the repair. A script that only ever
recorded the fixed state cannot show what it caught.

## Two things these probes got wrong first, kept because they are the useful part

**`03` timed cold target directories.** The first run reported 85s against 94s, which is a number
about rustc rather than about tests, and it would have supported no claim worth making. The warmup
in the current script exists because of it, and the reading was withdrawn before it was written
down.

**`03`'s first verdict was written for the wrong world.** It concluded that the two arms differ
meaningfully. They did not: both were debug, because `--release` never reached cargo, which is
what `04` then established. The apparent 127s-against-135s difference was noise between two
identical runs, and reading it as a profile comparison would have been exactly the error the
findings document is about, committed inside the probe written to demonstrate it.
