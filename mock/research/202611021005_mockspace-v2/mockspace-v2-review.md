Source: kaski channel (coord/kaski of clause-dev), research/mockspace-v2-review.md, first posted 2026-11-02 10:05 UTC, read 2026-11-15.

# mockspace v2 review: where the workflow stands and what to do with v2

Job `mockspace-v2-review`, 2 November 2026. Written for op. No code in any repository was changed, and nothing was pushed to a product repository.

## Framing

Op asked two connected things on the morning of 2 November: whether the workflow (homma and mockspace) is converging on the old mockspace v2 design by accident, and whether the structured data (registry, tickets, plan rows) should move off the main tree onto git refs to stop the landing churn. The answer wanted is a proposal: commit to v1 and postpone v2, or do the minor migration to the v2 shape already.

This note names which codebase it means every time. "v1" is the live mockspace: `src/`, `cargo-mock/`, `lib/` on the `dev` trunk of `hiisi-digital/mockspace`, version 0.0.2, 77,502 lines of Rust (measured with `find src cargo-mock/src -name '*.rs' | xargs cat | wc -l` on `dev` at `0faa72c`). "v2" is the redesign under `mockspace/mock/`: the spec at `mock/research/202605181400_mockspace-v2-spec.md` and eight crates, 49,706 lines (same method over `mock/crates`). The `main` branch of mockspace is 84 commits behind `dev`; a first read of `main` understated what v1 has become, so everything below is read from `dev`.

Both of op's claims are treated as hypotheses. The convergence claim is partly true, and the part that is false matters for the decision. The git refs idea is sound for one class of data and wrong for another. The proposal at the end composes the two.

Method. Sources opened: the v2 spec, the v1 parity audit, the migration guides, the Aug 2026 design rounds, v1 `src/registry*`, `src/suite/`, `src/bootstrap/*_driver.rs`, homma `mock/DESIGN.md.tmpl` and `homma-store`, kaski `mock/registry/ticket/`, `.gitattributes` and `mock/tools/tickets`, the planning schema panel brief, the agenda rows and the four captures of 2 November. Counts were taken from `git log` on kaski `origin/dev` with deep history (6,271 commits, 28 September to 2 November). Things I could not test from this container are marked as untested.

## Where things stand

### homma

homma orchestrates a multi-repository workspace and is deliberately small (`homma/mock/DESIGN.md.tmpl:1-5`). It has four primitives: a record store, a daemon, a registry of identities and a generation pass (`DESIGN.md.tmpl:30-50`). The store is "newline-delimited records, one per line, in files namespaced by kind" (`homma-store/src/lib.rs:8`), and the file says outright that whether correspondence and structural work share one engine "is not settled" (`lib.rs:10-13`). homma has no git-ref storage anywhere in its design or its store; the only `refs/` uses in its crates are repository operations (`homma-core/src/repo/ops.rs`, `gix_impl.rs`).

homma runs its own work through mockspace v1: its `WORKFLOW.md.tmpl` describes five phases (TOPIC, DOC, DRAFT, IMPL, CLOSED) derived from which flat files exist in `mock/design_rounds/`.

homma is therefore not a competitor to v2's storage. What the workspace tools do today (`msg`, `ticket`, `land-own`) are shell scripts and kaski tools that sit beside homma, and several of them already put data off the main tree. That is the precedent in section 4.

### mockspace v1, as it runs on dev

v1 is a gate and a generator. Its pieces, each with a place in `src/`:

- design rounds as flat files, phase derived from filenames (`src/design_round/`), with phase gates on commit;
- the typed row registry under `<mock>/registry/`, "designed for v2, prototyped here in v1" (`src/registry/mod.rs:52`), with slug identity, `reg::ns::slug` references and checks;
- lints and the lint pack, bench framework, sketch and research directories, the agent-rule render, `cargo mock tools`;
- since the August rounds, a large body of machinery that v2 never planned: merge drivers by file kind (`src/bootstrap/registry_driver.rs`, `round_driver.rs`, `lockfile_driver.rs`, `history_driver.rs`, `append_driver.rs`), `mock merge-registry` merging registry TOML by namespace and id (`src/registry_merge/mod.rs:6-22`), `mock registry-append` as the one writer of an answer or note into a row (`src/registry_append/mod.rs:6-12`), the test-selection planner and test history (`src/suite/`, 28 files), shook hook chains, and the single-source and incremental check work under `mock/research/2026103*`.

