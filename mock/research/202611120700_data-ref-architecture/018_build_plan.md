# Mock trees: the build plan

Status: a plan, not a design. It cuts the canon candidate (`017_consolidation.md` and `017_consolidation_migration_and_tests.md`, as amended by the last section of `000_op.md`) into design rounds, and orders them. It opens no round and changes no code. 017 is unratified, so each round's topic file is where its part of 017 gets read against op's words once more before anything is designed from it.

## What governs

Op's words in `000_op.md` govern, and 017 under them. The last section of 000 ("Answers to the final consolidation") amends 017 in three places, and this plan uses the amended form everywhere:

- The shadow tree is named `+` in the ref, the directory and the address: `refs/mock/+`, `.mock/+`, `mock://+/crates/foo` (`000_op.md:172`). Every `<shadow>` in 017 reads `+`. Every refspec the engine writes is fully qualified, since a leading `+` is the force flag (`000_op.md:174`).
- A purge must be signed by a key declared in the git tree (`000_op.md:171`). This is 017's Q-C, option b.
- A second remote is off by default and available where declared (`000_op.md:150`). This is 017's Q-A.

Two standing demands shape every round. The first is op's bar (`000_op.md:7`, `:9`): unit tests alone are not enough, the work needs "proper full migrations with mock data, and actual e2e scenarios emulated of using the mocksoace so no data loss", and feat/v2 reaches dev only when everything is ready to port. The second is the intent at `000_op.md:69`: no data loss, machinery that cannot be misused into losing data, a recovery cache under every overwriting step, and behaviour as close to git's own as possible.

## What the shipped code is

The shipped code on `origin/feat/v2` is a set of claims, and several of them are now wrong by design. The code holds data refs as files by pattern in a sparse checkout of the code worktree, hashes them itself with `hash-object --no-filters`, and keeps pending copies and backups in its own records. 017's L1 replaces that with a native linked worktree per mock tree that git merges, commits and filters by itself. That is a design change, so the code under the old design is deleted and written again from the new design, and nothing of it is migrated.

Where the code lives is open as well. On feat/v2 the data-ref verbs sit in the root engine (`src/data_refs.rs`, `src/data_refs/`), the git plumbing in `mock/crates/mockspace-store`, the stores in `mock/crates/mockspace-registry`, and the names in `mock/crates/mockspace-refnames`. The plan proposes that the mock-tree plumbing stays in `mockspace-store`, the verbs move to `mockspace-cli`, and the root `src/data_refs*` goes. Round 2's topic file must put that locus to a second reader before DOC locks, because one plan is one reading.

## How the rounds are cut

Each round is one feature that a person can see working at its end, across every crate it reaches. The cut follows the seams 017 itself has: names and refs (4.1, glossary), the instance and its verbs (4.2, 4.4, 4.5), the hooks (4.7), the remote (4.6), the shadow tree (4.3), the migrations (4.8, 4.9), and last the acceptance suite that op's bar asks for. Every round writes its red tests first, drawn from 4.10, and a test that cannot compile yet is committed as it is.

The tests of each round use the same world: a fixture that builds a remote, clones of each shape of 4.10 (plain, opted-in, linked, bare+linked with each refspec style, mirror, each crossed with placement `tree` and `external`), and mock data of realistic size and kind (research notes with relative links, a registry with rows, templates with computed lines, an LFS-filtered asset). Round 1 builds that fixture in `mockspace-test-fixtures`, and every later round adds its verbs to it. The shipped `tests/refs_e2e/` world is read for what it protects and then deleted with the code it drives.

## The order, in one paragraph

The work outside mockspace goes first, because two of its pieces are gates: the git-shook stdin fix gates auto-install, and letkajenkka's address form and case setting gate every name the engine writes. Then names, then the instance, then the hooks, then the remote, then the shadow tree, then the migrations, then acceptance. Names unblock every other round. The instance unblocks four. The remote round and the shadow round do not depend on each other; the remote goes first because no consumer data may be published before the guard exists. Migration is last of the building rounds because it needs every target to exist, and it is the one-way step.

## Rounds

### Round 1: names, refs and addresses

