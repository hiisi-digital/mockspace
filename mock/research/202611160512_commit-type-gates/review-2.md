# Review 2: the commit subject collection

Reviewed: mockspace `work/commit-type-gates` at `babbb78` against `dev` `fb3db6e`; mockspace-extra-lints `work/retire-commit-style` at `64ef2b4`; the six `work/commit-subject-config` branches (applets `d4cc9dc`, arvo `8a209381`, hilavitkutin `9b9c444e`, homma `7bf715f`, muisti `5500c00`, riimu `9ce6eb7`).

Canon gate: passed. mockspace v1 declares no canon, so op's request quoted in `survey.md:9` governs. A default-on gate, a per-project `extra_types` and `bench` in the default set are what it asks for. Review 1 was the first read on that and asked for a second. This review is the second, and I formed my reading from the quote before I read review 1's. Test gate: `cargo test -p mockspace-lint-rules` passes 669 (8 ignored). `cargo test -p mockspace --lib` passes 1836 and fails only the three known `bench_builds` tests. The integration binaries fail three more tests that this branch does not cause (Notes). I read every test the branch adds. One sampled law and one redundant assertion are findings 6 and 9, and both are owed before merge.

## Verdict

MERGE AFTER FIXES

## Round-1 findings

1 (landing order) is partly resolved: six consumers are covered, kaski and huurre are not, and applets is missing types (findings 2 and 3). 2 (mirror) is resolved for the pack. The same class lives on inside mockspace (finding 5). 3 to 11 are resolved, each checked against the code and a test that would fail. 12 is resolved, with a landing-order catch (finding 4).

## Findings

1. Blocker [correctness]: `lint-rules/src/commit_subject/policy.rs:13`. `DEFAULT_TYPES` has no `release`, but the workspace's own release tool writes that subject: `homma/mock/crates/homma-core/src/release/run.rs:375` (`git merge --no-ff -m "release: <v>"`, and `git merge` runs `commit-msg`) and `landing.rs:100`. huurre's `mockspace.toml:69-90` records the same refusal from a real probe. No consumer branch adds it. Once this lands, `homma release run` is refused at the commit gate in every consumer, and the pre-push scan of `main` refuses it again. Fix: add `release` to the defaults, since one tool writes it everywhere, and add a test that says why.
2. High [correctness]: applets `mockspace.toml:245`. `extra_types` is missing `deploy` (`bc16af3`, 9 Nov) and `merge` (`b10f5d2`, `93c4ffa`, `35b80aa`, 31 Oct to 6 Nov, all `git merge -m "merge: ..."`). The survey's 300-commit window is too short for a repository that moves this fast.
3. High [correctness, landing order]: kaski and huurre have no branch. The tickets are known, but `survey.md:60,75` names only muisti and riimu as the ones the mockspace branch waits on. Once the pack retires, huurre's `[lints.commit-style]` (`mockspace.toml:69`) governs nothing, and the defaults then refuse `research` (78 of its last 300 subjects) and `release`. Keep the mockspace branch from landing until 78609af5c53e closes and huurre's table moves, and say so in the survey. Make sure 081d8b9decad covers moving the table and not only the seat.
4. Medium [landing order]: arvo and hilavitkutin `SKILL.md.tmpl:12,16,23`. An older engine leaves `{{commit_rules}}`, `{{commit_types}}` and `{{commit_examples}}` in the text literally (`src/render_agent/vars.rs:182-188`, with no check for leftover tokens). If the consumer branches land first, as the survey orders, both skills render without rules or types until mockspace lands. Split the commit: land the config table now and switch the template after mockspace.
5. Medium [drift]: `src/design_round/subjects.rs:53-79`. A hand-written "model of the gate" is still there, and its doc says mockspace "does not carry" the gate. That is false from this branch on. It also disagrees with the gate: `>= 72` and a dash-glyph rule. Point the test at `CommitSubject` through `check_message_with_extra` and delete the model (`extract-the-shared-logic-never-mirror-it`). `merge_driver.rs:50,53` and `archive.rs:31,33` should go through the same check.
6. Medium [correctness, tests]: `lint-rules/src/commit_subject.rs:135-143` together with `lint-rules/src/run/gate.rs:40-52`. `restamp` overwrites a finding's own severity with the table's base severity. So `commit = "error"`, which muisti (`mockspace.toml:938`) and riimu (`:288`) both set, turns `autosquash-leftover` into an error at the commit gate. `fixup!` commits are then refused where the module docs say they pass. No test covers a base override with this finding. Fix the precedence, or drop the redundant gate keys from those tables, and add the test.
7. Low [tests]: `src/render_agent/commit_style.rs:91-93` claims that "an example never breaks the rules printed above it". This is false whenever `max_subject` is between 1 and 51: the `refactor` example is 52 characters. `commit_style_tests.rs:109-132` checks three configurations and none of them sets `max_subject` or `separator`. Check the law across those knobs, or truncate the examples.
8. Low [correctness]: `lint-rules/src/commit_subject/check.rs:58` drops every line starting with `#`, at the push gate too, where the message is the recorded one and has no comments. A recorded subject `#42 fix it` (`-m` uses whitespace cleanup) skips the gate. Strip comments only at the commit gate.
9. Low [tests]: `commit_subject_tests.rs:62` asserts that a constant contains a literal, and line 63 already proves the same thing. Delete line 62.
10. Low [correctness]: `policy.rs:215,248`. A list entry that can never match, such as `types = "feat fix"`, is not refused at load. It fails closed, but the message is confusing.

## Verified

- An older engine given `[lints.commit-subject]` only lists it (`src/entry/dispatch.rs:758`, `render_agent/catalogue.rs:54`) and never refuses.
- A new engine with a consumer that has not moved: kirjo and lato use no unknown types, only subjects that were too long in the past. muisti and riimu are covered. riimu's `style` (`e339a95`) is outside its set on purpose.
- Pre-push excludes anything already on any remote (`hooks.rs:185,194`), so in-flight pushed branches are not judged again.
- The pack retirement is complete, with no stray references. `mockspace-hilavitkutin-stack-lints` is the same repository under its old name.
- `check_params` runs on the only lint-table source, and severity keys never reach params.
- Every commit text in all eight branches follows the convention, with no bylines and no em-dashes.

## Notes

The three integration failures (`a_test_run_leaves_nothing_in_its_scratch_root`, `every_top_level_target_declares_the_scrub` and `the_bench_crates_build_into_the_target_dir_the_environment_chose`) come from `dev` (`2d7b5b2` added an unscrubbed target). This branch touches no `tests/` file. None of the three is ticketed in the brief. To run the suites, I used `build-sweep --older 30` to free 3.6 GB of build cache while no build was running.