It uses no mock-side git refs. Its only uses of `refs/` are tags in `design_round/rename.rs:270-274`, a branch name in `plumbing.rs:160`, and an `ls-remote` in `pack_pin.rs:253`.

v1 itself has no `mockspace.toml` and does not gate itself, which is deliberate and not a finding (rule `mockspace-is-v1-and-v2`). Consumers (kaski, homma and others) carry the config and run v1.

### mockspace v2, as it stands

v2 is the redesign, and it does carry the workflow: it has design rounds under `mock/design_rounds/`. Its spec is self-contained and large (4,387 lines). Its distinctive decisions:

- Part III, state on git refs. Mockspace state lives on `refs/mock/*`, orphan and flat: `refs/mock/round/<slug>`, `refs/mock/task/<ns-path>/<slug>`, `refs/mock/harness` for configuration, archives for closed rounds and tasks (spec lines 1498-1521). The reason given is clutter, tag pollution and breaking the coupling between the active round and the checked-out branch (lines 1523-1531).
- A `.mock/` rendered surface and a local `.git/mockspace/` cache (lines 1545-1612). No `refs/mock/index`; refs are enumerated directly (lines 1533-1543).
- Six phases (TOPIC, PLAN.DOC, APPLY.DOC, PLAN.SRC, APPLY.SRC, DONE) with content-addressed anchors and sealed manifests with a closed verifier catalogue (lines 1072, 1721-1790).
- Tasks as work items with slug identity and namespaces, a five-state machine (open, in-progress, blocked, deferred, closed), steps and close metadata (lines 1214-1360).
- Imports, exports, a `mock://` URI scheme, signing with per-developer trust, a lockfile and an optional transparency log (Part IV, lines 1983-2550).
- Hooks, profiles, env and bin policy; undo and redo; `mock doctor`; a PR lifecycle that auto-opens and manages a forge PR (Parts V and VI, line 3582).
- A migration path from the filesystem layout: `mock migrate` with journal, dry run, rollback and finalize, additive until finalize (lines 3713-3766).

The implementation covers the ref storage layer and the round and task lifecycle, and stops short of the rest of the product. `mockspace-core` (13,355 lines) holds the phase, slug, ref path, anchor, task, round and transition model and the ref I/O: advancing a phase (`io/advance.rs`, 751 lines), the transition lock (`io/lock.rs`), ref writes (`io/ref_write.rs`), sealing, archiving a closed round (`io/archive.rs`) and tasks (`io/task.rs`, 1,314 lines). The comment at `mockspace-core/src/lib.rs` that calls transition and atomicity "contract surface; impl in Phase 5" is stale; the implementing files exist. `mockspace-rs` (24,814 lines) is the lint engine and presets. The CLI crate (1,702 lines) has verbs `status`, `install`, `uninstall`, `refresh`, `explain`, `check`, `phase` (plan, apply, finish, replan), `close`, `task` (new, list, show, start, block, defer and more), `regenerate` and `migrate` (`mockspace-cli/src/main.rs:99-220`, dispatch at 490-540), with golden end-to-end tests for `phase`, `task`, `close`, the whole `refs/mock/*` tree after a lifecycle, `migrate`, `check`, `explain` and `install` (`mockspace-cli/tests/e2e_*.rs`; 353 `#[test]` markers across core and CLI tests). `migrate` is a guide: it walks `mock/design_rounds/` and prints a per-round plan and a checklist (`tests/e2e_migrate.rs:1-9`, `main.rs` `run_migrate`); it moves nothing, unlike the `mock migrate harness|rounds|tasks|all|finalize` of the spec (lines 3717-3731). There is no verb that creates a round (`tests/e2e_phase.rs:1-8` says so; `grep -c 'round new' main.rs` is 0), so a round on a ref is seeded by library calls only. No source file in the three v2 tool crates mentions signing or trust-on-first-use (a grep for `tofu` and `signing_key` finds 0 files; the same kind of search for `Task` finds 8 occurrences in `main.rs`, so the search works). I did not run v2's tests: the step was refused (see the open questions), so what v2 passes is not claimed here. The last commit to `mock/crates` on `dev` is 10 October 2026 and the last design round is 24 August 2026 (`git log -1 -- mock/crates`, `-- mock/design_rounds`). The consumer-facing migration guides cover lints and agent files only (`docs/MIGRATION-v1-to-v2-lints.md`, `-agent.md`); nothing migrates a consumer to refs.

