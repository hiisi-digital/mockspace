# Review 3: the commit subject collection

Reviewed: mockspace `work/commit-type-gates` at `7689220` (round-2 fixes `1c1ae75..7689220`, merges clean onto `origin/dev` `01ff5bc`); mockspace-extra-lints `work/retire-commit-style` at `64ef2b4`; applets `db4e64c`; arvo `79963bee` and `8a209381`; hilavitkutin `0ef8985d` and `9b9c444e`; homma `7bf715f`; muisti `5500c00`; riimu `9ce6eb7`. Every consumer branch merges clean onto its `dev`.

Canon gate: passed. mockspace v1 declares no canon, so op's request in `survey.md` governs: a gate that blocks more readily, per-project types without a mockspace change, `bench` a good type. The branch does those three. I formed that reading from the quote before reading reviews 1 and 2; this is the third independent read agreeing. Test gate: `cargo test -p mockspace-lint-rules` 675 passed, 8 ignored. `cargo test -p mockspace --lib` 1840 passed, 3 failed, all three the ticketed `bench_builds::tests::a_sweep_*`. I read every test the round-2 fixes add; none is tautological, each names a mutant it would kill.

## Verdict

MERGE AFTER FIXES. The mockspace and extra-lints branches are sound. Findings 1 and 2 are small consumer-side edits and must land before the mockspace branch does.

## Round-2 findings

1 (`release`): resolved in the defaults (`policy.rs:13`) with a test, but the class lives on in muisti and riimu (finding 1). 2: resolved, applets lists `deploy,merge`. 3: resolved, `survey.md` step 2 holds the mockspace branch on kaski and huurre. 4: resolved, arvo and hilavitkutin split into `commit-subject-types` (table only) and `commit-subject-config` (template), checked. 5: resolved, `subjects.rs` asks the lint, the hand model is gone, `FOLLOW_UP_SUBJECT{,_REBASE}` and the archive subjects are covered. 6: resolved, `override_keeps_passes` and `keep_passes` (`gate.rs:48-71`), unit and integration tests, riimu's `build = "off"` case included. 7: resolved, examples are filtered through the lint and swept over max 0..80 and 200, four separators, three cases. 8: resolved at push (`recorded`); commit-gate residual is finding 5. 9: resolved. 10: resolved, `never_matches` with two tests.

## Findings

1. Medium, must-fix [correctness]: muisti `mockspace.toml:942` and riimu `mockspace.toml:292` set `types = "feat,fix,refactor,docs,test,chore"`, which replaces the defaults and drops `release` and `bench`. `homma release run` merges with `release: <v>` (`homma/mock/crates/homma-core/src/release/run.rs:375`), and `git merge` runs `commit-msg`, so the first release of either repo is refused at its own gate. That is round-2 finding 1 surviving in the two repos that replace the set. `survey.md:116` ("in every repository") is false for them. Fix: add `bench,release` to both lists, or move both to `extra_types`, and correct the survey line.
2. Low, must-fix [drift]: the comment above the table names the defaults without `release` in arvo `mockspace.toml:1529` (both branches), hilavitkutin `:1538` (both branches) and homma `:44`. applets `:242` is right. One-line edit each.
3. Low, follow-up [rules]: muisti `max_subject = "72"` (`:946`) admits a 72-character subject. The workspace rule is under 72. Set `71` with finding 1, or record it as muisti's deliberate choice.
4. Low, follow-up [correctness]: arvo's set lacks `merge` (6 on `dev`, last 2026-08-31, `git merge -m "merge: ..."`), the same habit applets needed listed. The next such merge in arvo is refused. Older one-offs (`register`, `handles`, `options`, `probes`, all August) can stay out.
5. Low, follow-up [correctness]: at the commit gate `git commit -m "#42 fix it"` passes. With no editor git's cleanup is `whitespace` and keeps the line, so the push gate is the first to refuse it. The comment in `src/entry/message_subject_tests.rs` (`a_pound_subject_...`) says "git itself aborts an empty message", which is only true with an editor. `#` is hard-coded, and `core.commentChar` is ignored. The push catches it, so nothing reaches a remote.
6. Low, follow-up [drift]: `commit_subject.rs:50-51` and `docs/USAGE_GUIDE.md` say `fixup!` is refused "at the build and push gates". I found no automated caller of `check-message --gate build`. The hooks pass only `commit` (`hooks.rs:135`) and `push` (`hooks.rs:253`). The build level fires only on a manual invocation. Next check: whether `mock` runs message lints at build anywhere I missed. If it does not, narrow the text.
7. Low, follow-up [drift]: no load check stops a `types` list dropping a type mockspace's own tooling writes (`chore` in `subjects.rs`, `merge_driver.rs:50,53`; `release` from homma). Finding 1 is one instance. A load warning would catch the class.
8. Low, follow-up [drift]: `src/entry/message.rs` grows from 512 to 523 lines, past the 500 tripwire. `run_message` is the seam.

## Verified

- Every consumer table on these branches passes the new load checks. Every key is in `KEYS`, every boolean, number and case parses, and no type entry holds whitespace, the separator or `(`.
- Pack cdylibs are patched to the engine's own lint-rules (`custom_lints.rs:347`), so the new default trait method cannot split a vtable. No consumer builds `MessageContext` by struct literal, so the new `recorded` field breaks no pack.
- `[lints.commit-style]` remains on `dev` only in huurre (ticketed), muisti and riimu (moved on their branches). extra-lints holds no `commit-style` reference.
- Every commit subject on all branches is under 72 characters and in shape, with no byline, advert or dash glyph.
