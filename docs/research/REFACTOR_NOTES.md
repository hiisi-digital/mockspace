# Refactor notes — 2026-04-19

Captures the design decisions behind the current cleanup pass. This document is a thinking artifact, written before the code changes land so future readers (human or otherwise) can see the reasoning without excavating commits.

## Motivation

Three observations triggered this pass:

1. **Platform asymmetry in agent hook generation is apparent but not real.** The authored source files under `mock/agent/settings/` (`claude.json` ≈ 6 lines, `copilot-hooks.json` ≈ 19 lines with explicit `preToolUse` bindings) suggest that Copilot gets configured hooks while Claude gets nothing. In fact, `render_agent.rs` generates hook bindings for *both* platforms programmatically from the same source — `mock/agent/hooks/*.sh.tmpl` plus built-in hooks — so behavior is symmetric. The files are visual cruft, not functional config.
2. **The authored settings files are effectively dead inputs.** `copilot-hooks.json` is never read by any code path in `render_agent.rs`. `claude.json` *is* read, but only for a string-typed `attribution` field; all reference consumers set `attribution` as an object (`{"commit":"", "pr":""}`), so the extractor silently returns `None` and contributes nothing to the generated `.claude/settings.json`. Neither file contains live content today.
3. **Mockspace has no README.** The tool has grown significantly as a side-product during other work, and has accumulated enough design-intent that the lack of a primer is now a cost every new user pays. "Documented" is a first-class goal of this pass.

## Decisions

### D1 — `mock/agent/hooks/*.sh.tmpl` is the canonical source of truth for agent hook generation

`render_agent.rs` reads hook templates from `mock/agent/hooks/`, combines them with built-in hooks (`check-byline.sh`, `mockspace-write-guard.sh`), and emits platform-appropriate hook files and hook configs for every supported agent platform. The same hook source must produce:

- **Claude:** `.claude/hooks/<name>.sh` + entries in `.claude/settings.json` `PreToolUse` array
- **Copilot:** `.github/hooks/<name>.sh` + entries in `.github/hooks/hooks.json`
- **Any future platform:** same treatment, derived from the same source

**Why this matters:** a single authored template must drive all platforms. Any asymmetry in generated output should come from platform semantics (Claude's `hookSpecificOutput` JSON wrapper vs. Copilot's flat JSON), not from authoring differences. Mockspace's platform-helper injection mechanism (`{{HOOK_HELPERS}}` substitution) already handles the semantic gap correctly.

### D2 — `mock/agent/settings/claude.json` and `mock/agent/settings/copilot-hooks.json` are deprecated; the reading code is removed

