Source: kaski channel (coord/kaski of clause-dev), research/mockspace-v2-port-evaluation.md, first posted 2026-11-02 10:55 UTC, read 2026-11-15.

# mockspace v2 port: what the work is, and what it costs

Job `mockspace-v2-review`, second job, 2 November 2026. Read-only: nothing was changed or pushed in any repository. Written for op, through console-coord.

## Framing

Op answered the three v2 questions on the morning of 2 November. The words that decide this job, from the DM of console-coord, verbatim: "I say we properly evaluate the work this would need and if it seems reasonable, just do it. I propose though, that we might just take what we have as v1, port it to the v2 mockspace and conventions, change only the backends from fs to refs and expand the existing plumbing as needed. Instead of fully rewriting the things v1 already has." On the fallback: "Yes, although ultimately we'll want to use our own git host in a VPs or something, so let us not get too attached to GitHub". On granularity: "One per namespace (Recommended)".

The intent, as I read it: the v2 shape becomes the mockspace trunks, with no separate `mock/crates` tree; mockspace's data leaves the main tree so a reader or a small contributor sees code; the storage backends move from the filesystem to refs, one ref per namespace, a branch allowed as fallback; nothing may rest on GitHub. The port-instead-of-rewrite is his vehicle, and this note tests it rather than assuming it. "I think", "might" and "I propose though" are his hedges, kept here as provenance and not carried as questions.

This note answers the six questions of the brief in order: the target shape, a module-by-module map of v1, what moves to refs and how a ref-held design stays tied to the code it governs, host neutrality, consumer migration, and the order of work with a total. It settles nothing that is op's. Section 3 names options for the design-versus-code question and stops there.

Method and limits, stated first because they bound every claim. Everything was read from `hiisi-digital/mockspace` `dev` at `0faa72c` and kaski `dev` at `955a4451c`, both on 2 November. mockspace declares no canon (v1 has no `mockspace.toml`), so the v2 spec at `mock/research/202605181400_mockspace-v2-spec.md` is read as the design and cited by line. Two pieces of evidence the brief asked for were not obtained. The v2 test run was refused again by the auto-mode classifier (reason `[Code from External]`); a hook also requires `CARGO_INCREMENTAL=0` on that command. I did not work around it, and console-coord has the exact command in a DM. What v2 passes is therefore not claimed; its test markers are counted, 920 across the eight crates (the first review counted 353 in core and the CLI alone). The canon tool was not run: `cargo mock` is not installed here and building it from the mockspace source is the same class of action as the refused run. The kaski tenets cited below were found by grep over `mock/registry/tenet/` and are cited by file and line; a grep finds rows and cannot show that the canon is silent on anything. Sizes in helper-days are my estimates, with the basis given in the next section.

## How the numbers are made

Lines are Rust lines counted with `wc -l`, split into tests and the rest by file name (`*_tests.rs`, `tests.rs`, `tests/`). Coupling to the filesystem is a count of `fs::` call sites in a module, and is a pointer, not a measure of work.

A helper-day is one helper session carrying one set to a landing. I price changed or new reviewed code with tests at 1,500 to 2,500 lines a helper-day. The basis is the repo's own pace: since 31 August, 77 non-merge commits on `src/`, `cargo-mock/` and `lint-rules/` added 62,267 lines of Rust and deleted 8,254, over 28 active days, about 2,200 added lines per active day with every session counted together (holds for: this repository, Rust files, tests included, 31 August to 2 November). Code that moves with only paths and imports changed is cheaper per line; I have no measurement for that rate and the figures below are judgement. Ranges are wide on purpose, and the widest are marked.

## In short

On the evidence the port is reasonable. The backend swap is smaller than it looks: the registry reads from one root in one 227-line file (`src/registry/load.rs:75`), called from five non-test sites. Most of v1's 77,000 lines move as they are. Three things are not backend swaps and carry most of the risk: rounds (v1's changelist model and v2's manifest and anchor model are two designs), the harness (hooks, drivers, and lint and tool crates that compile from files on disk), and the consumers (16 repositories besides mockspace, kaski with 19,034 tracked files). The total is 30 to 51 helper-days in ten sets, with a half-day probe first. Four decisions of op's gate the middle of the plan, listed in section 6. The first four sets are the same work as the narrower proposal of the earlier note, done in v2's shape, so starting loses nothing if op later narrows.

## 1. The target shape

mockspace after the port has one Cargo workspace at the root of the repository and no `mock/` directory. The v2 crates are today under `mock/crates/`; v1 is at the root (`src/`, `cargo-mock/`, `lint-rules/`, `mockspace-manifest/`, the bench crates). The target puts both under `crates/`:

```
Cargo.toml                 workspace
crates/
  mockspace-core           v2, survives as it is (minus `lint.rs`, see below)
  mockspace-config         v2, extended with v1's keys
  mockspace-template       v2
  mockspace-store          new, small: the backend trait and the remote sync
  mockspace-registry       ported from v1 src/registry, registry_merge, registry_append, append_merge
  mockspace-suite          ported from v1 src/suite
  mockspace-lint-rules     v1, name kept (consumers depend on it by that package name)
  mockspace                v1 engine: entry, bootstrap, render_agent, custom_lints, panel, document
  mockspace-cli            v2 `mock` binary, verbs merged with v1's
  cargo-mock               the launcher, name kept
  bench-core, bench-harness, bench-macro, bench-matrix, mockspace-manifest
docs/                      rendered
```

mockspace's own design rounds, research and registry rows go to refs like any consumer's, so the repository dogfoods the shape before any consumer moves.

Survivors from v2, as they are. `mockspace-core` is 13,132 lines. Without `lint.rs` (2,298) it is 10,834 lines of: slug, namespace, `ref_path`, `branch_name`, `iso8601`, entity and identity (1,367 together); phase, transition, typestate, atomicity, anchor, bookkeeping, manifest, verifier, round and task (4,344); and the ref I/O under `io/` (5,123: `repo`, `ref_tree`, `ref_write`, `lock`, `advance`, `seal`, `archive`, `anchor_capture`, `task`). The ref I/O is gix-based, in process, local only: it reads and writes trees and commits and updates refs with a compare-and-swap expectation (`io/ref_write.rs:97-103`), and has no `git` subprocess outside tests and no fetch or push anywhere (a grep of the non-test v2 sources for `ls-remote`, `force-with-lease`, `refspec` and the arguments `"push"` and `"fetch"` finds only lint gate names). `mockspace-config` (1,999 lines) and `mockspace-template` (1,066) survive with changes. `mockspace-test-fixtures` (461) is a useful survivor for the port's own tests.

What changes in the survivors. `write_round_ref` always writes a parentless commit (`io/ref_write.rs:78-80`), which is right for a round and wrong for a ticket namespace that needs history; a history-bearing write is a small addition. The remote half (fetch with an explicit refspec, push with a lease, retry on a lost race) does not exist and is the main new code, about 400 to 600 lines. `msg` already has the design in shell: `tools/msg/lib/store.sh` (187 lines) fetches a channel branch into a private ref `refs/msg/<channel>`, reads it with `cat-file` and `ls-tree`, writes by `hash-object`, a temporary index and `commit-tree`, pushes `commit:refs/heads/coord/<channel>`, and on a rejected push fetches and rebuilds on the new tip, with jittered backoff and a test of the race (`tools/msg/tests/race_test.sh`). It is in daily use from cloud containers, including this one today. The port should take that loop, not invent one.

The config is not a small change. kaski's live `mockspace.toml` uses `mock_dir`, `src_dirs`, `canon_paths`, `registry_namespaces`, `append_only`, `crate_prefix`, `abi_version`, `proc_macro_crates` and `module_crates`; v2's `mockspace-config` contains none of them (grep over its `src/`: 0 hits each, only `lint-crates` appears). Its sections are `refs`, `hosts`, `imports`, `profile`, `transparency`, `undo` and others v1 never read. So the config schema is a merge of two schemas, and v1's `config.rs` (2,396 lines, 91 items) is read by almost every v1 module.

Not carried, with the reason. These are the v2 parts that no consumer or plan asks for, with their spec spans: imports, exports, `mock://`, signing and trust-on-first-use, the lockfile, the transparency log (Part IV, lines 1983 to 2551, 570 spec lines); profiles and the hook protocol as specified (Part V, 2552 to 2821); undo and redo and `mock doctor` (§42 to §43, lines 3112 to 3278); the PR lifecycle and forge projection (§48, lines 3582 to 3646), which op's host-neutrality answer removes; the multi-active-round mode (spec line 4287, already deferred). The `.mock/` rendered surface (§20) is carried only as far as the harness needs it (section 3). On the code side that leaves two crates with no home: `mockspace-rs` (24,908 lines, v2's Rust lint engine, bootstrap, render and config loader) and `mockspace-builtin-lints` (3,282, a viola cdylib with git dependencies over ssh on hilavitkutin, viola, arvo and notko). No consumer runs either. v1's lint stack (`lint-rules` 24,661 lines, `custom_lints` 5,965) is what the consumers' lints and tools are written against, and the port cannot also run a second engine. My recommendation is to move the two v2 crates to a named branch so the trunk is clean and nothing is deleted, and to take from them only the v2 directive vocabulary (`docs/MIGRATION-v1-to-v2-lints.md`), which is a source-comment convention and works on either engine. Whether to carry them instead, or to move the consumers' lints onto viola plugins, is op's call (section 6, decision D3).

What "no `mock/crates`" means in a consumer. In kaski the crates are `mock/crates` (102 crates, 2,528 files, 14.9 MB) because `mockspace.toml` sets `mock_dir = "mock"` and `src_dirs = ["crates", "applets/console/crates"]`. The target moves them to `crates/` with `Cargo.toml` and `Cargo.lock` at the root. That is a path change in 2,528 files that touches every open branch, and it is independent of refs: the data can move first and this later. The per-crate templates (`README.md.tmpl`, `DESIGN.md.tmpl`, `BACKLOG.md.tmpl`, `SHAME.md.tmpl`, 222 files in kaski) are the design tier. The v2 spec says they live under `mock/` "or under the harness ref" (spec line 199) and does not choose. Where they go decides the atomicity question in section 3.

