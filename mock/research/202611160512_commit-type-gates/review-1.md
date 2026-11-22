# Review 1: `work/commit-type-gates` at 5920744 against `dev` fb3db6e

Canon gate: mockspace v1 declares no canon, so op's request quoted in `survey.md:9` governs. The intent, block more readily and add types per project, matches what the branch is for. Neither op nor any canon has said whether the lint belongs in the engine or in a pack. Whatever the answer, building a mirror is a defect (`extract-the-shared-logic-never-mirror-it`). This is the first read, and a second is owed. Test gate: `cargo test -p mockspace-lint-rules` passes 648 tests (8 ignored), and `cargo test -p mockspace --lib -- commit_s message_subject` passes 17. Several tests are redundant or check only some of the knobs (findings 7 to 9).

## Verdict

DO NOT MERGE

## Findings

1. Blocker [correctness]: landing this breaks every consumer the moment it lands, not "after a pin bump". All ten consumers set `mockspace_branch = "dev"`: applets, arvo, hilavitkutin, homma, huurre, kaski, kirjo, lato, muisti and one more (`grep '^mockspace_branch' */mockspace.toml`). `README.md:28` says such a pin "moves under you by design". `survey.md:59` and the brief both assume a project has to choose to take the change, which is false. Here is what gets refused straight away:
   - kaski: `tickets` is about 35% of its last 300 subjects, and the workspace ticket tool writes them (`.shared/scripts/remote/ticket:587,605,641,655,677,786`), and its header says "Never `--no-verify`" (`:113`). Every ticket write stops working.
   - arvo: `research` (91), `panel`, `probe`, `canon`, `registry`, `style`.
   - applets: `gui`, `frame`, `design`, `round`.
   - hilavitkutin: `research`, `plan`.
   - The pre-push scan also refuses in-flight helper branches that already carry these types.

   Survey line 59 says this cost "is stated here rather than hidden". Stating a cost does not deal with it. Fix: first land `[lints.commit-subject] extra_types` in each consumer from the survey's own counts. The old engine only lists an unknown table as "configured and matching no lint" (`src/render_agent/catalogue.rs:54`, report only), so the order is safe. Only then land this branch.

2. Blocker [drift]: `lint-rules/src/commit_subject.rs` is a near-copy of `mockspace-extra-lints/src/lints/commit_style.rs`. It has the same nine knobs with the same names, the same `configure` parsing (`commit_style.rs:135-166`) and the same `check_type<'m>` shape. The survey admits the two "can disagree" and leaves retiring the pack copy as "a change in the other repository" (`survey.md:73`), with no ticket. They already disagree:
   - muisti (`mockspace.toml:938`) sets `[lints.commit-style] types` without `bench` and `max_subject = "72"`.
   - Once this lands, the builtin skill tells muisti's agents that `bench` is allowed and that the limit is 71, while muisti's pack gate refuses `bench`.

   That breaks the branch's own claim that the skill and the gate read one source. `docs/USAGE_GUIDE.md:27` hands the conflict to each consumer to switch off by hand. Fix: keep one implementation. Either the builtin, with `commit-style` deleted from the pack and `muisti` and `riimu` moved to `[lints.commit-subject]`, or one shared core both call. Do it in the same arc.

3. High [correctness]: a value the lint cannot read changes the gate silently (`commit_subject.rs:253-274, 481-486`).
   - `require_type = "ture"` and `"enabled"` both read as false, which turns off the type requirement.
   - A bad `max_subject` or `subject_case` silently keeps the default.
   - A misspelled key such as `extra_type` is ignored, and the refusal message then tells the author to set `extra_types`, which they think they already did.

   No lint's params are checked against `config_keys()` today; only tools do that (`lint-rules/src/tool/params.rs:121`). For a gate that is on by default and blocks, that is not acceptable. Fix: refuse unknown keys and unreadable values loudly, the way `refuse_lint_type` does, and add a test for each.

4. Medium [correctness]: a subject starting with a blank line is refused at the commit gate. The `commit-msg` hook reads the message file before git's cleanup strips leading blank lines, so `"\nfix: x"` raises `empty-subject` even though git would record `fix: x`. `commit_subject_tests.rs:147-150` locks this behaviour in. Fix: skip leading blank lines before taking the subject.

5. Medium [correctness]: the exemptions at `commit_subject.rs:469-472` are not split by gate.
   - `fixup! `, `squash! ` and `amend! ` also pass at the push gate, so autosquash leftovers reach the trunk unchecked. They should be refused at push.
   - Any authored subject that starts with `Merge ` skips the whole gate.
   - Git 2.43 and later writes `Reapply "..."` when you revert a revert, and that is refused.

6. Low [correctness]: `commit_subject.rs:419-424` reports an unclosed scope as `unknown-type`, which is the wrong finding kind. A scope that contains a space, `feat(a b):`, comes out as `missing-type`.

7. [tests]: the tests check only some cases of each law.
   - `every_knob_can_loosen_what_it_tightens` (`:239`) covers 4 of the 9 knobs. A mutant that never reads `separator`, `breaking_marker` or `require_type = false` passes the whole suite.
   - The length test (`:132`) uses only ASCII, so replacing `chars().count()` with `len()` passes.
   - The `..` exemption at `:341` has no test.
   - The case check is tested only on ASCII letters.

8. [tests]: two tests should be deleted.
   - `a_disabled_skill_is_still_a_name_the_sweep_knows` (`commit_style_tests.rs:181`) asserts that a constant contains a literal. `render_agent/tests.rs:641-660` already enforces this, so the test is redundant.
   - `assert_eq!(CommitSubject::from_params(None), CommitSubject::default())` (`commit_subject_tests.rs:278`) cannot fail, because that is the whole body of `from_params(None)`. Delete that line.

9. [tests]: the documentation says `commit = "off"` alone turns the check off (`commit_subject.rs:13`, `USAGE_GUIDE.md:24`, `SKILL.md:17`). That works only because of the `or_insert(Severity::OFF)` in `src/config.rs:1438`. The test (`message_subject_tests.rs:93`) sets all three gates, so the form the docs describe is never tested. Add a test with only `commit = "off"` that runs the push gate.

10. [rules]: `commit_subject.rs` is 501 lines. The 500-line rule is a smell, not a hard limit, but the knob table and parsing are a clear seam to split out.

11. [drift]: `src/entry/message.rs:86` still mentions "the empty-lints early return below", which this branch deleted.

12. [drift]: `survey.md:35,55` says every repository has its own hand-written commit-style skill. Only arvo and hilavitkutin have one. Both override the builtin and keep a fixed list with no `bench`, so the "one source" claim is false there until they switch to `{{commit_types}}`. Make that switch in those two repos as part of the consumer step in finding 1.

## Notes

Both commit messages are clean: correct shape, no trailers, no em-dashes. No em-dash appears in the diff. The engine half is sound: the early return is gone, the batch path runs the builtin, and the lint is registered in the catalogue and the duplicate check. The blockers are in landing order (finding 1) and the duplicate lint (finding 2), not in the engine code.