The standing direction is that v2 replaces v1, and the record shows the opposite happening. The registry design note of 18 July says v1 "is at the end of its road and is marked for deprecation once v2 is ready to replace it", that features added to v1 "are deliberate backports of v2 features, built early so they can be test-driven", and that the model is what v2 inherits (`docs/research/202607181700_registry-of-terms-design.md:13-20`). The root `README.md:136` still describes the workflow redesign as "a successor model ... in development". Since 18 July, `dev` has 296 commits (history fetched back to 10 March, so the window is covered): 195 non-merge commits touch v1 (`src/`, `cargo-mock/`) and 10 touch v2 (`mock/crates/`). v1 grew from 34,896 lines on `main` (31 August) to 77,502 on `dev` (2 November). So the choice op asked about is also a choice about whether that stated direction stands: committing to v1 reverses it, and migrating to v2 follows it. There are two competing task designs in the same repository as well: the v2 spec keeps tasks on refs (spec lines 1214-1360), while `docs/research/TASKS_BRANCHES_PHASES_EPOCHS_DESIGN.md` (22 May, "proposed") keeps open tasks as one Markdown file each under `mock/tasks/` in the tree (lines 384-396). The spec says that where it disagrees with anything else in the repository it wins (spec line 7), but it does not mention the other design.

The parity audit of 22 May names where v2 trailed v1 even then: a bench findings generator, custom lint surface, sketch convention (`mock/research/202605222330_v1-parity-audit.md:20-90`). Part of the multi-active round mode is "deferred until consumer demand warrants" (spec line 4287), and the bench slice was "blocked on consumer demand" (audit lines 22-30). No consumer runs v2.

### The de facto extensions

What actually runs, and is growing daily, is kaski's use of v1 as a platform:

- `mock/registry/` with a ticket namespace (1,363 ticket files in `mock/registry/ticket/items/` at the dev tip I read, one file per ticket holding its row and its moves), `kinds.toml`, `states.toml`, `epochs.toml` (four rows with `id`, `title`, `what`), canon rows (`tenet/`, `ikiuni/`, `subject.toml`) and intake rows.
- `mock/tools/tickets` (the `kaski-tickets` rules `entry`, `check_move`, `check_needs`, `check_epoch`, `check_branch`, `check_store`), the console that reads the registry from GitHub and writes changes to `dev`, and the shell scripts under `.shared/scripts/remote/` (`ticket`, `land-own`, `land-core.sh`) as the writers.
- The channel for coordination, `msg`, whose storage is a dedicated branch per channel: `coord/kaski` is the `kaski` channel (`tools/msg/README.md:3-5`), and "nobody checks out, pulls or pushes a coord branch". It is the one off-tree data store already in daily use.
- The planning schema work in flight. The branch `work/console-planning-schema` is pushed at `557300806` and so far holds only the panel brief (`mock/research/202611020947_planning-schema/00_brief.md`), and at its tip that is all it holds; the strawman in the 2 November captures (a `plan` namespace with epoch, milestone, stretch and change rows) is not on it.

This is the convergence op saw. The next section tests it row by row.

## Concepts compared

The columns are v2's position (spec), today's plan (the four captures, the agenda rows and the planning brief), and the gap. "Today" includes v1 as extended.