## 2. v1, module by module

Lines are non-test / test, from `0faa72c`. Disposition: A is port as it is, B is port with a backend change from filesystem to ref, C is rewrite. Days are helper-days for the port work on that module alone and do not repeat in section 6's sets, which group them.

| Module | Lines | `fs::` sites | Disposition | What changes | Days |
|---|---|---|---|---|---|
| `src/registry` (load, resolve, validate, schema replay, refs, rowref, render) | 6,192 / 1,192 | 102 | B | `load.rs` (227 lines) reads `mock_dir/registry` with one `read_dir` and one `read_to_string`; it takes a `Store`. Five callers: `entry/dispatch.rs:1113`, `:1443`, `entry/tool.rs:231`, `registry/mod.rs:1498`, `registry_append/mod.rs:78`. The rest is in-memory | 0.5 to 1 |
| `src/registry_merge`, `registry_append`, `append_merge` | 1,358 / 578 | 9 | A | pure text merge by namespace and id; becomes the race handler of the registry writer. `registry_append` reads through `load_registry` and gets the same `Store` | 0.5 |
| `src/design_round` and the `changelist_*` lints | 1,700 / 1,847 and 2,837 in `lint-rules` | 63 | C | v1 has changelists (`lock`, `deprecate`, `unlock`, `close`, `archive`, `rename-round`, `round-id`; `src/design_round/mod.rs:3-9`) and phases derived from flat files. v2 has manifests, anchors and verifiers on a round ref. The two have to be merged into one model. See below | 4 to 6 |
| `src/suite` (planner, fingerprint, nextest, history) | 5,829 / 4,772 | 94 | A, and B for history | the planner reads the source tree and stays. `history.rs` (415), `history_merge.rs` (316) and `history_guard.rs` write `mock/test-history/**/*.json`, committed and guarded at the pre-commit hook (`history_guard.rs:6-14`); they take a `Store` and the guard moves into the writer | 1 to 2 |
| `src/bootstrap` (drivers, hooks, shook, gitignore, activate) | 5,728 / 2,716 | 244 | B, partly C | the registry, round and test-history merge drivers and their `.gitattributes` lines go once those move; hooks gain the fetch refspec and a writer-side gate; the lint and tool builds need files on disk (below) | 3 to 4 |
| `src/render_agent` (rules, skills, hooks to `.claude/`) | 5,204 / 4,895 | 218 | A | reads `mock/agent` (6 files in kaski) and registry rows; source path moves with the harness. v2's embedded-fragment model has no consumer override (`docs/MIGRATION-v1-to-v2-agent.md`), so v1's render is the one to port | 0.5 to 1 |
| `src/entry` (CLI dispatch, check, nuke, edit, tool, test, panel) | 8,464 / 2,917 | 191 | A, B | round verbs are replaced by the merged CLI; the rest takes the `Store` where it names a data path | 1 to 2 |
| `lint-rules` | 19,817 / 4,844 | many | A | package name and public surface stay: `RegistryView`, `Lint`, `RepoLint`, `Tool`, `ToolContext`, `lint_pack!`. kaski's tools depend on it as `mockspace` (`mock/tools/tickets/Cargo.toml:16`, `canon/Cargo.toml:16`, `corpus/Cargo.toml:12`). Only the changelist family (counted above) goes | 0.5 |
| `src/custom_lints`, `src/lint`, `lint_catalogue` | 4,595 / 3,139 | 294 | A | builds the consumer's `mock/lints/*.rs` into a cdylib in a shared store and dlopens it (`custom_lints.rs:6-26`). Needs the lint sources on disk, so the harness must materialise or keep them (below) | 1 |
| `src/config.rs` and v2 `mockspace-config` | 2,396 and 1,999 | 7 | C | one schema; v2's types extended with v1's keys and a per-namespace `store = "dir" or "ref"` | 2 to 3 |
| `cargo-mock` launcher | 689 / 378 | few | A | `CANONICAL_URL` (`cargo-mock/src/lib.rs:35`) becomes a setting | 0.25 |
| bench stack (`bench.rs`, `bench_gen`, `bench_docs`, `bench_rev`, four `bench-*` crates) | about 23,800 | 69 in `bench.rs` | A | already shared at the root between v1 and v2 (`mock/research/202605222330_v1-parity-audit.md`, area 1); bench results are data, see section 3 | 0.25 |
| `panel`, `document`, `render_design`, `render_md`, `parse`, `graph`, `autofix`, `deny`, `plumbing`, `pack_pin`, `tool_catalogue`, `mockspace-manifest` | about 8,400 | about 100 | A | path updates | 0.5 |

