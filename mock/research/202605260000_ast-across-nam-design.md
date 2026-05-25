# Design: AST across the NAM boundary (wave 4 opener)

**Date:** 2026-05-25
**Status:** Pre-implementation design memo. Resolves the load-bearing question for wave 4 of the lint catalog cdylib port: how does a tree-sitter-walking lint get its parse tree inside a cdylib?
**Scope:** Task #610 wave 4 (per the port-priority memo at `mock/research/202605252300_lint-port-priority-second-wave.md`). Decides which option carries bucket-2 lints (`no_todo` tree-sitter variant, `export_count`, `no_empty_crate`) across the cdylib boundary.
**Source artefacts:**
- `mock/research/202605252300_lint-port-priority-second-wave.md` (port-priority memo; flags this as the wave-4 opener).
- `mock/research/202605252000_cdylib-buffer-ownership-design.md` (PR #213, open at op): the v1 buffer-ownership question; this memo's answer is independent of it.
- `viola/mock/crates/viola-plugin-abi/src/nam.rs` (the NAM v1.0.0 schema this memo proposes extending).
- `mockspace-rs/src/builtins/ast_node_position.rs:117-145` (the in-process v2 reference impl; reads `doc.tree_sitter()` to get the pre-parsed tree).
- `lint-rules/src/no_todo.rs:31` (v1 tree-sitter variant; walks `macro_invocation` nodes).
- `lint-rules/src/export_count.rs:37` (v1 tree-sitter variant; counts pub-export nodes).
- `lint-rules/src/no_empty_crate.rs:49` (v1 tree-sitter variant; checks substantive content).

## The question

Wave 4 ports tree-sitter-walking lints to the cdylib boundary. The bucket-2 lints all currently read a pre-parsed `tree_sitter::Tree` via `MockspaceDocument::tree_sitter()`. That accessor exists in-process; the cdylib boundary at viola-plugin-abi v1 does not currently carry parse trees in NAM. Three options answer the gap.

## Three options

### Option (a): NAM v1.x ships a parse-tree pointer; cdylibs walk via tree-sitter

NAM v1.0.0's `NamFileEntry` gains an opaque `tree_ptr: *const c_void` field (or a sibling-versioned schema `NamFileEntry` at v1.1.0 with the extension). The pointer references a host-owned `tree_sitter::Tree` (specifically the underlying `TSTree*` from the tree-sitter C library). Cdylibs statically link tree-sitter, cast the pointer to a `tree_sitter::Tree` borrow, and walk it the same way in-process lints do.

**What this requires**:
- A NAM schema version bump (v1.0.0 to v1.1.0) for the new field. The accessor function `nam_file_entries` either extends to read the new field, or a sibling `nam_file_entries_v1_1` ships alongside it.
- ABI compatibility between the host's tree-sitter version and the cdylib's tree-sitter version. Tree-sitter ships ABI compatibility via the language-parser binary version field; the host and cdylib must agree on which language ABI they target (currently tree-sitter ABI 14 across the workspace).
- Documentation pinning the tree-sitter version dependency: any cdylib targeting v1.1.0 NAM must link a tree-sitter version compatible with the host's.

**Cost**: zero re-parsing across cdylibs (host parses once, all consumers walk the same tree). The cdylib body shape stays close to the in-process body. ABI version-pinning friction is real but solvable for first-party cdylibs that ship from this workspace.

**Risk**: third-party cdylibs face the version-pinning friction without a workspace-wide build process to enforce it. If a third-party cdylib links a different tree-sitter version, walking the host's tree may either work (cross-version ABI is usually stable) or crash undefined-behaviour. The protocol cannot guarantee safety here; it can only document the contract.

### Option (b): NAM v1.x ships a serialised tree representation; cdylibs walk a custom representation

NAM v1.0.0 gains a `nodes: *const NamNode` slice per `NamFileEntry`, where `NamNode` is a `#[repr(C)]` flat record carrying node kind id, parent index, first-child index, span start byte, span end byte, span start row, span end row. Walking is via index arithmetic on the flat array (parent/child links are indices into the same array). The host pre-walks the tree-sitter tree and serialises into this flat representation.

**What this requires**:
- A NAM schema version bump (v1.0.0 to v1.1.0) with the new `nodes` slice.
- A canonical node-kind id table shipped at viola-plugin-abi (or per-language, sub-versioned with the parser). Each tree-sitter node kind (e.g. `macro_invocation`, `function_item`, `struct_item`) gets a stable id (arvo numeric, exact width TBD at sketch time); the cdylib looks up ids from a shared constants table.
- A walking helper in viola-plugin-abi: `nam_node_walk(nodes: *const NamNode, len: USize, root: USize, visitor: fn(NamNode))` or similar. Cdylibs use the helper rather than rolling their own walks.

**Cost**: serialisation overhead on the host once per parse (linear in tree size). Cdylibs do not link tree-sitter (smaller binary size, no version-pinning friction). Walking is via array indexing, cache-friendly. Cross-language cdylibs (a JS-grammar cdylib written in C, say) work without linking tree-sitter at all.

**Risk**: the canonical node-kind id table is a versioning surface. Adding a new node kind to the host's tree-sitter grammar requires either reserving id space or breaking the canonical table. The table needs to ship with the parser binary version; mismatched table+parser-binary versions are a possible failure mode.

### Option (c): cdylibs each statically link tree-sitter and re-parse

NAM v1.0.0 stays as-is (source bytes only). Each cdylib that needs an AST calls tree-sitter directly inside its boundary: invokes the parser, walks the resulting tree, drops the tree at end of `evaluate`. Multiple cdylibs scanning the same source file each parse independently.

**What this requires**:
- No NAM schema change.
- Each cdylib's `Cargo.toml` lists tree-sitter as a dependency. The cdylib carries the parser binary for whichever language it targets.
- Each cdylib picks its own tree-sitter version. The protocol does not constrain it.

**Cost**: duplicated parse cost. If four bucket-2 lints scan the same file, four parses happen. Tree-sitter parsing is fast (typically sub-millisecond per file), but the cost scales with cdylib count. Binary size grows: tree-sitter + a language grammar adds typically 200-400KB per cdylib.

**Risk**: lowest of the three. No version coordination across protocol participants. Each cdylib is self-contained. The trade is wall-clock cost (parse repeated N times per file across N cdylibs) and total disk size.

## Recommendation: Option (b)

Option (b) is the right answer for the workspace's evolution profile.

The decisive case is what happens when third-party cdylibs ship. Option (a) requires version-pinning a Rust-side dependency across the boundary; for third-party cdylibs that may be written in C, Zig, or against an older tree-sitter version, this either fails compilation or silently corrupts memory. Option (b) decouples the boundary from the parser implementation: the host parses with whatever tree-sitter version it chooses, the cdylib walks a canonical wire format that has its own stability contract. The version-coordination problem shrinks from "tree-sitter ABI compat" to "NAM schema compat", which is what the workspace already manages.

Option (c) is simpler today but accumulates ongoing cost: every additional bucket-2 lint re-parses every source file. At the catalogue's eventual scale (16 mockspace built-ins plus the 17 stack-lints, several of which are AST-shaped), the re-parse cost becomes the dominant cost of running the lint pass. Option (b) pays a one-time host-side serialisation cost and removes the re-parse-per-cdylib growth.

Option (a) is acceptable as a workspace-internal-only protocol for first-party cdylibs that ship from this workspace under controlled tree-sitter version constraints. It is not the right protocol for the public ABI surface.

**Agent's call**: Option (b). Op confirmation point flagged: the canonical node-kind id table is the load-bearing piece that needs to ship alongside the schema bump; alternatives are per-pack id tables (more flexible, harder for cross-pack lints) or per-parser id tables shipped with each grammar (loosely coupled, requires lookup at every node). The recommendation here is the workspace-canonical table at viola-plugin-abi, versioned alongside NAM.

## What this memo does NOT lock

- The exact `NamNode` field set. The list above (kind id, parent index, first-child index, span byte range, span row range) is a starting point; the wave-4 first-port round can refine after a sketch validates real bucket-2 lints walk it efficiently.
- The canonical node-kind id table's stability rules. Adding a new node kind without breaking consumers needs a reserved-id convention. The wave-4 first-port round picks the rule.
- Whether the `NamPayload` carries the serialised tree alongside source bytes or in a sibling payload. The DOC CL R1 conversation (NAM schema vs parallel vtable) generalises here: same trade-off, same likely answer (extend the schema in place, sibling payloads if a real consumer needs the split).
- Whether bucket-2 lints port as one cdylib (the existing `mockspace-builtin-lints` carrier extended with three more `ProviderEntry` rows) or as separate cdylibs. This is the same per-lint vs per-pack question wave-3 will also surface; resolved at the time of porting, not here.

## Open questions for op

1. **Option (a), (b), or (c)?** (Agent's call: b, on the third-party-cdylib argument and the parse-cost-growth argument.)
2. **If (b): the canonical node-kind id table lives at viola-plugin-abi?** Or per-pack (mockspace-builtin-lints ships its own subset table for the kinds its lints walk; other packs ship theirs). The workspace-canonical table reads cleaner but requires the parser-version-and-table coupling.
3. **If (b): how many languages does the canonical table cover at v1.1.0?** Rust (mandatory; bucket-2 lints all target Rust source). Markdown (for `WritingStyle` and any future content-on-md lints). Other languages defer to schema sub-versions.

## What this memo does NOT do

- Edit `viola-plugin-abi`'s nam.rs. The schema change ships as a viola-side slice once op confirms option (b).
- Implement any bucket-2 lint port. Those ship after the schema bump.
- Address the wave-5 NAM project-scope shape. That is its own design memo (the wave-5 opener), structurally distinct from this.

## See also

- `mock/research/202605252300_lint-port-priority-second-wave.md` (the port-priority memo flagging this question as the wave-4 opener).
- `mock/research/202605252000_cdylib-buffer-ownership-design.md` (PR #213): the wave-1 gating question, op-confirmable on its own axis.
- `viola/mock/crates/viola-plugin-abi/src/nam.rs` (NAM v1.0.0 the schema this memo proposes extending).
- `mockspace-rs/src/builtins/ast_node_position.rs:117-145` (the in-process reference impl shape).
- Workspace tasks #610 (parent), #254 (viola becomes a hilavitkutin app; the eventual WorkUnit reshape).