- Carries: 017 glossary, 4.1, L10, L11 (the name only), L13, L22, L28, L29, D9; op at `000_op.md:108` to `:111` (no literal `mock/` directory, `mock://<ns>`, git tree and mock trees as the terms), `:123` (registry off `refs/heads/`), `:140`, `:172`, `:174`, `:65` and `:79` (the letkajenkka spec, with case as a setting).
- Feature: one place that knows a mock tree's kind (`data`, `shadow`, `registry`, `history`), its namespace, its three refs (published, mirror, working), its snapshot ref, its remote `mock-<ns>` with the explicit unforced refspec, and the two resolutions of `mock://<ns>/<p>`. Every address is parsed by letkajenkka and never handed to git. The instance root is read from config, default `.mock`.
- Crates: `mockspace-refnames`, `mockspace-store` (`refs.rs`), `mockspace-config` (the declaration: kind, placement, remote, instance root), `mockspace-registry` (`stores.rs`, `refpath.rs`, `resolve.rs`, `rowref/`), `mockspace-test-fixtures`.
- Deletes: `RemoteForm::Branch`, `branch_form` and the `+` of `fetch_refspec` in `mockspace-store/src/refs.rs` (017 R.3; the migration that moves existing data is round 6, and the old form is read there by a named migration reader, not by the engine); the hand-split reference readers the ticket `hand_split_references_remain_where_letkajenkka_should_parse` lists; the copied name fold in `mockspace-config` `fold.rs`; `mockspace-registry/src/legacy_refs.rs` as an engine reader; the literal `mock/lints.toml` in `CONFIG_FILES` (`mockspace-refnames/src/lib.rs:26`), which names a `mock/` directory the config is meant to decide.
- Red tests first:
  - no engine-written ref is under `refs/heads/`, `refs/tags/` or `refs/remotes/`, asserted over every ref the fixture world ends with, locally and by `ls-remote`;
  - `a` beside `a/b` is refused at declaration, and so is a registry namespace that prefixes a data one (R.6);
  - `+` round-trips as ref, directory and address, and no refspec the engine writes starts with a bare `+` (asserted on the written git config);
  - a plain clone of the fixture remote fetches no mock object (object count compared);
  - `mock://` is never an argument of a git process (the fixture's git spy);
  - an off-spec reference is refused by the reader, and the case rule follows the `letkajenkka.toml` of the fixture in both settings;
  - a fetch of the mirror ref is refused when it would lower it.
- Tier: opus medium. Names are judgement work with a wide blast radius, and the checks are cheap.

### Round 2: the instance, its verbs and its copies

- Carries: 4.2, 4.4 (all verbs but `regenerate` and `purge`), 4.5 (snapshot and user cache), L1 to L9, L11, L14, L15, L18, L33, D11, R1, R3, R5, R6, R7; op at `000_op.md:27` (shared sparse, copy before any overwriting step), `:35` (ref-held plus cache), `:45` (LFS as on a branch), `:51` (git's own merge machinery), `:59` (cache liberally), `:140`.
- Feature: `instantiate`, `status`, `update`, `commit`, `push` (unforced, without the guard of round 3), `restore`, `retire`, `verify` and `verify --fix`, over a locked linked worktree at `.mock/<ns>` whose HEAD is a symref onto `refs/worktree/mock/<ns>`, with a snapshot before every step and a 24-hour user cache. Placement `external` with its scan, clone id and symlink.
- Crates: `mockspace-store` (the instance, snapshot, cache), `mockspace-cli` (verbs), `mockspace-config`, `mockspace-builtin-lints` (the `ls-files -- .mock` lint), `mockspace-test-fixtures`.
- Deletes: `mockspace-store/src/instance.rs` and all of `instance/` (`apply`, `backup`, `files`, `held`, `lines`, `overwritten`, `pending`, `records`, `stats`, `tracking`, `walk`); `ref_ops.rs` and `ref_history.rs` where they hash or commit by hand (the three `--no-filters` sites, `files.rs:228`, `backup.rs:320`, `ref_ops.rs:303`); the root `src/data_refs.rs` and `src/data_refs/` (`gate`, `landing`, `sparse`, `standing`, `verbs`; `hook.rs` goes in round 3); the per-pattern exclude seeding. The pairing code (`src/pairing*.rs`) is kept only as far as L17 says and is re-derived, not carried.
- Red tests first:
  - the verb matrix of 4.10: every tree-writing git verb of 010 2.7, run in the git tree and in the instance, in every shape and both placements, each asserting that an uncommitted and an ignored instance edit is still recoverable byte for byte afterwards;
  - `update` with an ignored file at an incoming path: git refuses, and the file is in the snapshot and the cache;
  - a file changed in two clones merges through git, with a custom merge driver of the fixture called (op's `:51`);
  - the LFS test inverted: through real git-lfs the tree holds the pointer and a second clone smudges it;
  - double-f `clean`, `rm -rf .mock` and `worktree remove -f -f`: the bytes come back from the newest copy, and for `external` nothing was lost;
  - `retire` refuses a dirty or unpushed instance;
  - an interrupted verb (killed after each git step) re-run to the same end state;
  - a bare clone with linked worktrees can still check out, fetch into and move its HEAD branch (the defect at `000_op.md:25`);
  - the cache keeps its newest entry past 24 hours;
  - cost: one `commit` of one file stays under a fixed count of git processes, as a harness bench and not an assertion made up here.
- Tier: opus high. Storage and concurrency, and nobody to ask.

### Round 3: hooks through git-shook, and the pre-write copy

- Carries: 4.7, 4.5 (pre-write copy), L15, L21, L25, L26, D8, R2 (prevention and detection); op at `000_op.md:105`, `:113`, `:135`, `:143`.
- Feature: `instantiate` installs git-shook where absent and registers four entries (`pre-commit` lints with the tracked-`.mock` refusal, `pre-push` as a stub that round 4 fills, `fsmonitor-watchman` for the pre-write copy, `post-merge` and `post-rewrite` for regeneration, filled by round 5). A person's own hooks keep running. `status` says whether the pre-write copy is on, and why not.
- Crates: `mockspace-cli`, `mockspace-store` (the copy), root `src/bootstrap/` where the v2 install still reads it.
- Deletes: `src/data_refs/hook.rs`; `src/bootstrap/hooks.rs`, `hooks/layout.rs`, `follow_up_hooks.rs` and the chain hook that spawns `sh` on every ref move; `src/bootstrap/shook.rs` is written again against the fixed git-shook.
- Red tests first:
  - the pollution rows of 4.10: a commit tracking `.mock/<ns>/x` arrives by `merge`, `checkout`, `checkout -f` and `reset --hard`; the edit is recoverable from the pre-write copy or the snapshot, and `verify` and `pre-commit` name it;
  - the hook rows: auto-install over existing hooks, over `core.hooksPath`, over an existing `core.fsmonitor`, each asserting the person's hook still ran and its output is unchanged;
  - the stdin cases of `014_probes/g1b` as tests: a git command with stdin from a pipe, from `/dev/null` and inside a `while read` loop returns, and the loop reads all of its lines;
  - a hung engine behind a hook does not stall the git command past a stated timeout;
  - the pre-write copy runs once per tree change and not again on re-entry.
- Tier: opus medium for the design and the copy, sonnet high for the install rows.

### Round 4: the remote: guard, repair and purge

- Carries: 4.6, L12, L16, L21 to L24, L32, R4, R9; op at `000_op.md:101`, `:111`, `:112`, `:137`, `:150`, `:154`, `:171`.
- Feature: contact on every remote-touching verb (the five cases), the `pre-push` layer, unforced repair with its daily count, the epoch order, the `Mock-Ref ... epoch=<n>` witness, `purge` as the one forced push, and adoption in every other clone. A purge commit is accepted only when signed by a key the git tree declares; where the declaration is read from (which commit of which branch) is settled in the topic file after probe P14, because a declaration a purger can edit in the same push is no authority. A second remote is a declared property, default off.
- Crates: `mockspace-store`, `mockspace-cli`, `mockspace-config` (purge keys, second remote), the root `src/pairing*.rs` successor for the witness line.
- Deletes: whatever of `ref_store.rs`, `retry.rs` and `merge.rs` pushes or fetches a mock ref with force; the shipped backup-on-push records.
- Red tests first:
  - the remote rows of 014 2.9: `push --mirror` from a plain clone, from an opted-in clone with and without `--no-verify`, a deleting push, a forced rewind; after each, one engine command in any other clone restores the ref unforced and no tip is lowered;
  - the purge rows: clones A to E of `016_probes/p2`, the witness carried only by `Mock-Ref` lines, and the clone with no contact that republishes and is reversed;
  - an unsigned purge, a purge signed by an undeclared key, and a purge whose key was added in the same push are each refused by contact in every clone, and a declared key is accepted;
  - op's question at `:101` as a test: the true tip is deleted on the remote and held by exactly one clone, then by none; the first is repaired, the second is reported as lost with what the cache still holds;
  - the repair threshold stops and warns;
  - local-only commits survive adoption, and a conflicting pick stays on its `-pre-epoch-<n>` ref.
- Tier: opus high. Concurrency across clones and the only forced operations in the engine.

### Round 5: the shadow tree, two-way

- Carries: 4.3, 4.3.1, L30, L31, L34, D9, D10, D12, R10; op at `000_op.md:97`, `:109`, `:133`, `:141`, `:142`, `:172`, and on the references rewrite `:39` with `:75`.
- Feature: templates live in `mock://+/`, `regenerate` renders twice, classes each output by its produced set, carries a clean edit back into the template and never overwrites a pending one. The merge driver keeps ours only for two pure regenerations and otherwise returns `git merge-file`. `status` lists pending carry backs with their class.
- Crates: `mockspace-template`, `mockspace-core`, `mockspace-cli`, `mockspace-builtin-lints` (the contents rule of the shadow tree), root `src/render*.rs`, `src/render_agent/templates.rs:266`, `src/agent_builtin/`, `src/bootstrap/driver_script.rs`.
- Deletes: the `generated && followed_up` keep-ours block of `driver_script.rs`; the adjacency readers that find a template beside its output (all go through one `shadow(git_path)`); the template pattern machinery of the shipped data refs; `follow_up_commit.rs` where regeneration replaces it.
- Red tests first:
  - every case of `015_probes/j1` (11) and `016_probes/p1` (12) against the real renderer;
  - the classification and base rows of `017_probes/k1`, with a feature branch that regenerated from newer data;
  - the driver rows of `017_probes/k2` through the real generated driver, the binary row included;
  - regeneration repeated with an edit pending: the edit is byte-identical after each run (N1, N2);
  - a squash merge, a cherry-pick and a direct commit as the source of an edit;
  - a GitHub-style merge with no custom driver;
  - each class of 4.3.1 produces its named `status` line and writes nothing;
  - a file in the shadow tree that is none of the four permitted kinds is refused by `verify` and the lint;
  - a non-deterministic generator is refused by name.
- Tier: opus high. Hidden edge cases dominate, and a wrong merge is a silent revert.

### Round 6: the migrations

- Carries: 4.8, 4.9 (R.0 to R.6), R8; op at `000_op.md:7`, `:9`, `:59`, `:65`, `:123`.
- Feature: one named, idempotent, undoable migration from each starting state to the mock trees: a v1 consumer with a tracked mock directory, a shipped-v2 clone with `refs/heads/mock/<ns>` data refs and registry stores, and a clone caught half way. Templates move into `mock://+/` with the produced set seeded. References are rewritten to the letkajenkka spec in the same migration, after which off-spec text is refused. The R.5 sweep keeps collecting branches an old engine recreates.
- Crates: `mockspace-cli` (the migration verbs), `mockspace-store`, `mockspace-registry` (the store hook merge of R.1), root `src/migrate*.rs` and `src/migrate_refs*.rs`.
- Deletes: `src/migrate_refs.rs` and `migrate_seeded.rs` as written for the pattern design; the `form` key's `branch` value in `RefDecl`; the remote branches themselves, only once each tip is an ancestor of its published ref.
- Red tests first:
  - a full migration over mock data for each starting state, comparing every byte of every file before and after, tracked, untracked and ignored;
  - each step killed and re-run (every R step, every 4.8 step), ending in the same state;
  - `migrate --undo` after each step restores the starting bytes;
  - a registry row held in one clone that has not migrated is present after R.1 to R.5 in every order of two clones;
  - a remote branch is not deleted while its tip is not an ancestor of the published ref;
  - an old engine recreates `refs/heads/mock/<ns>` after the migration, and the sweep merges and removes it without losing its commits;
  - an output with an existing hand edit starts in conflict state and is not overwritten;
  - an edit made at a pre-migration commit and carried back by `checkout -f`, `reset --hard` and a rebase across the migration is recoverable (the three losses at `000_op.md:19`, `:57`);
  - the dry run prints exactly what the real run does;
  - `lints.toml` array `extends` and ref-held templates are rewritten (the misses of the shipped rewrite).
- Tier: opus high. One-way migration.

### Round 7: acceptance, the emulated use of a mockspace

- Carries: op's bar at `000_op.md:7` and `:9`, the residual list R1 to R10, L25 and op's bound at `:105`.
- Feature: scenario suites that play a mockspace through weeks of ordinary use across several clones, and the benches. No new mechanism. Whatever a scenario finds goes back to the round that owns it as a red test there.
- Crates: `mockspace-test-fixtures`, the root `tests/`, `mock/benches`.
- Deletes: the shipped `tests/refs_*.rs` and `tests/refs_e2e/` that earlier rounds left standing, each asked first what it protects. A test that protects a property the new design still has is rewritten in the new world.
- Scenarios, red first:
  - two people and an agent seat on one consumer: research added, a round run, templates edited, a pull request that edits generated docs merged on the forge side, the next person regenerating; no byte lost at any step;
  - a copy of a real consumer's mock data (kaski's shape, anonymised sizes) migrated, used and purged once;
  - every residual R1 to R10 played out, each ending inside its stated bound;
  - a clone that sleeps through the migration and a purge, then wakes;
  - the three reds of the audit (`tests/refs_consumer_setups.rs:168`, `tests/refs_daily_history.rs:105`, `tests/refs_cost_scaling.rs:160`) restated in the new world, with the four-row sample replaced by the full set;
  - harness benches at 30 000 and 100 000 files for the pre-write copy and for each verb, against op's sub-second bound.
- Tier: opus medium for the scenarios, sonnet high for the benches and the fixture growth.

## Order and dependencies

Ranked by what each unblocks:

1. Outside work O1 (git-shook) and O2 (letkajenkka), with the probes that need no code. Unblocks rounds 1 and 3.
2. Round 1. Needs O2. Unblocks 2, 3, 4, 5, 6.
3. Round 2. Needs 1 and probes P6, P10, P12. Unblocks 3, 4, 5, 6.
4. Round 3. Needs 2 and O1, and P2, P3, P9. Unblocks 4 (the `pre-push` entry), 5 (the regeneration entries) and the bound on R2.
5. Round 4. Needs 2 and 3, and P1, P7, P14. Unblocks 6 (nothing is published before the guard exists).
6. Round 5. Needs 2 and 3, and P4. Unblocks 6 (the template move).
7. Round 6. Needs 1 to 5, and P8, P11, P13. Unblocks the port of every consumer.
8. Round 7. Needs all. Its fixture work starts with round 1 and grows with each round.
9. Outside work O4 (the consumers' port) only after round 7 is green on the merge of feat/v2 with dev, measured on that merge.

Rounds 4 and 5 share no crate surface beyond `mockspace-cli` verbs and could run side by side at sonnet tier only; at opus they run one after the other, 4 first.

## Known defects on feat/v2, each against its round

The ticket tool has no listing verb in this container and `cargo mock tickets` printed only the launcher help, so the ids below were read from `.shared/scripts/life/tickets-mockspace.toml` and `tickets-git-shook.toml` in the workspace, which may be behind the registry. Each is re-read at its round's opening.

- `two_data_ref_namespaces_where_one_prefixes_the_other_cannot`: round 1 (L13, R.6).
- `move_the_registry_stores_off_refs_heads_to_the_custom_mock_n`: the refs in round 1, the move of existing rows in round 6.
- `hand_split_references_remain_where_letkajenkka_should_parse`, `task_and_namespace_ids_keep_hyphens_beside_the_letkajenkka_s`, `round_202611110830_says_the_syntax_folds_names_the_host_does`, `mockspace_refs_by_the_letkajenkka_spec`: round 1 for the reader, round 6 for the rewrite.
- `a_data_ref_holds_a_filtered_file_as_its_pointer_as_op_picked` (and its question `a_data_ref_holds_a_filtered_file_as_its_full_bytes_not_the_p`, `test_the_filtered_hash_of_an_instance_file_with_an_lfs_filte`): round 2. The defect goes with the code: git commits the file, so the filter runs.
- `the_reftable_copy_test_passes_green_when_this_git_cannot_run`, `run_the_reftable_copy_test_on_a_git_with_refs_migrate`: round 2, `#[ignore]` naming the ticket until P12 has a git of 2.46 or later.
- `a_ref_file_changed_in_two_clones_has_no_engine_way_through`, `per_worktree_config_in_a_bare_clone_leaves_its_head_branch_u`, `the_way_past_a_copy_a_git_gc_pruned_is_provisional`, `a_data_refs_commit_of_one_file_starts_about_150_git_processe`, `the_store_leaves_a_blob_staging_directory_and_writes_bytes_t`, `materialising_a_data_ref_over_a_file_the_branch_has_gives_a`, `data_refs_instantiate_refuses_an_instance_that_is_there_the`, `a_data_ref_s_exclude_lines_are_only_appended_and_no_verb_cha`, `data_ref_instances_read_ignored_files_until_pr_229_lands`, `data_refs_backup_reverting_to_bytes_already_in_the_odb_after`: round 2. Most close by deletion of the mechanism they are about; each still gets its test in the new world.
- `git_shook_s_dispatcher_reads_stdin_whole_so_an_fsmonitor_hoo`: O1, proven closed by round 3's stdin rows.
- `data_refs_hook_a_hung_launcher_stalls_the_git_command_no_tim`, `chain_hook_spawns_sh_on_every_ref_move_in_every_activated_re`, `a_failed_merge_driver_registration_lets_the_command_go_on`, `split_gate_rs_bootstrap_hooks_rs_and_entry_dispatch_rs_under`: round 3.
- `data_refs_paired_fails_after_a_rehouse_in_the_clone_that_hel`: round 4 (the witness and pairing of the current epoch only).
- `the_mockspace_generated_merge_driver_keeps_ours_and_drops_a`: round 5 (D12). L34 widens it: the edit is dropped for the whole file, not the hunk.
- `the_references_rewrite_of_ref_held_templates_is_refused_by_a`, `a_merge_of_generated_or_append_only_files_resolves_itself`, `the_merge_drivers_run_in_every_merge_command_and_every`: round 5.
- `data_refs_an_edit_made_at_a_pre_migration_commit_is_lost_by`, `the_way_back_from_a_pre_migration_commit_drops_an_edit_carri`, `a_forced_checkout_back_from_an_old_commit_drops_an_edit_no_e`, `a_rebase_across_the_migration_may_drop_an_edit_no_copy_holds`, `close_the_two_pinned_data_ref_losses_before_feat_v2_reaches`, `references_to_letkajenkka_misses_lints_toml_array_extends_an`, `fix_reference_migration_span_edits_fences_and_guide`, `a_git_failure_in_config_from_dir_mid_migration_is_a_panic`, `decide_the_gate_scope_of_a_reference_the_standard_refuses` (answered at `000_op.md:65`): round 6.
- `no_test_shows_data_refs_commit_refused_when_the_repo_s_lints`, `v2_cli_golden_install_bare_fixture_tree_is_stale_on_feat_v2`, `the_lock_test_the_lock_is_held_until_it_is_dropped_fails_onc`, `bench_generated_e2e_fails_under_load_and_passes_alone`: round 7, or sooner where the round that touches the crate meets them.
- The suite defects 017 names: `tests/refs_backup.rs:325-327` (round 2), `tests/refs_consumer_migrations.rs:171-183` (round 2), `tests/refs_daily_history.rs:106-111` (round 7).
- `the_engine_build_needs_the_private_letkajenkka_over_ssh` and the question `how_should_the_public_mockspace_engine_depend_on_letkajenkka`: O2. The question is op's and gates feat/v2 reaching dev; a lockfile test refusing any `git+ssh` source goes in red with round 1.

## Work outside mockspace

- O1, `git-shook` (repo `git-shook`). Two fixes, both gates of 4.7: the dispatcher replays stdin only for the events git feeds on stdin (`pre-push`, `pre-receive`, `post-receive`, `post-rewrite`, `reference-transaction`, `proc-receive`, `push-to-checkout`), and exactly one entry answers the `fsmonitor-watchman` protocol, the person's own or the floor. Plus what P3 finds about installing over existing hooks. Tests in git-shook's own `tests/`, from `014_probes/g1b` and `012_probes/f1`. Lands and is released before round 3 opens. Tier: opus medium, since a hook that hangs stalls every git command.
- O2, `letkajenkka` (repo `letkajenkka`). Three things: the case rule (capitals allowed or refused) becomes a style setting read from `letkajenkka.toml`, with the spec and conformance cases for both settings; the `mock://<ns>/<p>` address is a form the standard parses, with `+` valid as a namespace; and the dependency question above is answered so a public engine can build. Lands before round 1 opens, since every ref and path that is neither a filesystem path nor a literal git one goes through it. Tier: sonnet high for the setting and conformance, opus medium for the address form.
- O3, `mockspace-refnames`. It is a crate inside mockspace's v2 tree (`mock/crates/mockspace-refnames`) and its work is part of round 1: it becomes the only place the engine folds a name, builds a ref name, or resolves `mock://`. Should it be wanted by another repo it is published from there; no separate repository is proposed.
- O4, the consumers (kaski first, then the rest that run a mockspace). No work until round 7 is green. Then each is migrated on a copy first, with the round 6 migration and its undo, and only then for real.
- `mockspace-extra-lints`: the tracked-`.mock` lint and the shadow-contents lint are general, and move there after rounds 2 and 5 where a consumer needs them without the engine.

## Probes owed, each before the round that leans on it

Those marked "op" need something only op can give.

- P1, a real host (op: which hosts, and an account that may push custom refs and delete them): accepts `refs/mock/*`, what `push --mirror` does, fetch by id after deletion, object survival, non-fast-forward refusal. Before round 1 locks its DOC, since it decides the default vehicle, and again before round 4 for the purge.
- P2, the git-shook fixes, then `014_probes/g1b` and `012_probes/f1` re-run. Part of O1, before round 3.
- P3, auto-install over existing hooks, `core.hooksPath` and an existing `core.fsmonitor`. Before round 3.
- P4, the real renderer under carry back. Before round 5.
- P5, external placement on macOS and Windows, and the symlink on Windows (op: a machine of each). Before round 2 ships the `external` arm outside Linux; on Linux the arm ships without it, and the other platforms refuse `external` until the probe is in.
- P6, second reads of the per-worktree working ref, with rebase, merge and gc reachability once HEAD has left it. Before round 2.
- P7, a second read of the purge epoch by a seat other than 002. Before round 4.
- P8, the shipped engine when its remote branch disappears or reappears. Before round 6.
- P9, a harness bench of the pre-write copy at 30 000 and 100 000 files. Before round 3 turns the copy on; repeated in round 7.
- P10, a census of namespaces read by path and of relative paths in templates. Before round 2 (which namespaces may be external) and round 5.
- P11, `adopt` of a pre-migration feature branch through `git apply -3`. Before round 6.
- P12, reftable on git 2.46 or later (the container has 2.43; op if the environment cannot carry a newer git). Before round 2 closes.
- P13, LFS through a real server with the per-namespace remote (op: a host with LFS). Before round 6; round 2 tests against a local git-lfs.
- P14, new: the signed purge. Which signature form (`gpg.format = ssh` with an allowed-signers file built from the declaration is the candidate), and which commit's declaration contact trusts, so that a purger cannot declare its own key in the same push. Before round 4.
- P15, new: `+` as a name across the verbs of `015_probes/d1`, in `.gitignore` and `.gitattributes` patterns, in a refspec written to config, and as a directory on Windows and macOS (the last two with P5). Before round 1.

## What this plan does not settle

- Whether `mockspace-store` or a new crate holds the mock-tree plumbing. Round 2's topic.
- Where the purge keys are declared and read from. Round 4's topic, after P14.
- How the public engine depends on letkajenkka. Op's, by the standing question ticket.
- 017 is silent on what a v1 consumer's migration reads beyond 4.8. Round 6's topic states it and a second reader checks it against the shipped `docs/` migration guides before DOC locks.