The registry is the best case for the port and the claim "change only the backends" holds there: one root, five callers, an in-memory model after load. It holds for test history too. It does not hold for rounds, and it holds with a cost for the harness.

Rounds. v1's rule family `changelist_required`, `changelist_immutability`, `changelist_seal`, `changelist_lock`, `changelist_doc_gate`, `canon_not_while_panel_open` and their helpers (2,837 lines in `lint-rules`) read `design_rounds/` from the working tree on every commit. Those gates cannot read a ref the way they read a directory, because the thing they guard changes: under v2 a phase transition is a verb that validates, seals and writes a ref (`io/advance.rs`, `io/seal.rs`, spec §24), and the commit-time lint has nothing to inspect. So rounds are a design merge: either keep v1's changelist vocabulary and store it on `refs/mock/round/<slug>` with v1's verbs, or adopt v2's manifests, anchors and the closed verifier catalogue, with v1's verbs becoming v2's `phase plan|apply|finish|replan`. The brief from console-coord lists the phase vocabulary among the survivors, which points to v2's six phases (TOPIC, PLAN.DOC, APPLY.DOC, PLAN.SRC, APPLY.SRC, DONE) over v1's five (TOPIC, DOC, DRAFT, IMPL, CLOSED, `homma/mock/WORKFLOW.md.tmpl`); op confirms it as D2. v2 has no verb that creates a round (`mockspace-cli/tests/e2e_phase.rs:1-8`), so a round on a ref is seeded by library calls only today, and nothing converts a consumer's flat files; `mock migrate` prints a plan and moves nothing. Closed rounds do not need interpreting: kaski's 809 items under `mock/design_rounds` and homma's 346 files can go whole into `refs/mock/round-archive` (spec §21, line 1678). Open rounds are the real conversion. This is the widest range in the plan (4 to 6 days) and the one I trust least.

The harness. v1 compiles two things from files in the tree: the consumer's lints (`mock/lints/*.rs`, 64 files in kaski) and its tools (`mock/tools/*`, 28 tool directories, 155 files). The v2 spec puts `lints/`, `agent/`, `templates/`, `hooks/` and `mockspace.toml` on a harness ref rendered into a gitignored `.mock/` (spec lines 1680-1692, 1553-1576), with a stated reason: a pull-request author cannot ship a lint, because lints are not on any source branch (spec lines 1712-1719). Tools are not in v2's harness (spec line 1694 reserves `tools/` and specifies nothing). For the port the harness is the one place where the filesystem is needed after the move, so it needs a materialisation step that writes the ref's tree to disk before the build; v1 already keeps a shared build store for lints keyed by repository identity (`custom_lints.rs:6-26`), which is half of that.

## 3. What leaves the tree, and what ties it back

Measured on kaski `dev` (19,034 tracked files, 396.8 MB, shallow pack 347.6 MiB; `git ls-tree -r -l HEAD` summed by top-level path):

| Path | Files | MB | Class | Ref (one per namespace) |
|---|---|---|---|---|
| `mock/research` | 6,264 | 301.8 | record | `refs/mock/research` |
| `mock/design_rounds` | 4,224 | 9.1 | record | active: `refs/mock/round/<slug>`; closed: `refs/mock/round-archive` |
| `mock/registry/ticket` | 1,365 | n/a | data | `refs/mock/registry/ticket` |
| `mock/registry` other namespaces: `ikiuni-renderer` 196, `bevy` 60, `tenet` 33, `ikiuni` 20, `intake` 19, `note` 10, `subject.toml` | 339 | n/a | canon and data | `refs/mock/registry/<namespace>` |
| `mock/registry` in all | 1,704 | 14.3 | | |
| `mock/benches` | 2,552 | 14.8 | mixed: 2,195 result files (`md`, `json`, `csv`, `tsv`) and 271 `rs` sources | results to `refs/mock/bench`; sources stay |
| `mock/test-history` | 309 | 0.7 | data | `refs/mock/test-history` |
| `mock/crates` | 2,528 | 14.9 | code and design tier | `crates/` |
| `mock/applets`, `mock/vendor` | 359 | 5.6 | code | stays |
| `mock/tools`, `mock/lints`, `mock/agent`, root templates and tomls | about 240 | about 2.0 | harness | harness ref, materialised |
| `content`, `docs` (rendered), root files | about 860 | 33.8 | stays | |

Research, rounds, registry, test history and bench results are 340.7 MB of 396.8 (85.9%) and 15,053 of 19,034 files (79.1%); without the bench directory, 325.9 MB (82.1%) and 12,501 files (65.7%). A shallow clone of the trunk stops carrying all of that. The history stays in the pack: the data's past commits are already in `dev`, and rewriting them is not on the table (section 5).

Homma, for comparison, has 468 tracked files and 1.8 MB, with no registry: `mock/crates` 86 files, `mock/design_rounds` 346 files. It is the right first consumer after mockspace.

