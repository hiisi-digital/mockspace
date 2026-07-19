#![allow(unused_imports)]
use super::*;

/// Generate all builtin agent templates (rules, skills, preamble, postamble).
///
/// These are project-agnostic defaults that every mockspace consumer gets
/// automatically. Consumer templates with the same filename OVERRIDE builtins.
pub(crate) fn generate_builtin_templates(cfg: &Config) -> BuiltinTemplates {
    let mock_rel = cfg
        .mock_dir
        .strip_prefix(&cfg.repo_root)
        .unwrap_or(&cfg.mock_dir)
        .to_string_lossy()
        .to_string();
    let project_name = &cfg.project_name;
    let crate_prefix = &cfg.crate_prefix;

    // --- Builtin Rules ---

    // The reference syntax, with this project's own roots filled in. Generated
    // rather than written as prose so a project reads its actual roots and
    // their actual behaviour, instead of a generic example it has to translate.
    let mut roots_doc = String::new();
    // The reserved roots first. They are constants rather than entries in the
    // roots map, so a project never declares them and they would otherwise be
    // missing from the one document meant to list what is available.
    roots_doc.push_str(&format!(
        "- `{}::` -> the registry itself; see the namespaces below\n",
        crate::registry::REGISTRY_ROOT
    ));
    roots_doc.push_str(&format!(
        "- `{}::<crate>` -> a crate in this workspace, rendered as a link to its generated document. The short name and the full directory name both resolve, since the prefix is stable\n",
        crate::registry::CRATE_ROOT
    ));
    for ns in crate::registry::BUILTIN_NAMESPACES {
        roots_doc.push_str(&format!(
            "- `{ns}::<slug>` -> the builtin `{ns}` namespace, addressed directly rather than through `reg::`\n"
        ));
    }
    let mut names: Vec<&String> = cfg.registry_roots.keys().collect();
    names.sort();
    for name in names {
        let path = &cfg.registry_roots[name];
        let mut notes = Vec::new();
        if cfg.frozen_roots.contains(name) {
            notes.push("frozen, so line citations into it are honest".to_string());
        }
        if let Some(label) = cfg.prose_roots.get(name) {
            notes.push(format!("renders as prose (\"{label}\"), never a link, so its path stays internal"));
        }
        if cfg.internal_roots.contains(name) {
            notes.push(
                "internal, so citations into it are dropped from generated documents. Still worth \
                 recording: the citation stays in the source, it is checked, and a row citing this \
                 root alongside a public one keeps the public citation"
                    .to_string(),
            );
        }
        let suffix = if notes.is_empty() { String::new() } else { format!(" ({})", notes.join("; ")) };
        roots_doc.push_str(&format!("- `{name}::` -> `{path}`{suffix}\n"));
    }

    let mut ns_doc = String::new();
    for ns in &cfg.registry_namespaces {
        let value = ns
            .value_field
            .as_ref()
            .map(|f| format!(", and a bare reference renders its `{f}`"))
            .unwrap_or_default();
        // Named rather than left to be discovered, because the surprising
        // direction is authoring a field and finding it absent from the table.
        let internal: Vec<&str> = ns
            .fields
            .iter()
            .filter(|f| f.visibility == crate::registry::FieldVisibility::Internal)
            .map(|f| f.name.as_str())
            .collect();
        // Terse, and repeated per namespace only as a list. The sentence
        // explaining what internal means is stated once in the prose below;
        // repeating it on every line would bury the field names it exists to
        // surface.
        let internal_note = if internal.is_empty() {
            String::new()
        } else {
            format!(
                " (internal: {})",
                internal
                    .iter()
                    .map(|n| format!("`{n}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        ns_doc.push_str(&format!(
            "- `{}::<slug>`: {}{}{}\n",
            ns.key,
            ns.description
                .clone()
                .unwrap_or_else(|| ns.title())
                .trim_end_matches('.')
                .to_string(),
            value,
            internal_note
        ));
    }
    if ns_doc.is_empty() {
        ns_doc.push_str("- none declared\n");
    }

    let rules = vec![
        BuiltinRule {
            name: "reference-syntax".to_string(),
            apply_to: vec!["**/*.md".to_string(), "**/*.md.tmpl".to_string(), "**/*.toml".to_string()],
            body: format!(
                "## Reference syntax\n\n                 Write a reference as `{{{{ root::selector }}}}` in any `*.md.tmpl`, and in any registry\n                 field declared to hold one. It resolves when documents are generated, and it is checked:\n                 a reference that points nowhere is reported rather than rendered as something that looks\n                 fine.\n\n                 The braces are required. They make a reference something you state rather than something\n                 the renderer guesses from a pattern, so prose about code is never rewritten by accident.\n                 References inside code fences are left alone.\n\n                 ### Roots in this project\n\n{roots_doc}\n                 A citation is `root::path::anchor`. The anchor is a heading (`#the-four-lanes`) or a line\n                 number. **Prefer the heading.** A line number fails silently: an edit above it shifts the\n                 target, the citation still resolves, and it now points at different content. A heading\n                 fails loudly when renamed, which is a report rather than a lie. Line numbers are honest\n                 only in a root declared frozen.\n\n                 A path may have any depth and the extension may be omitted, so `mock::DESIGN::12` finds\n                 `DESIGN.md.tmpl` without you tracking which form exists where. Two matches is an error\n                 rather than a guess.\n\n                 ### Registry namespaces in this project\n\n{ns_doc}\n                 A namespace is addressed by its own name: `law::keys`, `vocab::xpbd`. There is no\n                 prefix, because slot zero is either a declared root or a declared namespace and the\n                 two cannot collide. The older `reg::law::keys` still resolves.\n\n                 `{{{{ <ns> }}}}` renders that namespace's whole table inline. `{{{{ <ns>::<slug>::<field> }}}}`\n                 renders one field, which is how a document states a value once and every mention of it\n                 stays current instead of drifting into a copy.\n\n                 Two functions answer questions about a thing rather than rendering it.\n                 `{{{{ pathof(x) }}}}` is where x is DECLARED, the file to open to change it: a crate's\n                 directory, a cited file, or the TOML a registry row sits in. `{{{{ sourcesof(x) }}}}` is\n                 what x RESTS ON, its provenance, plural because provenance is an array.\n\n                 A postfix chain narrows the result: `pathof(crates::store).dir()`. Four methods read a\n                 path (`dir`, `filename`, `stem`, `ext`) and three read a list (`first`, `last`,\n                 `count`), applied left to right. An unknown method is reported rather than ignored,\n                 because a method that silently does nothing reads as one that worked.\n\n                 Registry rows are identified by a snake_case slug, never a number. A number carries no\n                 meaning and so has to be managed: never reused, never renumbered, never reordered, since\n                 any of those silently repoints every reference to it.\n"
            ),
        },
        BuiltinRule {
            name: "generated-agent-rules".to_string(),
            apply_to: vec![
                ".claude/**".to_string(),
                ".github/copilot-instructions.md".to_string(),
                ".github/instructions/**".to_string(),
                ".github/skills/**".to_string(),
                ".github/hooks/**".to_string(),
            ],
            body: substitute_builtin_vars(
                r#"STOP. Do NOT edit files in `.claude/` or `.github/copilot-instructions.md`, `.github/instructions/`, `.github/skills/`, `.github/hooks/`. These are AUTO-GENERATED by `cargo mock`. Edit the source templates in `{mock_dir}/agent/` instead, then run `cargo mock` to regenerate."#,
                &mock_rel, project_name, crate_prefix,
            ),
        },
        BuiltinRule {
            name: "generated-docs".to_string(),
            apply_to: vec![
                "docs/*.md".to_string(),
                "docs/*.dot".to_string(),
                "docs/*.png".to_string(),
                "docs/*.svg".to_string(),
            ],
            body: substitute_builtin_vars(
                concat!(
                    "STOP. Do NOT edit any file directly under `docs/`. These are AUTO-GENERATED by `cargo mock`. ",
                    "Edit the source templates in `{mock_dir}/` (root DESIGN.md.tmpl, WORKFLOW.md.tmpl, PRINCIPLES.md.tmpl) ",
                    "or per-crate templates (crates/*/DESIGN.md.tmpl, crates/*/README.md.tmpl), then run `cargo mock` to regenerate.",
                ),
                &mock_rel, project_name, crate_prefix,
            ),
        },
        BuiltinRule {
            name: "generated".to_string(),
            apply_to: vec![
                "CLAUDE.md".to_string(),
                ".claude/rules/*.md".to_string(),
                ".github/copilot-instructions.md".to_string(),
                ".github/instructions/*.md".to_string(),
            ],
            body: substitute_builtin_vars(
                "These files are AUTO-GENERATED from templates in `{mock_dir}/agent/`. Do not edit directly. Run `cargo mock` to regenerate from source templates.",
                &mock_rel, project_name, crate_prefix,
            ),
        },
        BuiltinRule {
            name: "mock-workspace".to_string(),
            apply_to: vec![format!("{mock_rel}/**")],
            body: substitute_builtin_vars(
                concat!(
                    "## Mock workspace rules\n",
                    "\n",
                    "The mock workspace at `{mock_dir}/` is the design source of truth for {project_name}'s target architecture.\n",
                    "\n",
                    "### Five documentation layers per crate\n",
                    "1. **README.md.tmpl** (3-10 lines) -- crate summary, inserted into root DESIGN.md\n",
                    "2. **DESIGN.md.tmpl** -- shipping contract. Every backticked type name here is expected to exist in source; the `design-doc-source-mismatch` lint enforces it.\n",
                    "3. **BACKLOG.md.tmpl** -- designed-but-deferred promissory notes. Decisions made, not yet shipped. Keep names UNBACKTICKED so the design-doc lint ignores them. Promote items into DESIGN.md.tmpl when they ship; don't duplicate.\n",
                    "4. **SHAME.md.tmpl** -- known gaps. Escape hatch for lints: a `## <key>` header with 50+ word explanation silences a lint violation keyed by `<key>` (type name, changelist filename, etc.).\n",
                    "5. **DEEPDIVE_*.md.tmpl** -- topic-specific deep dives.\n",
                    "\n",
                    "### Design rounds\n",
                    "No design modification without a completed design round document in `design_rounds/`. Each round has \"Current state\" and \"Changes proposed\" sections. When the round supersedes a deprecated CL, the active CL must carry a `## Comparison to deprecated changelist` section (enforced by the `deprecation-comparison` lint).\n",
                    "\n",
                    "### What the phase gate actually covers\n",
                    "\n",
                    "The gate protects two things and nothing else: per-crate documents under `crates/*/`, and Rust source. Knowing the boundary saves opening a round for work that never needed one.\n",
                    "\n",
                    "- **Root documents (`mock/*.md.tmpl`: DESIGN, PRINCIPLES, WORKFLOW, and any other) are NOT gated.** Edit them in any phase. They describe the workspace rather than a crate's contract, so they are not what a round exists to keep honest.\n",
                    "- **`Cargo.toml` is NOT gated.** Manifests are wiring: dependency edges are what the layer numbering, the structure graph, and the document ordering all read, and a crate with no manifest contributes none. Only `*.rs` files trip the source gate.\n",
                    "- **A `.rs` file that declares nothing is NOT gated.** A crate root carrying only module documentation and inner attributes is scaffolding, not source. It becomes source the moment it declares anything, and every commit is re-checked, so nothing lands behind a stub.\n",
                    "- **Per-crate `*.md.tmpl` IS gated**, to the DOC phase. **`*.rs` with declarations IS gated**, to IMPL.\n",
                    "\n",
                    "### Validation pipeline\n",
                    "1. `cargo check` -- all mock crates must compile\n",
                    "2. Parse -- tree-sitter extracts pub items from lib.rs\n",
                    "3. Lint -- architecture rules enforced\n",
                    "4. Generate -- docs, graphs, agent rules from templates\n",
                    "\n",
                    "### Rules\n",
                    "- Every crate must have `src/lib.rs` and `README.md.tmpl`\n",
                    "- Dependencies in `Cargo.toml` must match the target architecture layers\n",
                ),
                &mock_rel, project_name, crate_prefix,
            ),
        },
        BuiltinRule {
            name: "readmes".to_string(),
            apply_to: vec![
                "**/README.md".to_string(),
                "**/README.md.tmpl".to_string(),
                format!("{mock_rel}/crates/**/*.md"),
                format!("{mock_rel}/crates/**/*.md.tmpl"),
            ],
            body: "## Per-crate documentation templates\nREADME.md.tmpl files are SHORT (3-10 lines). They contain the crate's purpose, what it depends on, and what depends on it. They are inserted into the generated DESIGN.md via {{crate_summaries}}. Do not put detailed design information here -- that goes in DESIGN.md.tmpl or DEEPDIVE_*.md.tmpl.".to_string(),
        },
    ];

    // --- Builtin Skills ---

    let design_round_body = substitute_builtin_vars(
        concat!(
            "# Design round conversation\n",
            "\n",
            "Design rounds are the only way to change the framework's design. This skill\n",
            "codifies the conversational flow for running a design round with the user.\n",
            "\n",
            "## The flow\n",
            "\n",
            "A design round is a series of topic conversations, followed by a changelist,\n",
            "followed by execution. The conversations happen FIRST. No mockspace edits until\n",
            "the changelist is approved.\n",
            "\n",
            "### Step 1: Topic conversations (TOPIC phase)\n",
            "\n",
            "For each topic in the round:\n",
            "\n",
            "1. **Research.** Gather the complete current state. This means ALL mentions\n",
            "   across ALL docs, not just the immediate or similarly named doc. Search the\n",
            "   entire `docs/` directory for every reference to the types, traits, and\n",
            "   concepts under discussion. Include prior design rounds. Note the\n",
            "   generation timestamp of each doc so we know which ones probably supersede\n",
            "   others. Present older docs too; they may contain context that was lost.\n",
            "\n",
            "   Do not read only the crate's own generated document. Search for the type name, the\n",
            "   trait name across every `.md` file. If a concept appears in\n",
            "   a deep dive, a different crate's overview, DESIGN.md, PRINCIPLES.md, or a\n",
            "   prior design round, that context matters and must be included.\n",
            "\n",
            "   The mock source is also read, but separately. Never present source findings\n",
            "   without first presenting what ALL docs say. The docs may already answer the\n",
            "   question; if they do, say so. If they contradict each other, say so.\n",
            "\n",
            "2. **Present.** Show the user the current state and your analysis. Be thorough.\n",
            "   Include relevant code paths, type signatures, and cross-crate dependencies.\n",
            "   Lead with what the docs say (with timestamps and locations). Then show where\n",
            "   the source diverges. Never frame a doc-answered question as an open design\n",
            "   question. Never skip presenting a finding because it seems obvious; the user\n",
            "   makes all decisions, not the agent.\n",
            "\n",
            "3. **Ask.** Use AskUserQuestion with premade options (2-4 choices) plus always\n",
            "   leave a free-text \"Other\" option. The user may want to write freeform thoughts.\n",
            "   One question at a time. Do not batch questions.\n",
            "\n",
            "4. **Record.** After the user decides, record the decision in the design round\n",
            "   topic file. Move to the next subtopic or next topic.\n",
            "\n",
            "5. **Repeat.** Continue until all topics in the round have decisions recorded.\n",
            "\n",
            "### Step 2: Doc changelist (enters DOC phase)\n",
            "\n",
            "Only after ALL topic conversations are complete:\n",
            "\n",
            "1. Write the doc changelist: `design_rounds/{YYYYMMDDHHMM}_changelist.doc.md`.\n",
            "2. Per-crate, per-file, mechanical. Every doc template to create, rewrite, or update.\n",
            "3. Section-level detail: what to add, remove, keep.\n",
            "4. Commit the changelist. It can be iteratively updated and recommitted -- the\n",
            "   doc changelist is a living document during DOC phase.\n",
            "\n",
            "The doc changelist's existence opens the **DOC phase**: `*.md.tmpl` doc template\n",
            "edits are now allowed. Source changes (`src/*.rs`) are still blocked.\n",
            "\n",
            "### Step 3: Doc-only execution (DOC phase)\n",
            "\n",
            "Apply all doc template changes listed in the doc changelist:\n",
            "\n",
            "1. Edit `*.md.tmpl` templates per the changelist.\n",
            "2. Commit doc changes. This is the only window for doc template edits.\n",
            "3. Update the doc changelist if needed (iterate). Recommit.\n",
            "\n",
            "### Step 4: Lock docs, plan source (DRAFT phase)\n",
            "\n",
            "When all doc changes are applied and the doc changelist is finalized:\n",
            "\n",
            "1. **Lock the doc changelist**: `cargo mock lock`. This renames\n",
            "   `{YYYYMMDDHHMM}_changelist.doc.md` to `{YYYYMMDDHHMM}_changelist.doc.lock.md`\n",
            "   and commits. Doc templates are now frozen.\n",
            "2. Write the **source changelist**: `design_rounds/{YYYYMMDDHHMM}_changelist.src.md`.\n",
            "   This is written against the actual locked doc state. Per-crate, per-file,\n",
            "   mechanical. Every source file to create, modify, or update. Commit it.\n",
            "\n",
            "### Step 5: Source execution (IMPL phase)\n",
            "\n",
            "The source changelist's existence opens the **IMPL phase**:\n",
            "\n",
            "1. Execute source changes (`src/*.rs`) per the source changelist, in subsequent\n",
            "   commits.\n",
            "2. Validate with `cargo check`. Regenerate docs with `cargo mock`.\n",
            "3. Update the source changelist if needed (iterate). Recommit.\n",
            "\n",
            "### Step 6: Lock source (CLOSED phase)\n",
            "\n",
            "When all source changes are applied:\n",
            "\n",
            "1. **Lock the source changelist**: `cargo mock lock`. Round complete.\n",
            "2. **Close the round**: `cargo mock close`. Archives all files.\n",
            "\n",
            "## File layout: flat while active, subdir after close\n",
            "\n",
            "**Every active-round file lives FLAT at the `design_rounds/` root.**\n",
            "Never create `design_rounds/<timestamp>/<filename>`-style paths during\n",
            "an open round; that's the ARCHIVE location for closed rounds only.\n",
            "\n",
            "Active-round paths (flat):\n",
            "\n",
            "```\n",
            "design_rounds/{YYYYMMDDHHMM}_topic.{name}.md\n",
            "design_rounds/{YYYYMMDDHHMM}_changelist.doc.md          (DOC phase)\n",
            "design_rounds/{YYYYMMDDHHMM}_changelist.doc.lock.md     (after lock)\n",
            "design_rounds/{YYYYMMDDHHMM}_changelist.src.md          (IMPL phase)\n",
            "design_rounds/{YYYYMMDDHHMM}_changelist.src.lock.md     (after lock)\n",
            "```\n",
            "\n",
            "`cargo mock close` is the ONLY step that creates a subdirectory. It\n",
            "moves the flat files into `design_rounds/{YYYYMMDDHHMM}/` as part of\n",
            "archiving the round. Once a round is closed, every file inside its\n",
            "subdirectory is frozen.\n",
            "\n",
            "The `mockspace-write-guard` hook treats any path matching\n",
            "`design_rounds/[^/]+/` as a closed-round archive and denies writes\n",
            "to it at HARD_ERROR. If you hit that deny, check the target path:\n",
            "you almost certainly meant `design_rounds/{timestamp}_topic.{name}.md`\n",
            "(flat), not `design_rounds/{timestamp}/{timestamp}_topic.{name}.md`\n",
            "(subdir = archived).\n",
            "\n",
            "This rule is absolute. Do not mkdir under `design_rounds/` during an\n",
            "open round for any reason; the state machine owns that layout.\n",
            "\n",
            "## Rules\n",
            "\n",
            "- All conversation first. No mockspace edits during topic discussions.\n",
            "- **Active rounds live FLAT in `design_rounds/`.** See File layout above.\n",
            "  Subdirectories under `design_rounds/` are ALWAYS archived-round storage,\n",
            "  never active work. The write-guard hook enforces this at HARD_ERROR.\n",
            "- One question at a time. Never batch multiple questions.\n",
            "- Always offer free-text input as an option. The user's thoughts may not fit\n",
            "  premade choices.\n",
            "- Record decisions in the topic file as they are made.\n",
            "- The doc changelist covers ALL topics in the round. It is the doc execution plan.\n",
            "- The src changelist is written after docs are locked, against actual doc state.\n",
            "- Do not skip either changelist. Without them, execution is ad hoc.\n",
            "- **Topic files are frozen once committed.** A committed topic is history. It\n",
            "  cannot be edited, regardless of phase. To add corrections, create a new topic\n",
            "  file (only possible during TOPIC phase, before a changelist exists).\n",
            "- **No new topics after changelist.** Once a changelist is committed, the round\n",
            "  is crystallized. No new topic files can be created. To add new topics,\n",
            "  deprecate the changelist (`cargo mock deprecate`) and return to TOPIC phase.\n",
            "- **Doc changelist is living during DOC phase only.** It can be iteratively\n",
            "  updated and recommitted during doc execution. After locking (`cargo mock lock`),\n",
            "  it is frozen forever.\n",
            "- **Src changelist is living during IMPL phase only.** Same rules apply.\n",
            "- **Phase gates are global.** Lints check the entire working tree, not just staged\n",
            "  files. Untracked or unstaged changes to blocked paths will block commits. Revert\n",
            "  disallowed changes before committing.\n",
            "- **Lock before source.** Never start source changes until the doc changelist is\n",
            "  locked (`cargo mock lock`) and the source changelist is written.\n",
            "- **Deprecation replaces addendums.** If a changelist needs revision, deprecate\n",
            "  it with `cargo mock deprecate` (or `cargo mock unlock` for locked CLs). Write\n",
            "  a new changelist with a \"Comparison to deprecated changelist\" section.\n",
            "- **Full context, always.** Before presenting any finding, search ALL docs for\n",
            "  every mention of the relevant types, traits, and concepts. Do not\n",
            "  read only the immediate crate doc. Include timestamps so the user can judge\n",
            "  which docs supersede others. Present older/superseded docs too for context.\n",
            "- **Always present for the user's decision.** Never decide on your own that\n",
            "  something is \"clearly a code bug\" and skip presentation. Every finding gets\n",
            "  presented. Every decision is the user's. Even if the docs unambiguously\n",
            "  answer a question, present the docs' answer and let the user confirm.\n",
            "- **Docs before source.** Always present what the docs say first. Then show\n",
            "  where the source diverges. If the docs already answer the question, say so\n",
            "  explicitly when presenting. Never frame a doc-answered question as an open\n",
            "  design question.\n",
            "\n",
            "## What this is NOT\n",
            "\n",
            "- This is not brainstorming. Design rounds have structure: current state,\n",
            "  proposed changes, per-crate scope.\n",
            "- This is not implementation planning. The changelist is the plan. There is\n",
            "  no separate \"implementation plan\" step.\n",
            "- This is not a rubber stamp. The user makes the design decisions. You\n",
            "  research, present options, and record.\n",
        ),
        &mock_rel, project_name, crate_prefix,
    );

    let mockup_workflow_body = substitute_builtin_vars(
        concat!(
            "# Mockup-first workflow\n",
            "\n",
            "All design, documentation, and API changes flow through the mock workspace first.\n",
            "This is the only way to change the framework's design. No exceptions.\n",
            "\n",
            "## Documentation structure\n",
            "\n",
            "DOCUMENTATION LIVES WITH ITS DOMAIN CRATE. Three layers:\n",
            "\n",
            "```\n",
            "{mock_dir}/crates/<crate>/\n",
            "  README.md.tmpl                <- SHORTEST (3-10 lines, for root DESIGN.md)\n",
            "  DESIGN.md.tmpl                <- MIDDLE-LENGTH overview (-> the crate's generated doc)\n",
            "  DEEPDIVE_<SUBJECT>.md.tmpl    <- FULL deep dive (-> <CRATE>_<SUBJECT>.md)\n",
            "```\n",
            "\n",
            "The root `DESIGN.md.tmpl` is SHORT: principles + per-crate summaries + links.\n",
            "Domain-specific documentation NEVER goes in root templates.\n",
            "\n",
            "## When this applies\n",
            "\n",
            "- Adding a new trait, type, or extension contract\n",
            "- Changing existing type signatures\n",
            "- Adding a new crate\n",
            "- Updating design documentation\n",
            "- Implementing a feature in a real crate (mock first, then implement)\n",
            "\n",
            "## Steps\n",
            "\n",
            "### 0. Design round (for design changes)\n",
            "\n",
            "If you are changing the framework's design (new types, renamed traits, changed\n",
            "access patterns, new crate, restructured concepts), you MUST create a design\n",
            "round FIRST. See the `design-round` skill for the full conversational flow.\n",
            "\n",
            "The round follows five phases with lint-enforced gates:\n",
            "\n",
            "| Phase | Gate | Docs | Source | CL |\n",
            "|-------|------|------|--------|----|",
            "\n",
            "| TOPIC | no changelists | no | no | n/a |\n",
            "| DOC | `*_changelist.doc.md` | yes | no | doc CL (iterate) |\n",
            "| DRAFT | `*_changelist.doc.lock.md` | no (SHAME ok) | no | src CL creation |\n",
            "| IMPL | `*_changelist.src.md` | no (SHAME ok) | yes | src CL (iterate) |\n",
            "| CLOSED | `*_changelist.src.lock.md` | no | no | no |\n",
            "\n",
            "1. Create topic files in `design_rounds/`. Get them approved. (TOPIC)\n",
            "2. Write doc changelist: `design_rounds/{YYYYMMDDHHMM}_changelist.doc.md`. Commit. (DOC)\n",
            "3. Apply doc template changes per the doc changelist. Commit them. (DOC)\n",
            "4. Lock doc CL: `cargo mock lock`. (DRAFT)\n",
            "5. Write src changelist: `design_rounds/{YYYYMMDDHHMM}_changelist.src.md`. Commit. (IMPL)\n",
            "6. Apply source changes per the src changelist. (IMPL)\n",
            "7. Lock src CL: `cargo mock lock`. (CLOSED)\n",
            "8. Archive: `cargo mock close`.\n",
            "\n",
            "To revise a changelist, deprecate it with `cargo mock deprecate` (unlocked) or\n",
            "`cargo mock unlock` (locked). New CL must include \"Comparison to deprecated\n",
            "changelist\" section.\n",
            "\n",
            "Enforcement is global: lints check the entire working tree (staged + unstaged +\n",
            "untracked). Disallowed changes in any category block commits entirely.\n",
            "\n",
            "If you are only fixing a bug, updating wording, or making a non-design change\n",
            "(implementation in real crates that matches existing mock), skip this step.\n",
            "\n",
            "### 1. Edit the mock workspace\n",
            "\n",
            "```bash\n",
            "cd {mock_dir}\n",
            "```\n",
            "\n",
            "- **Crate design overview**: edit `crates/<crate>/DESIGN.md.tmpl`\n",
            "- **Deep dives**: edit or create `crates/<crate>/DEEPDIVE_<SUBJECT>.md.tmpl`\n",
            "- **Crate summary**: edit `crates/<crate>/README.md.tmpl`\n",
            "- **Types/traits**: edit mock crate sources in `crates/*/src/*.rs`\n",
            "- **Framework-wide docs**: edit root `*.md.tmpl` files\n",
            "- **Agent rules/skills**: edit templates in `agent/` directory\n",
            "- **New crates**: add to `crates/` and `Cargo.toml` members list\n",
            "\n",
            "### 2. Validate\n",
            "\n",
            "```bash\n",
            "cd {mock_dir}\n",
            "cargo check\n",
            "```\n",
            "\n",
            "The mock workspace must compile. Fix errors before proceeding.\n",
            "\n",
            "### 3. Regenerate docs\n",
            "\n",
            "```bash\n",
            "cargo mock\n",
            "```\n",
            "\n",
            "This runs:\n",
            "1. `cargo check` (safety net)\n",
            "2. Lint pass (naming, exports, consistency)\n",
            "3. Clean `docs/` (deletes ALL top-level files; subdirectories untouched)\n",
            "4. Doc generation -> DESIGN.md, STRUCTURE.md, per-crate overviews, deep dives, passthrough templates\n",
            "5. Agent rule generation -> .claude/ (CLAUDE.md, rules, skills, hooks, settings.json) and .github/ (copilot-instructions.md, instructions, skills, hooks)\n",
            "\n",
            "If any step fails, fix the mock and retry.\n",
            "\n",
            "### 4. Real crates (future phase)\n",
            "\n",
            "Real crates do not exist during the design phase. This step applies only after\n",
            "the mockspace is locked and immutable. Until then, your work is done after step 3.\n",
            "\n",
            "When the time comes: match mock signatures in real crate implementations. The\n",
            "mock is the source of truth; real code conforms to it.\n",
            "\n",
            "### 5. Test (future phase)\n",
            "\n",
            "```bash\n",
            "cargo test --workspace\n",
            "cargo clippy --workspace\n",
            "```\n",
            "\n",
            "## What NOT to do\n",
            "\n",
            "- Never edit files in `docs/` directly. They are ALL generated. Changes WILL be lost.\n",
            "- Never edit anything in `.claude/` or `.github/` directly. They are generated from `agent/` templates.\n",
            "- Never add types/traits to real crates without first defining them in the mock.\n",
            "- Never put domain-specific documentation in root `DESIGN.md.tmpl`. Use the crate.\n",
            "- Never skip the `cargo check` validation step.\n",
            "- Never skip the `cargo mock` regeneration step after mock changes.\n",
            "- Never modify doc templates (README.md.tmpl, DESIGN.md.tmpl, DEEPDIVE_*.md.tmpl)\n",
            "  without an unlocked doc changelist in `design_rounds/` (DOC phase).\n",
            "- Never modify source files without a source changelist (`*_changelist.src.md`)\n",
            "  in `design_rounds/` (IMPL phase).\n",
            "- Never create new topic files after a changelist exists. The changelist\n",
            "  crystallizes the round. Deprecate the changelist to return to TOPIC phase.\n",
            "- Never edit a committed topic file. Topics are frozen once committed, always.\n",
            "- Never reference or reason about real crate code during the design phase.\n",
            "  Real crates do not exist. The mock is the only codebase.\n",
            "- Never write discovered rules into memory files, plan files, or session notes.\n",
            "  Write them into mockspace templates (the only persistent documentation).\n",
            "\n",
            "## Generated file headers\n",
            "\n",
            "All generated docs and agent rules start with:\n",
            "\n",
            "```\n",
            "<!--\n",
            "  AUTO-GENERATED: DO NOT EDIT DIRECTLY\n",
            "  Generated by: mockspace (tools/mockspace)\n",
            "  ...\n",
            "-->\n",
            "```\n",
            "\n",
            "If you see this header, do not edit the file.\n",
        ),
        &mock_rel, project_name, crate_prefix,
    );

    let real_code_guard_body = substitute_builtin_vars(
        concat!(
            "# Real code guard\n",
            "\n",
            "**STOP. Real crates do not exist during the design phase.**\n",
            "\n",
            "The mock workspace is the only codebase. Real crates (`crates/` at the repo root)\n",
            "are not maintained, not referenced, and not part of the current workflow. They may\n",
            "contain legacy code; it is irrelevant.\n",
            "\n",
            "No real code is written until the mockspace is locked and immutable. If you are\n",
            "about to modify a file in `crates/`, `examples/`, or any Rust source outside\n",
            "`{mock_dir}/`: stop. You are in the wrong place.\n",
            "\n",
            "## When this skill applies (future phase)\n",
            "\n",
            "After mockspace lockdown, when implementation begins, the mock workspace MUST\n",
            "reflect the intended design. This is non-negotiable.\n",
            "\n",
            "## Documentation structure reminder\n",
            "\n",
            "Documentation lives with its domain crate in the mock workspace:\n",
            "\n",
            "```\n",
            "{mock_dir}/crates/<crate>/\n",
            "  README.md.tmpl                <- shortest summary (inserted into DESIGN.md)\n",
            "  DESIGN.md.tmpl                <- middle-length overview (-> the crate's generated doc)\n",
            "  DEEPDIVE_<SUBJECT>.md.tmpl    <- deep dives (-> <CRATE>_<SUBJECT>.md)\n",
            "```\n",
            "\n",
            "## Checklist before touching real code\n",
            "\n",
            "1. **Mock crate exists?** Check `{mock_dir}/crates/<crate>/src/lib.rs`\n",
            "   has the type, trait, or contract you want to implement.\n",
            "\n",
            "2. **Mock compiles?** Run `cargo check` in `{mock_dir}/`.\n",
            "\n",
            "3. **Docs regenerated?** Run `cargo mock`.\n",
            "   Check generated docs reflect the change.\n",
            "\n",
            "4. **Lints pass?** The `cargo mock` lint pass must complete with zero violations.\n",
            "\n",
            "Only after all four pass should you begin implementing in `crates/`.\n",
            "\n",
            "## What implementation looks like\n",
            "\n",
            "Match the mock signatures. The mock defines the public API contract:\n",
            "- Same type names\n",
            "- Same trait definitions\n",
            "- Same extension contracts\n",
            "\n",
            "The implementation adds:\n",
            "- Real data structures (not `PhantomData` placeholders)\n",
            "- Actual logic (not `todo!()`)\n",
            "- Platform-specific code\n",
            "- Performance optimizations\n",
            "- Tests\n",
            "\n",
            "## What to do if the mock is wrong\n",
            "\n",
            "If the mock has a design flaw discovered during implementation:\n",
            "\n",
            "1. Stop implementing\n",
            "2. Create a design round topic file (`design_rounds/{YYYYMMDDHHMM}_topic.{name}.md`)\n",
            "3. Get the topic approved, write a doc changelist (`*_changelist.doc.md`)\n",
            "4. Apply doc fixes (DOC phase)\n",
            "5. Lock the doc changelist (`cargo mock lock`)\n",
            "6. Write a source changelist (`*_changelist.src.md`) in DRAFT phase\n",
            "7. Apply source fixes (IMPL phase)\n",
            "8. Lock the source changelist (`cargo mock lock`)\n",
            "9. Regenerate docs, then continue implementation\n",
            "\n",
            "Never diverge from the mock \"because it's faster.\" The mock IS the spec.\n",
            "Never modify mock docs without an unlocked doc changelist (DOC phase).\n",
            "Never modify source without a source changelist (IMPL phase).\n",
            "\n",
            "## Exceptions\n",
            "\n",
            "None. If you think you need an exception, you are wrong. Fix the mock.\n",
        ),
        &mock_rel, project_name, crate_prefix,
    );

    let skills = vec![
        BuiltinSkill {
            dir_name: "design-round".to_string(),
            skill_name: "design-round".to_string(),
            skill_description: "Guide for running design round conversations. Use this when starting a new design round, discussing design topics with the user, or when the user asks to brainstorm or design a new subsystem, feature, or architectural change.".to_string(),
            body: design_round_body,
        },
        BuiltinSkill {
            dir_name: "mockup-workflow".to_string(),
            skill_name: "mockup-workflow".to_string(),
            skill_description: "Guide for the mockup-first design and documentation workflow. Use this skill whenever changing framework design, adding or modifying traits, types, crate structure, or regenerating documentation. Also use when asked to implement features in real crates -- the mock must be updated first.".to_string(),
            body: mockup_workflow_body,
        },
        BuiltinSkill {
            dir_name: "real-code-guard".to_string(),
            skill_name: "real-code-guard".to_string(),
            skill_description: "Enforces mock-first workflow when implementing features in real crates. Use this whenever modifying, creating, or refactoring code in the crates/ directory, examples/ directory, or any Rust source file outside the mock workspace. Also use when asked to implement a feature or make a change to the framework.".to_string(),
            body: real_code_guard_body,
        },
    ];

    // --- Builtin Preamble ---
    //
    // Hard-rooted reminders re-stamped on every rule load. Budget enforced by
    // `validate_bookend_size` + the unit test at the bottom of this module.

    let preamble = BUILTIN_PREAMBLE.to_string();

    // --- Builtin Postamble ---

    let postamble = BUILTIN_POSTAMBLE.to_string();

    BuiltinTemplates {
        rules,
        skills,
        preamble,
        postamble,
    }
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