| Concept | v2 | Today (v1 plus plan) | Gap |
|---|---|---|---|
| Design rounds and phases | Six phases, sealed manifests, anchors, restore on replan (spec 1072, 1721) | Five phases from flat files, gates on commit (`homma/mock/WORKFLOW.md.tmpl`); a round can carry a merge since 24 August | Equivalent in intent, different in mechanism. v2 stores the round on a ref, v1 in the tree. Anchors and replan have no v1 counterpart. |
| Registry and its schema | The v2 spec and the v2 crates have no typed row registry; "registry" in the spec means the package registry and the hook event registry (lines 512, 2611, 2521). A separate design note says it is "designed for mockspace v2, where it lands as a first-class feature" and prototyped in v1 as a deliberate backport (`docs/research/202607181700_registry-of-terms-design.md:4-6`, `docs/REFERENCE-SYNTAX.md`) | v1 has it (`src/registry/`, about 7,400 lines with its subdirectory), kaski uses it for 1,363 tickets, canon and intake; the plan adds `mockspace-registry` as a bare crate (agenda row `mockspace-takes-the-registry-tickets-and-project-crates`) | The registry is designed as v2's and built only in v1. v2's code has nothing for it, and the kaski extensions (ticket moves, holders, needs, merge by row) go well past the July design note. |
| Canon | Not a v2 concept | Typed rows declared by `canon_paths` in `mockspace.toml`; ratified by op | None to close. Canon belongs in the tree (section 4). |
| Tickets and tasks | A task is a ref, slug identity, namespaces, five states, steps, close block (1214-1360) | Ticket is a file with moves; kinds task, job, question; seven states with holders and needs; rank, part_of, needs, epoch | Identity and the state-machine idea converge. Question kind, holders, needs, rank, moves-as-history and part_of have no v2 counterpart. Taking v2 tasks literally would drop them. |
| Change units and grouping | A round pairs a mock-side ref with a source-side branch by slug (1654-1678) and projects a forge PR (3582) | Plan: a change as the pull-request analogue on own refs and registry, forge PRs and issues dropped (capture 202611020928) | Close in shape. v2 still projects a forge PR; today's plan removes the forge from the loop. The change row (ticket to change to epoch) is new and v2 has no epoch above a task. |
| Epochs, milestones, stretches | The v2 spec never uses the word. An older design note, which the root `README.md:136` still calls the successor model, defines an epoch as "an archival boundary that compacts closed workflow history into a historical package" (`docs/research/TASKS_BRANCHES_PHASES_EPOCHS_DESIGN.md:183-186`, 905-960) | Planning schema panel in flight; an epoch there is a body of work (four epoch rows exist, 180 of 1,363 tickets name one) | New ground for planning, and a word collision: the same word means history compaction in mockspace's own design and a body of work in kaski's. Settle one meaning before `mockspace-projectmanagement` upstreams it. |
| Where structured data lives and how it merges | `refs/mock/*`, orphan flat refs, merged by the atomicity contract (1498, 1787) | In the main tree, merged by drivers by file kind (`.gitattributes:2-19` in kaski) | The largest real divergence and the subject of section 4. |
| Lints and tools | v2 lint engine, directives, a closed verifier catalogue; tools directory reserved, unspecified (1694) | v1 lints and tools in `<mock>/lints` and `<mock>/tools`; kaski has about twenty tools (`mock/tools/`) | v1 is ahead on tools. v2's lint model is the one the lint pack is moving toward (`docs/MIGRATION-v1-to-v2-lints.md`), so this converges on its own. |
| Agent rules | Static fragments embedded in the binary, no consumer override (`docs/MIGRATION-v1-to-v2-agent.md`) | v1 renders rules; workspace rules aggregate through homma | Both ship; the workspace goes its own way through homma. Little to decide. |
| Project management | Absent | Planned: `mockspace-projectmanagement` (epochs, features) and `mockspace-tickets` as optional crates; a clause-dev `mock/` registry replacing the agenda (held by op as a thought, not a decision) | New ground in both. |
| Test selection and history | Not in the spec | v1 `src/suite/` planner, test history merge driver, `cargo mock test` runs what moved | v1 only. |
| Imports, signing, transparency log, `mock://` | Specified in full (Part IV) | Not wanted by anything in the plan | v2 has more than the workspace needs. |
| Undo and redo, `mock doctor` | Specified (Part VI) | Not planned | Same. |

Is the convergence real? Three parts are, and they matter. First, a work item with a slug identity, a namespace hierarchy and a lifecycle is v2 task and kaski ticket alike. Second, the idea of a change unit paired with source work and kept off a forge matches v2 section 21. Third, coordination data on a dedicated branch is v2's off-tree storage principle in practice; the `msg` channel is a working instance. Two parts are not: v2 has no typed registry and no epoch, milestone or stretch, so the planning work is not converging on v2, it is building the part v2 left out; and v2's remaining weight (imports, signing, `mock://`, harness ref, `.mock/` materialisation) is not on any plan.

In short, today's work is converging on one slice of v2, the slice that is a storage decision, and is leaving the rest.

## The git refs question

### What the churn is

Measured on kaski `origin/dev` with deep history (script kept in the scratchpad of this job; command shape `git log origin/dev --since=<d> --no-merges` with `git show --name-only` per commit):

| window | non-merge commits | touching only `mock/registry/` | subjects starting `tickets:` | merge commits |
|---|---|---|---|---|
| 19 Oct to 2 Nov (first commit in it is 22 Oct) | 5,004 | 1,662 (33%) | 1,534 | 939 |
| 31 Oct to 2 Nov | 1,382 | 897 (65%) | 820 | 223 |

Another 131 commits since 19 October touch the registry together with other paths (1,793 touch it in all). 1,707 commits touch `mock/registry/ticket/`. The share of data-only commits has doubled in the last two days, so the pressure is rising with the number of helpers.