Where op's reach is longer than the first review's. The first review put operational data on refs and kept canon and design in the tree. Op now wants a reader to see code only, which also moves canon rows, design rounds and the research record. Moving them is mechanically the same as moving tickets. What it breaks is the chain canon, design, code, which today is held by one fact: a commit is one tree. The lints that check a design against its code (`design_doc_source_mismatch`, the changelist family, `deprecation_comparison`) read both from the same working tree, and the commit gate runs over the merge commit, so a design and the code it governs cannot disagree at a landing. On refs there are two histories, and something must say which design snapshot a trunk commit answers to.

The options below are named with their costs and not settled. They go to op.

Option A, the per-crate templates stay in the trunk tree next to the code. Rounds, research, registry rows and bench results go to refs. The design-source pair (`DESIGN.md.tmpl` and the source it claims things about) stays atomic by construction, because both are in one commit. Canon rows leave the tree, so canon and design are tied by citation only, checked at landing against the canon ref's tip. Cost: the trunk keeps 222 template files in kaski beside the crates, and the tree still shows design files to a reader who wants only code. Smallest change, least of what op asked for.

Option B, a pin. The trunk carries one small file naming the commit of each pinned data ref it was built and gated against, like a lockfile. A design or canon change is a commit on its ref followed by a trunk commit that changes the code and bumps the pin together; the gate resolves the pin and checks the claims against that snapshot. Atomic by construction, since the pin is in the tree. Cost: every change to a pinned namespace that matters shows up in the trunk as a pin bump, so some churn returns for the pinned namespaces (tickets would not be pinned); the pin file needs a merge rule (take the descendant of both, a one-line ancestry check); the data ref must be pushed before the trunk commit, and a clone that lacks the ref cannot build or lint, so the failure has to be loud. A reader still sees one file.

Option C, the data commit as an extra parent of the landing merge. `land_local` already makes a merge commit on every landing (`_land_local_attempt`, `land-core.sh:514-648`), so a second parent costs nothing at landing time, and reachability makes the data travel with the trunk on a full `git fetch` of `dev`, with no custom refspec. Atomic by git itself. Cost: the data's history and trees become ancestors of `dev`, so a full clone downloads the research again (301.8 MB at the tip today), and `git log` on `dev` shows data commits unless `--first-parent` is used. A shallow clone, which is what cloud containers take, skips the parent, so it has neither the weight nor the data, and reading the data there needs a deepening fetch. This option serves atomicity and loses the reason for moving for every full clone.

Option D, design and canon stay in the tree and only operational data and records leave (the split of the first review). No atomicity problem. A reader still sees `mock/design_rounds` and the canon rows (9.1 MB and 33 tenet files in kaski), but not the research (301.8 MB), the tickets or test history.

Option E, v2 as specified: a round and its source branch are paired by slug, with independent histories, verified at phase transitions by running the verifier at the source-side branch tip, and a lost race is preserved as a side branch before reset (spec §21 and §24, lines 1652-1678 and 1787-1830). Consistency is a protocol, not a property of a commit: a plain trunk commit made outside the tool is not checked by it, so the gate still has to check on the trunk, and the drift states have their own findings catalogue (D001 to D044, §43 and §55). Needs the `phase` verbs wherever a helper changes a design, and the helpers' flow (`land-own`, `ticket`, `job-brief`) has no place for them today.

Also relevant to the choice, from the repository. Two ratified kaski tenets name the trunk as where registry changes land: `mock/registry/tenet/console.toml:124` ("What it edits in the registry reaches the repository's trunk by the console's own commit") and `mock/registry/tenet/workflow.toml:222` ("A ticket or a question so filed reaches the trunk as part of filing it, without anyone landing it by hand"). Their intent, that it reaches the repository by itself with no manual landing, survives a move to a data ref; the word "trunk" does not. Amending them is a canon change and op's. The superseded predecessor of the console tenet records that an orphan branch for the registry queue "was his vehicle at the time" (`console.toml:114`), later replaced by the direct commit to the trunk, so the idea of off-trunk registry data has been through his hands once before. Read by grep, not by the canon tool.

What each option does to the two data classes that are not in dispute: tickets, plan rows, intake, test history and bench results can leave under any of A to E, because nothing in a commit depends on their being in the same tree. The churn win of the first review comes from those, and none of A to E changes it.

## 4. Host neutrality

Nothing in the port needs a forge API. The port's own operations are `fetch` and `push` with explicit refspecs, `push --force-with-lease`, and the plumbing `hash-object`, `read-tree`, `write-tree`, `commit-tree`, `update-ref`. The two requirements on a host are that it accepts pushes of refs outside `refs/heads` (git's default; untested for GitHub and for the cloud proxy, and see below) and that it serves them to a fetch with a given refspec. The probe (P0) should include a bare repository over ssh as the neutral control, so a failure on the proxy can be told from a failure of the idea.