- `copilot-hooks.json` is unused — delete the files from consumers, no behavior change.
- `claude.json` is read but does nothing useful today (see Motivation #2) — remove the reading code and the `extract_json_string_field` helper. Delete the files from consumers.

**Why this matters:** carrying dead code and dead boilerplate files confuses every reader of the codebase. They look meaningful, invite "let me customise this", and then don't actually control anything. Either make them meaningful (D3) or delete them. We're deleting them.

### D3 — Non-hook agent settings are authored once in `mock/agent/config.toml`, rendered per platform

Agent-integration settings live in `mock/agent/config.toml`. Platform-specific authoring is forbidden by design: every value applies equally to every supported platform, rendered into each platform's native format at generation time.

**Landed first-party setting — attribution (byline policy):**

```toml
# mock/agent/config.toml
[attribution]
# Empty (default): no Co-Authored-By permitted.
# Non-empty glob pattern: bylines matching the pattern are accepted.
non_autonomous = ""

# Empty: autonomous mode is unusable (hook errors at commit time).
# Non-empty glob pattern: commits must carry at least one byline matching it.
autonomous = ""
```

Values are glob patterns interpreted with bash `[[ == ]]` semantics — `*`, `?`, `[...]` work as wildcards; patterns without wildcards degenerate to literal equality. Mockspace carries no default strings, no mapping of agent name → email, no hardcoded list of "known agents". Rationale recorded in session research: the only runtime-discoverable sentinel across agents is `CLAUDECODE=1` (Claude Code); Copilot has none; neither exposes the model version at runtime; no universal AI-commit-attribution standard exists. Autoderiving is impossible without hardcoding, and hardcoding violates opinion-freeness.

**Built-in `check-byline.sh` behavior:**

- Reads `${PROJECT_UPPER}_AGENT_MODE` (default `assistant`).
- In `autonomous` mode: errors if `autonomous` pattern is empty (config error). Errors if no byline. Errors if no byline matches the pattern.
- In `assistant` mode: if `non_autonomous` is empty, rejects any byline (current behavior preserved). If non-empty, rejects any byline that doesn't match the pattern.

**Earlier versions of this note treated attribution as speculative.** That framing was wrong on two counts: the old implementation *did* try to read `claude.json` attribution (broken, silent-failing), and attribution is a real first-class feature that needs a correct implementation. Both corrected now.

### D4 — Cross-project boilerplate becomes opt-in "starter" templates, never auto-injected

Every existing mockspace consumer has near-identical content in a few places (commit-style skill template, writing-style rule, attribution stanza, etc.). Rather than each consumer authoring the same ~50 lines verbatim, mockspace can ship these as named *starters* that a user pulls in via an explicit command:

```bash
cargo mock init --starter commit-style
cargo mock init --starter writing-style
cargo mock init --starter attribution
```

Each starter writes a fresh `.tmpl` into the consumer's `mock/agent/` tree; from that moment the consumer owns the copy and can customise freely. Mockspace never auto-injects content into consumers' authored tree.

**Why this matters:** mockspace must stay opinion-free about content (D5). A built-in that imposes content would violate that. An opt-in scaffold that a user copies in, owns, and edits is the right stance: it reduces boilerplate friction without taking away user authority.

### D5 — Mockspace is opinion-free about project content, opinionated about workflow

This is the framing constraint that all other decisions roll up to. Mockspace does not decide what a consumer's crates should do, what their principles should be, what their agent instructions should say. It *does* decide that:

- Design moves through a finite state machine (`TOPIC → DOC → SRC-PLAN → SRC → DONE`)
- Phase transitions happen via explicit subcommands, not file-renaming by hand
- Closed rounds are archived with metadata and history
- Hooks and agent files are generated from a single canonical authored source
- Git hooks are additive, not replacement

Anything that encodes content — crate names, lint rules specific to a domain, agent rules about a particular framework — belongs in the *consumer*'s mockspace, not in the tool.

### D6 — The README rewrite targets human developers, not agents

Agent-facing guidance belongs in each consumer's `mock/agent/*.tmpl` (which is generated into `.claude/` and `.github/`). The mockspace README is for a human developer evaluating or adopting the tool. It should describe:

- What mockspace is and is not
- The `mock/` layout
- The design-round state machine
- `cargo mock` subcommands
- `mockspace.toml` config
- The `build.rs` bootstrap
- The lint system
- How agent artifact generation works (as one capability among several, not as primary framing)
- Starter templates (once D4 is implemented)

It **does not**:

- Say "agents should do X"
- Reference specific consumer projects by name
- Assume the reader is operating Claude or Copilot or any agent

The previous draft README (currently at `README.md`) is agent-first in framing and cites specific consumer projects. It will be moved to clause-dev's session memory as a mockspace-internals reference and replaced with a proper upstream-worthy README after the D1–D4 refactor lands.

## Scope of this change

**Code:**
- `src/render_agent.rs`: remove the `mock/agent/settings/claude.json` reading path (lines currently 508–524) and the `extract_json_string_field` helper (lines currently 578–590).
- No other code changes. Hook generation was already unified; this pass just removes the vestigial non-hook settings reader.

**Documentation:**
- This file (`REFACTOR_NOTES.md`) records the decisions.
- A follow-up writes a proper `README.md` (replacing the current agent-first draft).

**Consumer repos:**
- Out of scope for the mockspace-side change. Consumer-side deletion of now-dead `mock/agent/settings/*.json` files happens per-repo, driven by whoever owns each consumer.

## Non-goals for this pass

- No change to hook runtime semantics.
- No change to the design-round state machine.
- No change to lint system.
- No change to docs generation pipeline.
- No change to `mockspace.toml` schema.
- Starter-template machinery (D4) is *designed* here but *implemented* separately.