The ticket named in the brief, `a_docs_only_landing_on_kaski_dev_outlives_a_30_minute`, is no longer in kaski's registry at `efce62b64`: it was moved out and now sits in the workspace tickets lane (`.shared/scripts/life/tickets-workspace.toml:69-75`, source `kaski@f1a2e3cc7f40`). My first search of kaski's registry found nothing and I reported it as absent; the control on that search worked (`milestone_2_fixes` found in 34 files) but the question was the wrong one, since a ticket can leave the registry it was filed in. Its text is the best direct evidence there is for the mechanism. A `land-branch --gated` of two research notes and no source ran from 20:20 to 20:50 UTC on 1 November and was stopped before it reached `dev`, because "the commit gate runs the whole lint pack on the merge commit and dev moved under it"; a bump on it adds that console-coord's console-only branch `feat/file-popup` "lost the push race twice, its gate running near an hour while dev moved under it". So the landing refusals are two things together: the gate on the merge commit is long and not scoped to what the change touches, and `dev` moves under it all the time. Data on a ref only addresses the second. It does not shorten the gate, and the note's step list now carries a separate step for that.

The pace of `dev` is the second half of the evidence. Commits per day on kaski `dev` (`git log origin/dev --format=%ad --date=short | sort | uniq -c`, the clone's history is complete from the first commit on 28 September): 4 to 14 a day in mid October, 458 on 24 October, 1,128 on 28 October, 1,144 on 30 October, 1,140 on 31 October, 430 on 1 November and 679 on 2 November up to 10:00. No commits fall between 19 and 21 October, so the first window below starts in practice on 22 October. I also cross-checked the registry-only counts with a path filter (`git log --since=... --no-merges -- . ':!mock/registry'`) and got 1,672 and 892 against the 1,662 and 897 of the first method, a difference under one percent that comes from a few commits landing between the two runs.

### What is already solved

Text conflicts on the data are mostly solved already. kaski's `.gitattributes:5` routes `mock/registry/**/*.toml` through `mockspace-registry`, which merges by namespace and id (`src/registry_merge/mod.rs:6-22`, covered by `kaski/mock/tools/tickets/tests/merge.rs:1-9`, which includes the control without the driver). Append-only files, rounds, test history and generated documents have drivers (`.gitattributes:2-19`).

What the drivers do not remove is contention on one ref. Two writers pushing to `dev` race on the ref update whatever the content, so one has to fetch, merge and try again. The merge is cheap when the driver resolves it, but the retry runs the commit gate on the merged tree, and the gate is the expensive step. A data-only commit does not need the code gate, but it pays it because it shares a ref with code.

### The two classes of data

Not all of `mock/` is the same kind of thing.

Operational data: tickets and their moves, epochs, milestones, plan and change rows, the channel, test history, intake rows. Written by many writers, many times a day, mostly by tools, append-heavy, row-keyed, read by tools and the console. Nobody reviews these as code, and they do not need to be in the same commit as a code change to be correct.

Governing data: canon rows (`tenet/`, `ikiuni/`, `subject.toml`), design rounds, `DESIGN.md.tmpl` and the other templates, lints. Written rarely, reviewed, ratified by op, and bound to the code by the chain canon, design, code. A design round and the code it governs must change together, and `canon_paths` and the gates read them from the working tree. Moving these off the tree breaks the mutation order the rules depend on.

The v2 spec moves both classes to refs (rounds, harness, tasks all under `refs/mock/*`). The kaski experience says only the first class has the churn and only the first class gains from leaving.

### What would live on which ref

A proposal, not a settled shape:

| Data | Today | On a ref | Reason |
|---|---|---|---|
| tickets and moves | `mock/registry/ticket/items/*.toml` | yes | 1,707 commits in two weeks, written by tools |
| epochs, milestones, stretches, changes (planning schema) | `epochs.toml`, to be designed | yes, from the start | not migrated yet, so no second migration |
| channel mail | branch `coord/<name>` in clause-dev | stays, already off tree | the precedent |
| intake rows | `mock/registry/intake/` | yes, later | same tool shape |
| test history | `mock/test-history/` with a merge driver | candidate, later | recorded by tools, has its own driver |
| canon and tenets | `mock/registry/tenet/`, `ikiuni/`, `subject.toml` | no | governs designs, reviewed, read by `canon_paths` |
| design rounds, templates, lints | `mock/design_rounds/`, `*.tmpl` | no | bound to the code in the same commit |
| clause-dev workspace registry (agenda replacement) | `.shared/scripts/life/*.toml` | yes if built | same class as tickets |

### Ref granularity

This is the choice that decides most of the cost, and v2 got it wrong for this load. The spec makes one orphan ref per task and says "workspaces typically have hundreds of refs, not millions" (line 1541). kaski has 1,359 tickets today and gains tens a day, so one ref per ticket gives well over a thousand refs in one repository that already carries 660 branches and 113 pull refs (`git ls-remote https://github.com/orgrinrt/kaski`). The spec's decision against an index ref (lines 1535-1541) is made for hundreds of tasks, and a query over 1,359 tickets that lists refs and reads each is a different cost. Options:

1. One ref per ticket (the spec). No cross-ticket race at all. Atomic batches across tickets are impossible. A query reads one blob per ticket, so the console and the lints need a local index (the spec has `.git/mockspace/index.bin`) and a way to build it from a fetch of 1,359 refs.
2. One ref per namespace (for example `refs/mock/registry/ticket`). One tree holds every ticket file, so a read is one fetch and a batch is one commit. The ref is still a single point of contention, but only among data writers, and a retry is a row-level merge with no code gate. The busiest hour measured held 66 registry-only commits (31 October, 18:00 hour; the next are 61, 53 and 42), about one write a minute, which one ref handles with retries.
3. One ref per shard, for example by epoch or by month of creation. Contention divides by the shard count, at the price of a shard rule everyone must know.

Option 2 is the cheapest step and loses nothing the current tools rely on (`ticket batch` is one commit, the console writes whole-registry edits). Option 1 is the endpoint v2 describes and only pays off if per-ticket contention is the actual problem. The measurements do not show that: contention is between code landings and data pushes, and option 2 separates those already.

### How reads, writes, the console, lints and merges reach it

- Reads. Tools read a ref's tree with `git cat-file` and `git ls-tree` plumbing, as the spec does (line 1650), or a local checkout of it as a worktree. `mockspace-core/src/io/ref_tree.rs` already reads a tree from a ref. The registry loader must take a source (a directory or a ref); today it walks `<mock>/registry/` (`src/registry/load.rs`).
- Writes. A write builds a commit on the ref's tip and updates it with a compare-and-swap; on loss it fetches, applies `mock merge-registry`'s row merge in memory, and retries. The merge code exists (`src/registry_merge/merge.rs`) and works on text, so it is reused as a library and no new merge model is invented. `mockspace-core/src/io/ref_write.rs` and the lock in `io/lock.rs` are the ref half.
- The console. It reads the registry from GitHub and writes changes onto `dev` today. Reading a non-branch ref through the contents API and the Git Data API is untested here; it needs a probe before a decision (step 0 below). A fallback is a branch namespace such as `data/registry`, which behaves like `coord/*`, works with every tool that already knows branches, and is visible in the forge. It is the safer first form of the same idea.
- Lints. Registry lints that run on commit read the tree. The data-ref gate becomes its own, small: the registry checks of `kaski-tickets` plus the schema checks, without the code gate. This is the gain.
- Merge drivers. A ref update does not go through `.gitattributes`. The same row merge is called by the writer in the retry loop. The drivers stay for the governing data that remains in the tree.
- Hooks and shallow clones. Cloud containers clone shallow through a proxy, and a default clone does not fetch `refs/mock/*` (the refspec is `refs/heads/*`). Bootstrap must add a fetch refspec. That the proxy serves such refs is untested (I attached repositories read only and could not push). The `msg` channel proves branch refs work through it.

### What it costs

- A second place to look. `grep` over the tree no longer finds a ticket; tools must. This workspace already says tools are asked before grepping, and `cargo mock tickets list` exists, so the cost is lower here than in an ordinary repository.
- No review of operational data in a pull request. Tickets are not reviewed today either; they land by tool.
- Audit trail. The ref has its own history, and the spec adds commit trailers (line 3647), so provenance survives; it is no longer in the same `git log` as the code.
- Clones and new containers need the extra refspec and a fetch, and a failed fetch must be loud, not an empty registry (rule `never-discard-stderr-on-a-check` applies to that code).
- Atomicity across the boundary. `land-own` pushes the branch into `dev` and then moves the job's ticket and tells the channel (`session-worker.md`, the `land-own` entry), so a crash between the two is possible today and the ticket-move commits on `dev` seen above are the result. Moving tickets to a ref does not add that class of failure, but it removes the one thing that hides it, which is that both writes land on the same ref in order. The recovery rule (the next landing check reconciles a landed branch with an unmoved ticket) should be written down before the move, and it is the main design risk. I did not read `land-core.sh` itself to confirm the order of its writes.
- Spec amendments. Granularity (above) and the "no index" decision need revisiting for this scale, and the task state machine needs the extra states and fields kaski tickets use.

### What it would remove, measured

The 1,662 data-only commits of the last two weeks (33% of non-merge commits on `dev`, 65% in the last two days) would not land on `dev` at all. Of the 939 merge commits in the window only 49 have a `tickets:` subject (359 are plain `Merge ...` lines and 531 carry other subjects), so the merges are mostly code landings and would stay. The non-merge history of `dev` would shrink by a third over the window and by about two thirds at the recent rate, and landing code would no longer race data pushes. That is the effect that can be claimed from the log. I have not measured the retry and gate time saved per landing, because no log of those exists; a one-week log of landing attempts (outcome, time to land, refusals) should be taken before and after the first move so the win is a measurement and not an estimate.

## Proposal

### The options chosen among

Option A. Commit to v1 and postpone v2. Keep operational data in the tree, extend v1 with the registry, tickets and project crates as planned, add nothing from v2. Cheapest in work. It also reverses the July statement that v1 is to be deprecated for v2, and would need that statement withdrawn in writing, or v2's spec and README would keep describing a plan nobody follows. Leaves the churn, which is rising, and the agenda replacement and planning schema would all be built on the surface that causes it.

Option B. Migrate to the v2 shape now as a minor bump. Take the spec as written: refs for rounds, tasks and harness, `.mock/` materialisation, six phases, `mock migrate`. This is large and v2 does not yet do what v1 does. It has the ref storage, the phase transitions, task lifecycle and archive, with golden end-to-end tests, which is further along than a first read of its CLI header suggests. It has no registry of typed rows, no test planner, no merge drivers, no tickets with holders and no verb to create a round, and `mock migrate` only prints a plan. The migration guides cover lints and agent files only. The bump itself is not the issue (v1 is 0.0.2, v2 crates are 0.1.0, and a minor bump 0.0.2 to 0.1.0 is a fair label for either); the issue is that taking all of v2 moves every consumer's rounds, canon and templates off the tree for no gain, since those do not churn.

Option C. A composition. v1 stays the runtime for rounds, lints, canon, tools and tests. One v2 idea, operational data on refs, is adopted now and built on v2's ref primitives. Everything else in v2 stays postponed until a consumer needs it.

Option C is the recommendation. It is also the practice the July note itself describes, backporting a v2 idea into v1 early so it is tested on real repositories, applied to the one v2 idea that has a measured use; what it changes is that v1 stops being described as the thing to be deprecated soon. It follows what the data says: the churn is in one class of data, the convergence is in one slice of v2, and the planning schema is about to create new operational data that should not be built in the tree and migrated later.

### Order of steps

0. A probe, one session, no design: create a `refs/mock/registry/probe` ref (and a branch `data/probe` as the fallback) in a scratch repository and in kaski, and check push and fetch through a cloud container's proxy, a shallow clone with an added refspec, `ls-remote`, and a read of both through the GitHub contents and Git Data APIs that the console uses. If the namespaced ref fails any of these, step 1 uses the branch namespace and nothing else changes. Without this probe every later step rests on three untested claims. Op's call: whether the fallback to branches is acceptable.
1. Tell the planning schema panel now that the rows may live on a ref, so the schema stays backend-neutral and the data-side helper does not hard-code a path. This costs one message and avoids a second migration.
2. Put a source abstraction under the registry loader in v1: a directory or a ref, same rows. Add the write path (CAS, row merge on loss) reusing `registry_merge` and the ref I/O of `mockspace-core` (`io/ref_tree.rs`, `io/ref_write.rs`, `io/lock.rs`). Those live in the v2 workspace under `mock/`, which v1 does not depend on today (v1 uses `git` by command), so this step decides whether v1 links the v2 core crate or takes the few hundred lines it needs; that choice is part of the step. This is the `mockspace-registry` crate op already put on the backlog, now with a reason to be built earlier. No opinions in it, as op said.
3. Move the kaski ticket namespace with the planning rows, in one ref per namespace (granularity option 2). Adapt `kaski-tickets`, the console reads, and the `ticket`, `land-own` and `job-brief` scripts. Define the landing reconciliation rule first. Run the before and after landing log.
3b. In parallel with 2 and 3, scope the commit gate by what a change touches, so a landing of research notes does not run the whole lint pack on the merge commit. The ticket `a_docs_only_landing_on_kaski_dev_outlives_a_30_minute` asks for exactly this, and the incremental-check work under `mock/research/202610300900_incremental-checks/` in mockspace v1 and the planner of ticket `mockspace_change_the_planner_settles_by_test_scopes_the` are the places it would land. Moving data off `dev` makes `dev` move less, and a shorter gate makes each race cost less; neither replaces the other.
4. Move intake and test history if step 3 shows the win. Build the clause-dev workspace registry on the same machinery if op decides to replace the agenda.
5. Revisit v2 in its entirety after steps 2 to 4 have run for a month. What is left in the spec is then a smaller question: tasks as refs (if option 1 is ever wanted), the six phases, the harness ref. Each of those would need its own consumer.

Version. Steps 2 and 3 add a public feature and a storage mode to v1, so they are a minor bump (0.0.2 to 0.1.0), as op suggested. Nothing in this order is a major change, because the default storage stays the tree and a consumer who changes nothing sees nothing.

### Risks

- The refs probe fails and the fallback to branches loses the "invisible in routine git operations" property the spec wanted. The branch form still removes the contention, which is the goal; it is only visible in the forge's branch list.
- The landing reconciliation is subtle (above). Getting it wrong yields tickets out of step with landed work, and the whole workflow trusts ticket state.
- The console is read-through GitHub today, and any API limit on non-branch refs falls on it.
- Two stores means two sources of truth during the move. Moving the ticket namespace in one cut, with the old files deleted in the same landing, avoids a long overlap.
- Spec debt. Taking one idea from the spec while the rest stays postponed leaves a spec that no longer describes what is built. It should be amended to mark the storage part as adopted and the rest as deferred, or the next reader makes the mistake this review made at the start (reading `main` and v2 as the current state).
- Cost of waiting. The churn is rising and so is the number of helpers writing to `dev`. Every week the data stays in the tree, more tickets and plan rows are built against it.

### What stays op's call

- Whether the data-ref form may fall back to a branch namespace if the probe fails.
- Granularity: one ref per namespace as proposed, or the spec's one ref per task.
- Whether v1 is still marked for deprecation. The July note and the README say yes; the commit record since then says v1 is where the work is. Under option C v1 stays the runtime for the foreseeable future and the note, README and spec should say so.
- Which task design stands, refs (the spec) or Markdown files in the tree (the older design), and which meaning `epoch` has.
- Whether canon and design stay in the tree. The recommendation is yes, because the chain canon, design, code depends on it; the choice is his.
- Whether the clause-dev `mock/` registry that replaces the agenda goes ahead (the agenda row `clause-dev-has-a-mock-registry-in-place-of-the-agenda` records it as a thought, not a decision).
- Whether the epoch, milestone and change model is built into mockspace as optional crates (`mockspace-projectmanagement`, `mockspace-tickets`) now or left in kaski until proven.
- Whether any of imports, signing, the transparency log, `.mock/` materialisation or undo and redo is ever wanted. Nothing in today's plan asks for them.
- The release: a minor bump label and when.

### Open questions this review could not close

- Whether GitHub and the cloud proxy serve `refs/mock/*` (untested, step 0).
- The retry and gate time per landing (no log; step 3 adds one).
- v2's tests were not run. I tried `cargo test -p mockspace-cli --test e2e_phase ...` in a detached worktree and the auto-mode classifier refused it as code from an external repository. What v2 implements is read from its crates, its tests' source and a count of `#[test]` markers (353), and what v2 passes is not claimed. The refused step is not ticketed: this job pushes nothing to kaski or clause-dev, so the coordinator files it.
- What the canon says about off-tree data, epochs and milestones was not asked of the canon tool. `cargo mock canon find` was never attempted in this job, because it builds and runs kaski's code and the same refusal would apply to it; the only statement of the canon's position here is the planning brief's own line that `cargo mock canon find epoch|milestone|stretch` is silent, which is a second-hand claim and not verified. The note makes no claim about what the canon says.
- The cost of scoping the commit gate by what a change touches was not estimated. Evidence that it is the other half of the landing problem is the ticket text quoted above; the work on it (`mock/research/202610300900_incremental-checks/` in mockspace v1, the test-selection planner in `src/suite/`) is not reviewed in this note beyond its existence.
- I did not read `land-core.sh` or the console's write path. That the console writes to `dev` comes from the planning brief; kaski also holds six `console-changes/*` branches and thirteen `q/*` branches, which suggests console edits and questions already travel on branches, and I did not check how they are used.