A fact that bears on that probe: the coordinator's state file records that the proxy "refuses ref deletion and tag pushes" (`.shared/state/kaski.md:60`). A tag is a ref outside `refs/heads`, so a custom `refs/mock/*` push may be refused the same way. The branch fallback is evidenced to work from a container, since `msg` pushes `refs/heads/coord/*` daily. `msg` shows the shape that makes both work: the local ref is private (`refs/msg/<channel>`) and the remote ref is a branch, so the store takes a pair, (local ref, remote ref), and the fallback is a setting.

Every place the port, the scripts or the console lean on the forge, found by grep and read:

| Place | What it uses | Plain-git form |
|---|---|---|
| `land-core.sh:95` `land_squash`, `:302` `_land_open_pr`, `:685` `land_release_branch`; `open-pr` (5 calls) | `gh api` for pull requests: squash-merge, find the open PR of a branch, close it | local landing is already plain git (`land_local`, push at `:648`). The PR path is only the fallback where the forge refuses a push to `dev` (`_land_local_via_pr`, `:665`), which is mockspace. On a host you own that refusal does not exist; a `pre-receive` hook is the plain-git form of a branch ruleset |
| `.shared/scripts/hygiene/unproposed-branches-forge.sh`, `delete-stale-branches.sh`, `repo-calibration.sh`, `remote/repos-needed`, `msg` board (`tools/msg/lib/board.sh:26-30`) | `gh` for pull request state and branch push times | the board has a git-only path (`board.sh:43-57`); the rest read branches and refs by `ls-remote` |
| the console, static-host mode: `console-core/src/github.rs` (1,431 lines), `github/home.rs`, `github/agenda.rs`, `console-app/src/{gh,signin,connect}.rs` (934); the CSP pinned to `api.github.com` in `mock/applets/console/build:51-52` | GitHub REST: `contents`, `git/blobs`, `git/trees`, `git/commits`, `git/refs/heads/*`, `commits`, `pulls`; a GitHub App sign-in | the console has a server mode that reads local clones by path (`README.md:42-48`, `repo = "/srv/kaski"`). On a self-hosted host that is the form to keep. The static-host mode needs a transport trait with GitHub REST as one implementation. I did not read whether the server mode's API covers the writes the static mode makes through the Git Data API |
| `ssh://git@github.com/hiisi-digital/mockspace.git` spelled in 31 files of kaski (the tool and lint `Cargo.toml` git dependencies), `cargo-mock/src/lib.rs:35` `CANONICAL_URL`, `bootstrap/mod.rs:115`, `bench_gen.rs:44`, `bench.rs:1701`, `custom_lints/patch.rs:42-43` | a GitHub address as a plain git URL | one `url.<base>.insteadOf` rule in git config re-points them all, with cargo fetching through the git CLI; untested here. The launcher's constant and the generated strings become settings |
| agent rules about `gh pr create` titles (`render_agent/message_*_tests.rs`) | a rule text, not a call | shrinks with the PR path |
| v2: `ForgeKind { Github, Forgejo }` (`mockspace-config/src/config.rs:207-209`), spec §48 | optional forge projection, skipped by `--no-forge` (spec line 1806) | left out of the port |
| the cloud container's proxy, the `mcp__github__*` tools, the session's GitHub App | platform, not mockspace | outside the port; named because the probe depends on the proxy |

Canon on the console, by grep: `mock/registry/tenet/console.toml:124` says the console "does everything through git and does it wherever it runs: on the lead designer's machine as the main editor, on a server with a clone of its own, and on a static host with no server of ours". Host neutrality is already the stated intent there; the code behind the static-host mode is where GitHub is spelled.

