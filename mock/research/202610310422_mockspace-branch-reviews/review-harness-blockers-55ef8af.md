Source: kaski channel (coord/kaski of clause-dev), briefs/review-harness-blockers-55ef8af.md, first posted 2026-10-30 20:21 UTC, read 2026-11-15.

# Review of mockspace fix/ticket-verbs-skip-lint-build at 55ef8af: merge after fixes

origin/dev has moved to 784b234; merge it in first.

1. src/custom_lints.rs:208, correctness, blocker. build_and_load calls store::record_owner for scoped builds too, which rewrites <mock>/target/mockspace-lints/shared-dir (store.rs:335) to the one-tool crate. entry/test.rs:234 takes the lints tree from that pointer (generated_dir_of), so `mock check` then `mock ticket-add` then `mock test` runs cargo test on a crate with no lint modules and reports green, the outcome test.rs:226-231 forbids; the same flip mid-run trips rewritten_during (test.rs:347). Fix: a scoped build writes .owner and .used but never the pointer. Test, failing today: whole build, then scoped, then generated_dir_of still names the whole build's directory.
2. src/custom_lints.rs:112 with src/entry/tool.rs:129, correctness. discover_scoped clears the packs, so a local tools/<x> that a [lint-crates] pack also registers runs silently instead of being refused with exit 2 (the stale-local-copy case of dispatchable_tool_names). Fall back to the whole build, or refuse, when a declared pack could register the name; a test naming the case.
3. src/custom_lints/store_tests.rs:568-572: the third assertion compares a pure function with itself; delete it.
4. Sampled laws: dispatch.rs:2205 checks only "check"; iterate help::known_commands() plus the help spellings, each with a tools/<name>/Cargo.toml present. dispatch.rs:2188's control returns None before the filesystem; the real control is --lint-only --commit --scope ticket-add with tools/ticket-add present. store.rs:172 claims every whole build keeps its key; pin it with a golden hex for the None key with packs present.
5. Public surface, blocker: README.md:156 still says a tool is compiled into the same library as the lints; README.md:143 and docs/USAGE_GUIDE.md:523 list the key's inputs without the scope.
6. src/custom_lints.rs:123: without CARGO_TARGET_DIR each scope is its own key directory with its own release target (about 85 MB, cold dependency build per tool, ~28 tools in kaski). Say in USAGE_GUIDE that the speedup rests on a shared target directory.

Unrelated red noted: tests/an_executable_is_written_by_the_one_writer.rs:100, from src/registry_append/tests.rs:100 already at 0030251. File or bump a ticket for it.
