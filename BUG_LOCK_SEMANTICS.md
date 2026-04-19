# BUG / design wart: src-CL `lock` semantics are backwards from user intuition

## What's wrong

Users reading "lock the src changelist" intuitively expect:
> "Freeze the plan before I start implementing."

But mockspace's current state machine treats it as:
> "The implementation is complete; sign off on the round."

Concretely, the write-guard hook (`.claude/hooks/mockspace-write-guard.sh`) does this:

```bash
if [[ -n "$SRC_CL_LOCKED" ]]; then PHASE="DONE"
elif [[ -n "$SRC_CL_ACTIVE" ]]; then PHASE="SRC"
...
```

And `.rs` edits under `crates/*` are permitted ONLY in phase `SRC`:

```bash
if echo "$REL_PATH" | grep -qE '\.rs$'; then
    if [[ "$PHASE" != "SRC" ]]; then
        deny "BLOCKED: cannot edit '${REL_PATH}' -- not in SRC phase."
    fi
```

So `cargo mock lock` on a src CL immediately forbids further `.rs` edits. The "contract" isn't enforced by the `.lock.md` suffix; it's enforced by git commit + convention while the CL is still `.md`-active.

Session tripping this exposed the surprise: a user and an agent both expected to lock the src CL before implementation starts; the tool said no. The workaround was to deprecate the prematurely-locked src CL and reauthor an active one. Destructive, wasteful, reputationally confusing.

## The fix options

Two reasonable directions. Pick one; don't do both.

### Option A — rename the suffixes / phase labels to match the user model

Keep the state machine shape, relabel:
- `.src.md` (active, pre-plan) → `.src.plan.md` or similar
- `.src.lock.md` (current "frozen-post-impl" state) → `.src.done.md`
- PHASE `SRC` (implementation allowed) → keep, or rename to `IMPL`
- PHASE `DONE` → keep

Lowest-invasive relabel: rename `SRC-PLAN`→`DRAFT`, `SRC`→`IMPL`, `DONE`→`CLOSED`. Users then intuit "lock" applies to the CL filename state, not to "freeze".

### Option B — introduce an actually-frozen-during-impl state

Add a phase between `SRC-PLAN` and the implementation phase:

```
SRC-PLAN  (src CL active, planning)
    │
    │ cargo mock lock-plan
    ▼
SRC-LOCKED-PLAN  (src CL .src.plan.lock.md, impl can start, CL is frozen)
    │
    │ implementation happens; .rs edits allowed
    │
    │ cargo mock close-impl  (or similar)
    ▼
DONE  (src CL .src.done.md or similar; no more writes)
```

Filename suffix now tracks three states per CL kind instead of two: active / plan-locked / closed. The "lock" verb in `cargo mock lock` means what users expect — freeze the plan before impl.

Would require:
- new file-suffix parsing in `lint-rules/src/changelist_helpers.rs`
- new phase discriminant
- updated hook enforcement (both changelist-immutability AND the crate `.rs` gate)
- updated subcommand behaviour (`cargo mock lock` while in SRC-PLAN advances to SRC-LOCKED-PLAN, not DONE; new subcommand for the impl-complete transition)

## Recommendation

Option A is cheaper. It's a semantic relabelling, not a schema change. Agent prompts, docs, and `cargo mock <subcommand>` help-strings need updating, but no state-machine logic changes.

Option B is more correct but more work. Defer unless the relabelling in A feels too hollow after use.

## Severity

Not a blocker. Workflow is usable with the current semantics once you know them. But it ambushed both the user and a subagent during the first real design round (arvo L0, 2026-04-19). Worth fixing before more users hit it.

## Links

- Session that tripped this: `~/Dev/clause-dev/arvo/` branch `feat/arvo-l0-design-round`, deprecated `202604192100_changelist.src.deprecated.md`, reopened as `202604192200_changelist.src.md`
- Hook source: `.claude/hooks/mockspace-write-guard.sh` (generated; source in `mock/agent/hooks/` but this one is a builtin from `src/render_agent.rs`)
- Phase logic: `lint-rules/src/changelist_helpers.rs` `current_phase()`