One consequence worth stating for a self-hosted host. A `pre-receive` hook can run the registry and schema checks on the data refs on the server, which is the plain-git place for the gate that data pushes lose when they stop going through a commit. On GitHub there is no such place, so for now the checks run in the writer (`ticket` today runs the repository's commit gate in a throwaway worktree before it pushes; `ticket:20-24`).

## 5. Consumer migration

Consumers. A code search of the `orgrinrt` organisation for `mockspace.toml` finds `applets`, `arvo`, `hilavitkutin`, `hokkeli`, `homma`, `huurre`, `ikiuni-renderer`, `kamu`, `kenno`, `kirjo`, `kolli`, `muisti`, `renki`, `riimu` and `vehje` (three of them with the config inside `mock/`), plus `-stale` copies. kaski does not appear in that result though its `mockspace.toml` exists, so the search is a lower bound. With kaski that is 16 consumers, and mockspace itself is a 17th.

How two shapes coexist. The v2 spec's own migration is additive until `mock migrate finalize`: the old directory stays on the branches while the refs exist, and the shape change lands with a release (spec §51, lines 3713-3766; "Old release tags" keep `mock/` and are not remediated). The port should keep that rule per namespace:

1. The engine reads a namespace from the tree or from a ref, by `store = ...` in the config. Both exist in one release.
2. A namespace is copied to its ref, verified by tree hash, and the tree copy is deleted in the same landing, so the two copies do not live side by side. For tickets this is the cut the first review recommended.
3. The crates move (`mock/crates` to `crates/`) is its own cut, last, in a freeze window, since it conflicts with every open branch.

How long they coexist. The engine supports both shapes until the last consumer has finalised. I would hold that window short per consumer, a day or two for a small one, and I have no data on how long the large ones need; kaski has several helper branches in flight at a time (six tracks on 2 November, `.shared/state/kaski.md:67`), which is what a crates-move freeze has to drain.

Order. mockspace itself first, then homma (468 files, no registry, rounds and crates only), then the small consumers, then kaski last, because kaski has the largest data (396.8 MB), the live ticket flow and the most tooling pointing at the paths: 53 files in kaski outside research and rounds name `mock/registry` (17 of them console tests), and 45 files in `.shared/scripts`, `tools` and `.shared/skills` do, among them `ticket`, `ticket-batch`, `job-brief`, `briefs-to-jobs`, `session-worker.md`, `asked-before.sh`. Two consumers are not ready regardless: `hilavitkutin` and `vehje` cannot push, because their lockfiles pin `mockspace` and `arvo` revisions that no longer exist on the remote (workspace ticket `hilavitkutin-and-vehje-pin-arvo-revisions-that-no-longer-exist` in `.shared/scripts/life/workspace.toml`). That has to be fixed before they can take any migration.

Pins decide how the port lands. Consumers pin mockspace by git revision in their lockfiles, and a history rewrite of `arvo` is what orphaned those pins. So the port lands as ordinary commits on `dev`, never by rewriting history, and the old engine stays installable by revision, which also keeps old release tags usable.

The compatibility rule that protects the tools: `mockspace-lint-rules` keeps its package name and its public surface (`RegistryView`, `Lint`, `RepoLint`, `Tool`, `ToolContext`, `lint_pack!`) through the port, with only the constructor behind `RegistryView` changing. kaski's tools depend on it by git revision of `dev`; if it changes shape, the 31 files that name it (tool and lint `Cargo.toml` files) and 64 lint files change with it.

## 6. The order of work

Ten sets, probe first. Each is one helper on one branch; dependencies are what must have landed. "Op decides" is what has to be answered before the set starts, from the open items in sections 1 to 5.

| Set | Work | Needs | Days | Op decides before it starts |
|---|---|---|---|---|
| P0 | Probe. In a scratch repository and a bare repository over ssh, create `refs/mock/registry/probe` and `refs/heads/mock/probe`; push, fetch with an explicit refspec, shallow clone with the refspec, `ls-remote`, from a cloud container through the proxy; and read both through the GitHub Git Data API. Report which of the two forms each path allows | none | 0.5 | D6: where it may push. I would use a scratch repository, since a probe ref in kaski is an outward write |
| S1 | `mockspace-store`: the `Store` trait (directory or ref), history-bearing ref writes, fetch and push by explicit refspec with a lease, retry on a lost race with a merge hook, taking `msg/lib/store.sh` as the model; tests with the race case | P0 | 1.5 to 2.5 | Commit identity on data refs (v2 uses a synthetic `mockspace <noreply@mockspace.local>`, `msg` pins a named author) |
| S3 | Registry on `Store`: `load_registry` and `registry_append` take a store; `registry_merge` becomes the race handler; the registry checks run in the writer | S1 | 1 to 2 | Which namespaces go first; D5: the amendment of the two tenets that name the trunk (`console.toml:124`, `workflow.toml:222`) |
| S5 | Test history on `Store`; the pre-commit guard moves into the writer | S1 | 1 to 2 | none |
| S7 | Scripts and console: `ticket`, `ticket-batch`, `land-own`, `job-brief`, `briefs-to-jobs` and the console's reads and writes through the store; transport trait for the console; `gh` uses per section 4 | S3 | 3 to 5 | Whether the static-host console keeps GitHub REST as one transport, or the server mode becomes the only form |
| S2 | Reshape mockspace: one workspace under `crates/`, the v2 survivors and the ported v1 crates, config schema merged, launcher setting for the URL | none, but best after S3 so that a moved file keeps its history | 3 to 5 | D3: carry, park or drop `mockspace-rs` and `mockspace-builtin-lints`; crate names; D1: where per-crate templates live |
| S4 | Rounds and canon on refs: v1's changelist verbs onto v2's phase machinery, `round new`, conversion of open rounds, closed rounds to the archive ref, the changelist lints removed | S2 and a choice among section 3's options | 4 to 6 | D1: the atomicity option (A to E); whether v1's changelist vocabulary or v2's manifests and verifiers stand |
| S6 | Harness: drivers removed, fetch refspec and writer-side gate in bootstrap and hooks, materialisation of lints, tools and agent files, shared build store | S2, S4 | 3 to 4 | The gate placement: in the writer, on a self-hosted server, or both; D6: the refspec for cloud clones (op owns the environment setup, so a setup-script change is his) |
| S8 | Consumers, in the order of section 5: mockspace 0.5 to 1, homma 1, 14 smaller consumers at 0.5 to 1 each, kaski 4 to 6 (data cut first, crates move last in a freeze) | S3, S4, S6; per consumer, the pins fixed | 12 to 22 | D7: the consumer order and the kaski freeze window; D4: the version label |
| S9 | Retire the old shapes, amend the v2 spec to what is built and what is deferred, rewrite the three migration guides, bump to 0.1.0 | S8 | 1 to 2 | none beyond D4 |

Total: 30 to 51 helper-days (the sums of the low and of the high figures; the sets that carry the most doubt are S4, S7 and S8). Along the longest chain, P0, S1, S2, S4, the kaski cut of S8, S9, the elapsed work is about 15 to 22 days if the other sets run beside it. In practice less runs beside it than the table suggests, because S3 and S5 share the registry and suite crates, and S4 and S6 both change `entry` and `bootstrap`, so those pairs would collide if run side by side.

What this does not count: op's reading time, the canon amendments, the cost of a freeze to the helpers working on kaski, the pin repairs for `hilavitkutin` and `vehje`, and any follow-up when the probe fails on the proxy.

Recommended sequence, value first. P0, S1, S3, S5, S7 and the ticket cut of S8 deliver the churn win of the first review in about 8 to 14 helper-days, done in place before any file moves. Then S2, S4, S6 reshape and move the records, and the rest of S8 and S9 follow. The first four sets are the same work whether or not the rest is done, so the plan can stop after them without waste.

## Risks

The probe may find the proxy refusing refs outside `refs/heads`, as it refuses tags. The fallback is the branch form, which op allowed, and the store takes a pair of refs, so the code does not change; what is lost is the hidden-from-routine-git property in the forge's branch list.

Rounds are where the port is least a port: v1's changelists and v2's manifests, anchors and verifiers are two designs to merge, and I have given them the widest range. If op wants v1's changelist model kept, S4 shrinks and the phase vocabulary the brief names as surviving does not.

Data writes skip the commit gate, since nothing is committed in a worktree. The gate over tickets today is the repository's commit gate, run in a throwaway worktree by `ticket`. Without a deliberate replacement (the writer, or a server hook) the checks on data silently vanish. The same refs fix only the half of the landing problem where `dev` moves under the gate; the gate on a landing of research notes still runs the whole lint pack (the ticket `a_docs_only_landing_on_kaski_dev_outlives_a_30_minute`, text in `.shared/scripts/life/tickets-workspace.toml:69-75`), and scoping it by what a change touches stays a separate piece of work.

The landing order of writes is verified now, and it is the one the first review feared. `land_local` pushes the merge to `dev` (`land-core.sh:648`); `land-branch` then tells the channel (`land-branch:306`) and then moves the job's ticket with `ticket landed` (`:311`), which is a separate commit, gate and push to `dev`; a failure of the last two is a warning and the landing stands (`land-branch:307`, `:312`). So a crash between the push and the ticket move leaves a landed branch with an unmoved ticket today. Moving tickets to a ref does not add that failure, and the recovery rule, that the next check reconciles a landed branch with an unmoved ticket by `cargo mock tickets landed`, should be written before S7.

Cloud clones are shallow, single-branch and take the default refspec, so `refs/mock/*` is not fetched unless the clone config says so. A failed fetch must say so loudly and never read as an empty registry (rule `never-discard-stderr-on-a-check`). The setup script of the helper environment is op's to change.

The parked v2 lint engine (28,190 lines in two crates) will rot on a branch. That is acceptable only if the directive vocabulary is taken first, and if nobody expects it to come back without a consumer.

## What stays op's

D1. The atomicity option for design and canon against code, A to E in section 3, and with it whether the per-crate templates stay beside the source or go to a ref. This gates S2 and S4 and so the rest.
D2. Whether v1's changelist model or v2's manifests, anchors and verifiers are the round model.
D3. What happens to `mockspace-rs` and `mockspace-builtin-lints` (28,190 lines): a branch, kept in the workspace, or dropped; and whether the consumers' lints ever move to viola plugins.
D4. The version label and when. A minor bump to 0.1.0 is what op named earlier.
D5. The amendment of the two tenets that name the trunk, and, if it goes ahead, the amendment of the v2 spec so that it describes what is built.
D6. Where the probe may push, and whether the cloud environment's setup script gets the fetch refspec.
D7. The kaski freeze window for the crates move, and the consumer order.

## Open evidence

v2's tests were not run (classifier refusal, second time; DM sent to console-coord). The canon tool was not run; the cited tenets are from grep. Not read: the console server mode's API against the writes of the static mode; the sizes of the 14 smaller consumers' data (the per-consumer estimate is a flat 0.5 to 1 day without them); whether `url.insteadOf` is honoured by cargo's git fetch in these containers; whether GitHub accepts refs outside `refs/heads` and `refs/tags` through the Git Data API (the console already reads `git/ref/heads/<ref>`, `github.rs:612`). The throughput basis counts all sessions together, so a single helper's rate may be lower.
