//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// Generate all builtin agent templates (rules, skills, preamble, postamble).
///
/// These are project-agnostic defaults that every mockspace consumer gets
/// automatically. Consumer templates with the same filename OVERRIDE builtins.
/// Every builtin skill directory name, enabled or not.
///
/// The renderer sweeps stale skill directories against this list, so a knob
/// turned off takes back what it once wrote. A name missing here leaves its
/// directory behind forever after the knob flips; the catalogue-sync test
/// below the skill assembly is what keeps this list honest.
pub(crate) const BUILTIN_SKILL_DIRS: &[&str] = &[
    "design-round",
    "mockup-workflow",
    "real-code-guard",
    "sketching",
    "benchmarking",
    "design-talk",
    "canon-tiers",
    "reference-syntax",
    "merging",
    "test-history",
];

pub(crate) fn generate_builtin_templates(cfg: &Config, pack: &LintPack) -> BuiltinTemplates {
    let mock_rel = cfg
        .mock_dir
        .strip_prefix(&cfg.repo_root)
        .unwrap_or(&cfg.mock_dir)
        .to_string_lossy()
        .to_string();
    let project_name = &cfg.project_name;
    let crate_prefix = &cfg.crate_prefix;
    let src_dirs_repo = src_dirs_from_repo(cfg);

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
            notes.push(format!(
                "renders as prose (\"{label}\"), never a link, so its path stays internal"
            ));
        }
        if cfg.internal_roots.contains(name) {
            notes.push(
                "internal, so citations into it are dropped from generated documents. Still worth \
                 recording: the citation stays in the source, it is checked, and a row citing this \
                 root alongside a public one keeps the public citation"
                    .to_string(),
            );
        }
        let suffix = if notes.is_empty() {
            String::new()
        } else {
            format!(" ({})", notes.join("; "))
        };
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
        // The relations, from the declared types. A reader authoring a row needs
        // to know which of its fields take a slug from another namespace, and
        // the alternative to generating it here is a hand-written list that
        // stops matching the config the first time a field is added.
        let relations: Vec<String> = ns
            .fields
            .iter()
            .filter_map(|f| {
                let (target, many) = crate::registry::row_reference_target(&f.r#type)?;
                cfg.registry_namespaces
                    .iter()
                    .any(|n| n.key == target)
                    .then(|| {
                        let card = if many { "slugs" } else { "a slug" };
                        format!("`{}` takes {card} from `{target}`", f.name)
                    })
            })
            .collect();
        let relation_note = if relations.is_empty() {
            String::new()
        } else {
            format!(" ({})", relations.join("; "))
        };
        ns_doc.push_str(&format!(
            "- `{}::<slug>`: {}{}{}{}\n",
            ns.key,
            short_sentence(&ns.description.clone().unwrap_or_else(|| ns.title())),
            value,
            relation_note,
            internal_note
        ));
    }
    if ns_doc.is_empty() {
        ns_doc.push_str("- none declared\n");
    }

    // **Where this project's canon is, from its own config.**
    //
    // `canon_paths` is what `mock check` refuses a write to while a panel
    // is open, so it is the one place that already knows, and until now
    // this rule asserted `<mock>/canon/` regardless. A project whose canon
    // is a typed registry got a rule naming a directory it does not have,
    // two lines from a config that says so, and every agent session loaded
    // it.
    let declared_canon: Vec<String> = cfg.canon_paths.clone();
    // `canon_paths` decides, and nothing else does. A registry namespace is not
    // a canon declaration: a project can hold rows nobody has called canon, and
    // reading a namespace as one guesses at what the project deliberately did
    // not say.
    //
    // An earlier version guessed exactly that, so a project with a namespace
    // and no `canon_paths` was told its canon lives at `<mock>/registry/`,
    // which it had never declared, in the same paragraph that says the
    // declaration is the answer.
    let canon_is_the_reserved_directory = declared_canon
        .iter()
        .any(|p| p.contains("/canon/") || p.trim_end_matches('/').ends_with("/canon"));
    // A project declaring no `canon_paths` has not said where its canon is, and
    // naming a directory at it invents a second address after the first one was
    // wrong. Which consumers those are is a question for the tree rather than
    // for this comment: `rg -L canon_paths <repo>/mockspace.toml` answers it,
    // and a figure written here would be true the day it was typed.
    //
    // The honest answer is to say so. An agent told the project has not
    // declared its canon goes and asks; one told it lives at a path that does
    // not exist goes and looks, finds nothing, and concludes the repository is
    // broken.
    let canon_location = if declared_canon.is_empty() {
        "nowhere this project has declared: it names no `canon_paths`, so there is no canon here \
         to reason from and where it should live is open"
            .to_string()
    } else {
        let named = declared_canon
            .iter()
            .map(|p| format!("`{p}`"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{named}, which this project declares as its canon")
    };
    // The reading surface, named only where the project has one.
    let canon_view = if canon_is_the_reserved_directory {
        format!(
            "- **`{mock_rel}/canon/` is the reading surface, never the authority.** Its `.md.tmpl` documents render the rows. Editing one changes nothing and is overwritten when documents generate; change rows.\n"
        )
    } else {
        String::new()
    };
    // The skill's form of the same, gated on the same fact.
    let canon_view_paragraphs = if canon_is_the_reserved_directory {
        format!(
            "`{mock_rel}/canon/` is the reading surface, not the authority.\n\n\
             - Its `.md.tmpl` documents pull the canon in with `{{{{ <ns> }}}}` (a whole namespace) or `{{{{ <ns>::<slug>::<field> }}}}` (one field).\n\
             - Those documents are a view. Changing what the canon says means changing rows; editing the view changes nothing and is overwritten at the next generation.\n\
             - A canon file is `.md.tmpl` generated from rows. A `.md` there holding canon prose no row backs is the failure this shape prevents."
        )
    } else if declared_canon.is_empty() {
        "There is no reading surface here: this project has not said what a rendered canon would render."
            .to_string()
    } else {
        "This project keeps no rendered canon directory. The rows are the whole of it.".to_string()
    };

    let mut rules = vec![
        BuiltinRule {
            name:     "reference-syntax".to_string(),
            apply_to: vec![
                "**/*.md".to_string(),
                "**/*.md.tmpl".to_string(),
                "**/*.toml".to_string(),
            ],
            body:     format!(
                "## Reference syntax\n\nWrite a reference as `{{{{ root::selector }}}}` in any `*.md.tmpl`, and in any registry field declared to hold one. It resolves when documents are generated, and it is checked: a reference that points nowhere is reported rather than rendered as something that looks fine.\n\n**The braces are required**, so prose about code is never rewritten by accident. References inside code fences are left alone.\n\n**A citation is `root::path::anchor`, and the anchor should be a heading rather than a line number**, which shifts silently.\n\n**`{{{{ <ns> }}}}` renders a namespace's whole table inline**, and `{{{{ <ns>::<slug>::<field> }}}}` renders one field, so a value is stated once.\n\n**Rows are identified by a snake_case slug, never a number**, which would have to be managed and silently repoints every reference when it moves.\n\n**A relation between rows is one field typed `ref[]`**, whichever namespace the other row is in; a field per target namespace is wrong. An item of two segments whose first is a declared namespace is a link, `<ns>::<slug>`; three or more is a citation, `research::<dir>::<file>::#<heading>`. A bad link is `unknown-row-reference` or `malformed-row-reference`. It is written once; `refsto` and `cargo mock links <ns>::<slug>` derive the other end.\n\n### Roots in this project\n\n{roots_doc}\n### Registry namespaces in this project\n\n{ns_doc}\nFires when writing a reference, a citation, or a registry field that holds one.\n\n**Use the `reference-syntax` skill** for fields that hold a row, the three functions that ask about a thing rather than render it, and the postfix chains that narrow a result.\n"
            ),
        },
        BuiltinRule {
            name:     "generated-agent-rules".to_string(),
            apply_to: vec![
                ".claude/**".to_string(),
                ".github/copilot-instructions.md".to_string(),
                ".github/instructions/**".to_string(),
                ".github/skills/**".to_string(),
                ".github/hooks/**".to_string(),
            ],
            body:     substitute_builtin_vars(
                r#"STOP. Do NOT edit files in `.claude/` or `.github/copilot-instructions.md`, `.github/instructions/`, `.github/skills/`, `.github/hooks/`. These are AUTO-GENERATED by `cargo mock`. Edit the source templates in `{mock_dir}/agent/` instead, then run `cargo mock` to regenerate."#,
                &mock_rel,
                project_name,
                crate_prefix,
            ),
        },
        BuiltinRule {
            name:     "generated-docs".to_string(),
            apply_to: vec![
                "docs/*.md".to_string(),
                "docs/*.dot".to_string(),
                "docs/*.png".to_string(),
                "docs/*.svg".to_string(),
            ],
            body:     substitute_builtin_vars(
                r#"STOP. Do NOT edit any file directly under `docs/`. These are AUTO-GENERATED by `cargo mock`.

- Edit the source templates in `{mock_dir}/` (root DESIGN.md.tmpl, WORKFLOW.md.tmpl, PRINCIPLES.md.tmpl) or the `DESIGN.md.tmpl` and `README.md.tmpl` of a crate in {src_dirs}, then run `cargo mock`.
- A generated file does not conflict in `git merge` or rebase: the `mockspace-generated` driver keeps this side's copy and a follow-up regeneration commit follows. Cherry-pick, revert, `git am`, `git checkout -m` and stash application regenerate nothing: run `cargo mock` once the sources are resolved, never pick a side.
- Still stops: modify/delete of a generated file, an octopus merge over differing documents, `-X ours` or `-X theirs` on a hand-written file in `docs/`.
- While a regeneration is owed, `mockspace-kept-ours` under the git directory stands: a push is refused, and the next commit regenerates or, if it cannot, is refused (run `cargo mock`).
- Details: the `merging` skill."#,
                &mock_rel,
                project_name,
                crate_prefix,
            ),
        },
        BuiltinRule {
            name:     "registry-merge".to_string(),
            apply_to: vec![format!("{mock_rel}/registry/**/*.toml")],
            body:     substitute_builtin_vars(
                "Registry rows merge by `[[namespace]]` and `id`: additions on both sides stay, only a row changed two ways conflicts. See the `reference-syntax` skill. Merges: the `merging` skill.",
                &mock_rel,
                project_name,
                crate_prefix,
            ),
        },
        BuiltinRule {
            name:     "generated".to_string(),
            apply_to: vec![
                "CLAUDE.md".to_string(),
                ".claude/rules/*.md".to_string(),
                ".github/copilot-instructions.md".to_string(),
                ".github/instructions/*.md".to_string(),
            ],
            body:     substitute_builtin_vars(
                "These files are AUTO-GENERATED from templates in `{mock_dir}/agent/`. Do not edit directly. Run `cargo mock` to regenerate from source templates.",
                &mock_rel,
                project_name,
                crate_prefix,
            ),
        },
        BuiltinRule {
            name:     "mock-workspace".to_string(),
            apply_to: vec![format!("{mock_rel}/**")],
            body:     substitute_builtin_vars(
                r#"## Mock workspace rules

`{mock_dir}/` is the design source of truth for {project_name}'s target architecture.

### Five documentation layers per crate

1. `README.md.tmpl`: 3-10 lines, the crate summary, inserted into root DESIGN.md.
2. `DESIGN.md.tmpl`: the shipping contract. Every backticked type name in it must exist in source (lint `design-doc-source-mismatch`).
3. `BACKLOG.md.tmpl`: designed-but-deferred decisions. Names UNBACKTICKED so the design-doc lint ignores them. Promote an item into DESIGN.md.tmpl when it ships; do not duplicate.
4. `SHAME.md.tmpl`: known gaps. A `## <key>` header with a 50+ word explanation silences the lint violation keyed by `<key>` (a type name, a changelist filename).
5. `DEEPDIVE_*.md.tmpl`: topic-specific deep dives.

### Design rounds

- No design modification without a completed round in `design_rounds/`; each round has "Current state" and "Changes proposed".
- A changelist superseding a deprecated one carries `## Comparison to deprecated changelist` (lint `deprecation-comparison`).

### Phase gate

- Covers per-crate documents under {src_dirs} and Rust source that declares something. Nothing else: root documents, `Cargo.toml` and a `.rs` file with only module docs need no round.

### Rules

- Every crate has `src/lib.rs` and `README.md.tmpl`.
- `Cargo.toml` dependencies match the target architecture layers.
- A merge that conflicts or a gate refuses: the `merging` skill.

Fires when changing framework design, adding or modifying a type or a crate, or editing anything under `{mock_dir}/`.

**Use the `mockup-workflow` skill** for the gate boundaries and the validation pipeline.
"#,
                &mock_rel,
                project_name,
                crate_prefix,
            ),
        },
        BuiltinRule {
            name:     "bench-and-sketch-discipline".to_string(),
            apply_to: vec![format!("{mock_rel}/benches/**"), format!("{mock_rel}/research/**")],
            body:     substitute_builtin_vars(
                r#"## Benches live in the bench framework, never in sketches

- Any benchmark, and any measurement whose numbers feed a decision, goes in `{mock_dir}/benches/` under the mockspace bench framework: `cargo mock bench init` scaffolds, `cargo mock bench run` drives, `bench.toml` registers benches and sizes, each variant is its own cdylib crate under `variants/` on `mockspace-bench-core` + `mockspace-bench-macro`.
- Sketches (`{mock_dir}/research/sketches/`) check feasibility only: does it compile, does the trait solve, does the shape work, with a WORKS / FAILS / INCONCLUSIVE outcome.
- A sketch that needs a timer, an iteration loop or a performance claim is a bench and moves to `{mock_dir}/benches/`. A question with both halves splits: feasibility is a sketch, measurement is a bench.
- Do not re-roll the framework's pieces in a bench: timing helpers, bump providers and stat collection come from `mockspace-bench-core`.

Fires before writing any timing loop, and before any claim that one thing is faster, cheaper or smaller than another.

Invoke the skill before starting. **Use the `sketching` skill** for feasibility work and **the `benchmarking` skill** for measurement.
"#,
                &mock_rel,
                project_name,
                crate_prefix,
            ),
        },
        BuiltinRule {
            name:     "canon-design-code-chain".to_string(),
            apply_to: vec![
                format!("{mock_rel}/*.md.tmpl"),
                format!("{mock_rel}/research/**"),
                format!("{mock_rel}/design_rounds/**"),
            ]
            .into_iter()
            .chain(src_dirs_repo.iter().flat_map(|d| {
                [format!("{d}/**/*.md.tmpl"), format!("{d}/**/*.rs")]
            }))
            // What the project declared, and nothing where it declared
            // nothing. The other five globs are structural and cover a
            // project either way.
            .chain(declared_canon.clone())
            .collect(),
            body:     {
                let body = substitute_builtin_vars(
                r#"## The canon, design, code chain

Three tiers, each governing the one below.

- **Canon is the theory**: intent, choices made and their reasoning, in the abstract, as typed rows. Lives at {canon_location}.
{canon_view}- **Design is the spec**: the `*.md.tmpl` in {src_dirs} and its siblings. Concrete snippets, namings and backticks belong here, not in the canon. Design rounds sit below the canon and reason from it.
- **Code is last**: mechanical transcription of the design. It is the tier that gets nuked.

### The mutation order

- Changing code: nothing has to be nuked first, but nothing may appear in code that the design does not say. A change introducing something the design is silent on is an undeclared design change wearing the leaf tier's freedom.
- Changing a design: nuke the code under it first, not migrated.
- Changing a canon row: every design that declares it is nuked first, and the code beneath those.
- Adding a row nukes nothing. A purely additive edit nukes nothing: the trigger is invalidation, not editing.
- `cargo mock --nuke` takes the source down; `--nuke=docs` takes the designs and the source under them. It names every file, asks (`--y` answers) and refuses an uncommitted tree.
- Canon is never deleted, only demoted. A superseded row stays referenceable through its provenance.

### Two rules that hold now

- Every design document must declare the canon it relates to. A design serving no canon has no reason to exist.
- Naming a canon that does not exist is a hard failure, and so is depending on one that has been superseded. Both are meant to fail at commit, push and build.

Fires when writing or changing canon, a design, or the code beneath them.

**Use the `canon-tiers` skill** for where the canon lives, the mutation order at length, telling which tier a document is, and what is built as against intended.
"#,
                    &mock_rel, project_name, crate_prefix,
                )
                .replace("{canon_location}", &canon_location)
                .replace("{canon_view}", &canon_view);
                body
            },
        },
    ];
    rules.push(BuiltinRule {
            name: "design-round-consumes-its-inheritance".to_string(),
            apply_to: vec![format!("{mock_rel}/design_rounds/**")],
            body: substitute_builtin_vars(
                r#"## A round consumes what was filed for it, and never sheds it

- Every flat topic in `{mock_dir}/design_rounds/` is this round's work, whoever wrote it and whenever.
- Name every open topic in this round's changelists: what it changes, or why it needs nothing, with the reason.
- Never shed one: do not stash it, delete it, move it to another branch, or close the round leaving it unmentioned. A topic that belongs to later work is re-filed as a fresh flat topic after the round closes.
- Before opening a changelist, check every ref for flat topics and archived round directories this branch cannot see, and pull in what it finds. Do it in TOPIC.
- A topic file is a transcript and accretes: one file per subject, appended to as the round returns to it. Never edit or delete what is recorded.

Fires while a design round is open, and before opening a changelist in one.

**Use the `design-round` skill** for the three ways a filing disappears and how long a topic file should be.
"#,
                &mock_rel, project_name, crate_prefix,
            ),
    });

    rules.push(BuiltinRule {
            name: "readmes".to_string(),
            apply_to: ["**/README.md".to_string(), "**/README.md.tmpl".to_string()]
                .into_iter()
                .chain(src_dirs_repo.iter().flat_map(|d| {
                    [format!("{d}/**/*.md"), format!("{d}/**/*.md.tmpl")]
                }))
                .collect(),
            body: "## Per-crate documentation templates\n- `README.md.tmpl` is 3-10 lines: the crate's purpose, what it depends on, what depends on it.\n- It is inserted into the generated DESIGN.md via {{crate_summaries}}.\n- Detailed design goes in `DESIGN.md.tmpl` or `DEEPDIVE_*.md.tmpl`.".to_string(),
    });

    // On by default, from `mock/agent/config.toml`. A generated snapshot of what
    // this repository itself declares: its project tools and the lints it set,
    // with `mock tools` and `mock lints` named as the live answer once the
    // snapshot is older than the last change. What is the same in every repository
    // is left to those commands, because this rule loads on the first file read
    // and every session there pays for its size. See `crate::tool_catalogue` for
    // why a snapshot rather than only the instruction to run the commands.
    if cfg.agent.catalogues {
        rules.push(BuiltinRule {
            name:     "catalogues".to_string(),
            apply_to: vec!["**".to_string()],
            body:     super::catalogue::body(cfg, pack),
        });
    }

    // Off by default, from `mock/agent/config.toml`. Describes how a panel
    // (several personas working one question) mints and consolidates seats
    // against a formalised inventory, and states the one discipline the
    // mechanism cannot enforce by itself: that a panel never writes canon
    // directly. See `crate::panel` for the whole mechanism, and
    // `crate::entry::check`'s panel-discipline row for the mechanical half of
    // the canon rule.
    if cfg.agent.panel_discipline {
        rules.push(BuiltinRule {
            name: "panel-discipline".to_string(),
            apply_to: vec!["**".to_string()],
            body: "## How a panel works\n\n- A panel is several personas working one question: arguing, converging, proposing. Panels do not write canon.\n- `mock panel seat <slug> <persona> <topic...>` mints the next seat number from the panel's inventory file under a lock. Never count or guess a number. Ninety-nine is the last seat; past it the panel is over: close it or open a new one.\n- Minting refuses once enough seats are minted since the last consolidation (`panel_consolidate_every` in `mockspace.toml`, or the inventory's own cadence) until `mock panel consolidate <slug> <note...>` records what was decided.\n- `mock panel status [slug]` reports whether a panel is open (seats no consolidation covers) and how many seats remain before the next consolidation is due; with no slug, every panel under `<mock>/panel/`.\n- The ledger is `<mock>/panel/<slug>.toml`: every seat and consolidation, in order.\n- Only the coordinating human or agent adds what a panel produced to the canonical registries or documents, and only after the panel consolidated. `mock check` refuses a change touching a configured canon path while any panel is open.".to_string(),
        });
    }

    // --- Builtin Skills ---

    let design_round_body = substitute_builtin_vars(
        r#"# Design round conversation

A design round is topic conversations, then a doc changelist, then a source changelist, then execution. No mockspace edit before the doc changelist exists. `cargo mock check` reports the phase.

## Round id

- Run `cargo mock round-id` once when the round opens and use what it prints for every file of the round. Never type the minute.
- An id is `YYYYMMDDHHMM-tttt`. A bare `YYYYMMDDHHMM` from an older round is still an id.
- Merge refused with `round id collision`: abort the merge, switch to the branch to rename, run `cargo mock rename-round <id> --auto-commit` (renames the directory, the files carrying the id, the `round:` line of `.meta`, the local `round/<id>/*` tags), merge again.

## File layout

- Active-round files live flat in `design_rounds/`. Never create `design_rounds/<id>/...` during an open round.
- Flat paths:
  - `design_rounds/{id}_topic.{name}.md`
  - `design_rounds/{id}_changelist.doc.md`, `{id}_changelist.doc.lock.md` after lock
  - `design_rounds/{id}_changelist.src.md`, `{id}_changelist.src.lock.md` after lock
- `cargo mock close` moves the round's files into `design_rounds/{id}/`; `cargo mock archive` into `design_rounds/{id}-abandoned/`. They are the only steps that create a subdirectory. A closed round's files are frozen, and the one way a file joins one later is `cargo mock close --take <file>` with no round open, which files a flat topic under the newest closed round and never overwrites a file there.
- The `mockspace-write-guard` hook denies writes to `design_rounds/[^/]+/` at HARD_ERROR. On that deny, the intended path is the flat one.
- `lock`, `close` and `archive` with `--auto-commit` commit only the round's own files and bring the index to that commit for those paths. Do not `git reset` or restage after.

## Steps

1. Topic phase, per topic:
   - Research: search every `.md` in `docs/` for every mention of the types, traits and concepts, including prior rounds and deep dives. Note each doc's generation timestamp so the user can judge which supersedes which, and present older and superseded docs too. Then read the mock source, separately.
   - Present: what the docs say first, with timestamps and locations; then where the source diverges. A doc-answered question is never framed as open. Every finding is presented, even one the docs unambiguously answer (present the docs' answer and let the user confirm); every decision is the user's.
   - Ask: `AskUserQuestion`, 2-4 premade options plus free-text "Other", one question per call.
   - Record the decision in the topic file. Repeat until every topic has one.
2. Doc changelist (opens DOC): `design_rounds/{id}_changelist.doc.md`. Per crate, per file, every doc template to create, rewrite or update, with section-level detail. Commit it. It is living during DOC.
3. DOC execution: edit `*.md.tmpl` per the changelist, commit. Source stays blocked.
4. `cargo mock lock` (DOC to DRAFT): renames the doc changelist to `.doc.lock.md` and commits; doc templates are frozen. Write `{id}_changelist.src.md` against the locked docs, per crate, per file, and commit it (opens IMPL).
5. IMPL: edit `src/*.rs` per the source changelist, validate with `cargo check`, regenerate with `cargo mock`. The source changelist is living during IMPL.
6. `cargo mock lock` (IMPL to CLOSED), then `cargo mock close`. Close takes the changelists, the files starting with the id of one, the files one names, and any flat file given as `--take <file>` (repeatable); any other flat file stays and is listed.

## Rules

- Record decisions in the topic file as they are made. The doc changelist covers every topic of the round.
- Topic size: under about 300 words is a fragment, append it to the file it belongs to; over about 2000 words carries two subjects, split it.
- A topic file is a transcript and accretes: one file per subject, a later section appended when the talk returns to it. Never edit or delete what a committed topic records. Start a new file when the subject changes, not when a question is answered.
- No new topic after a changelist is committed. Deprecate the changelist (`cargo mock deprecate`) to return to TOPIC.
- Phase gates are global: lints read the whole working tree, staged, unstaged and untracked. Revert disallowed changes before committing.
- No source change before the doc changelist is locked and the source changelist is written.
- Deprecation replaces addendums. Revise with `cargo mock deprecate` (or `cargo mock unlock` for a locked pair). The new changelist carries a "Comparison to deprecated changelist" section; `deprecate` and `unlock` print the section it owes with the names to mention. A bare-title stub beside a locked changelist of its kind is retired by `deprecate` in any phase.
- A round is not brainstorming, not an implementation plan (the changelist is the plan), and not a rubber stamp.

## A filing disappears in three ways

- A topic on a branch that never merged. A round branched fresh from trunk cannot see it.
- A topic swept into another round's archive. `close` takes only what its changelists name or share an id with; a topic filed while another round is in flight stays open unless that round names it or carries its id. `archive` with changelists takes the same; in TOPIC with none it takes every flat file and is named by the earliest.
- A closed round's directory present only on an unmerged branch.

If a filing turns up after TOPIC, deprecate the changelist and return to TOPIC.
"#,
        &mock_rel,
        project_name,
        crate_prefix,
    );

    let mockup_workflow_body = substitute_builtin_vars(
        r#"# Mockup-first workflow

Design, documentation and API changes go through the mock workspace first.

## Documentation layout

Documentation lives with its domain crate, in the crate's directory under {src_dirs}:

```
<crate directory>/
  README.md.tmpl                shortest, 3-10 lines, into root DESIGN.md
  DESIGN.md.tmpl                middle-length overview, into the crate's generated doc
  DEEPDIVE_<SUBJECT>.md.tmpl    full deep dive, into <CRATE>_<SUBJECT>.md
```

- Root `DESIGN.md.tmpl` is short: principles, per-crate summaries, links. Domain documentation never goes in root templates.
- Generated files start with the `AUTO-GENERATED: DO NOT EDIT DIRECTLY` header. Do not edit one.

## Applies to

- A new trait, type or extension contract.
- A changed type signature.
- A new crate.
- Updated design documentation.
- A feature in a real crate (mock first).

A bug fix, a wording change, or an implementation matching the existing mock skips the round.

## Phases

| Phase | Gate | Docs | Source | CL |
|-------|------|------|--------|----|
| TOPIC | no changelists | no | no | n/a |
| DOC | `*_changelist.doc.md` | yes | no | doc CL (iterate) |
| DRAFT | `*_changelist.doc.lock.md` | no (SHAME ok) | no | src CL creation |
| IMPL | `*_changelist.src.md` | no (SHAME ok) | yes | src CL (iterate) |
| CLOSED | `*_changelist.src.lock.md` | no | no | no |

Revise a changelist with `cargo mock deprecate` (unlocked) or `cargo mock unlock` (locked); the new one carries "Comparison to deprecated changelist". Enforcement is global: staged, unstaged and untracked changes all count.

The `design-round` skill carries the conversation flow and file layout.

## What the phase gate covers

- Gated: per-crate `*.md.tmpl` under {src_dirs} (DOC phase) and `*.rs` that declares something (IMPL phase).
- Not gated: root `{mock_dir}/*.md.tmpl` (DESIGN, PRINCIPLES, WORKFLOW and others), `Cargo.toml`, and a `.rs` file carrying only module docs and inner attributes. A `.rs` file is gated the moment it declares anything.

## Edit

- Crate overview: `<crate directory>/DESIGN.md.tmpl`.
- Deep dive: `<crate directory>/DEEPDIVE_<SUBJECT>.md.tmpl`.
- Crate summary: `<crate directory>/README.md.tmpl`.
- Types and traits: `<crate directory>/src/*.rs`.
- Framework-wide docs: root `*.md.tmpl`.
- Agent rules and skills: templates in `agent/`.
- New crate: a directory under one of {src_dirs} and an entry in the `Cargo.toml` members list.

## Validate and regenerate

- `cargo check` in `{mock_dir}`; the mock must compile.
- `cargo mock` runs: `cargo check`, parse (tree-sitter pub items of each `lib.rs`), lints, clean of `docs/` top level (subdirectories untouched), document generation (DESIGN.md, STRUCTURE.md, per-crate overviews, deep dives, passthrough templates), agent file generation into `.claude/` and `.github/`.
- Useful flags: `--lint-only`, `--generate-only` (`--stage`, `--commit-regenerated`), `--doc-only`, `--scope <crates>`, `--commit` and `--strict` for the commit and push severities, `--dir <path>`.
- `cargo mock check` is the readiness report (git, phase, build, tests, lints). `cargo mock tools` and `cargo mock lints` list every subcommand, tool and lint live. `cargo mock help <command>` is the full help of one.
- `cargo mock clean` removes generated output. `cargo mock activate` / `deactivate` set and restore `core.hooksPath`.
- `cargo mock --nuke` takes the source down so it can be rewritten from the designs; `--nuke=docs` takes the designs and the source under them. It names every file first and asks (`--y` answers). It refuses a tree with anything uncommitted.

## Real crates

- Real crates do not exist during the design phase; after lockdown they match the mock signatures. The `real-code-guard` skill covers it.

## Test

- `cargo mock test`, not `cargo test`: it also runs the tool, bench and lint trees.
- Members rerun only when their fingerprint moved since their suite last passed. Under nextest, integration-test binaries rerun only when their own files, the library or dependencies moved.
- `--cheap` defers heavy tests that have passed before. `--full` runs every member asked for, every binary. `--no-cache` runs cached heavy tests and left-out binaries too. `--plain` uses `cargo test` and reaches doc tests. `--benches` includes the bench tree; `-p` leaves it out without it.
- Run `--full --no-cache` once at the end of a piece of work and once after merging the trunk.
- `[test]` keys: `heavy_after_secs` (10), `heavy_threads` (1), `timeout_secs` (900), `commit` (warn runs `--cheap`, error also blocks, off and info run nothing), `debug`, `lean`. `[test.opt_level]` maps a member to an opt level (0 to 3, "s", "z").
- Members build without debug info. Symbols only for a member whose tests failed last time, or `--debug=<crate>` / `-p <crate> --debug`. `--no-debug` gives none. `[test] lean = false` leaves the profile alone. Rustflags with `-g` or `-C debuginfo=` give every crate symbols and cannot be overridden.
- Commit `{mock_dir}/test-history/` with the work unless `[test.history]` keeps it on a git ref. Do not hand-edit it. A deleted test or binary is pruned by the next run.
- `cargo mock help test` says what selects and what is cached. Everything after `test` goes to cargo.

## Never

- Edit `docs/`, `.claude/` or `.github/` directly.
- Add a type or trait to a real crate before the mock defines it.
- Put domain documentation in root `DESIGN.md.tmpl`.
- Skip `cargo check` or the `cargo mock` regeneration.
- Modify doc templates without an unlocked doc changelist (DOC phase) or source without a source changelist (IMPL phase).
- Create a topic file after a changelist exists.
- Rewrite or delete what a committed topic records; appending is expected.
- Reason about real crate code during the design phase.
- Write discovered rules into memory files, plan files or session notes; write them into mockspace templates.
"#,
        &mock_rel,
        project_name,
        crate_prefix,
    );

    let real_code_guard_body = substitute_builtin_vars(
        r#"# Real code guard

- Real crates (code outside `{mock_dir}/`), `examples/` and any Rust source outside `{mock_dir}/` are not touched during the design phase. The mock workspace is the only codebase.
- No real code until the mockspace is locked. After lockdown the mock reflects the intended design and the real crates match its signatures: same type names, trait definitions and extension contracts.
- Implementation adds real data structures (no `PhantomData` placeholders), real logic (no `todo!()`), platform code, optimisations and tests.

## Checklist before touching real code

1. The mock crate has the type, trait or contract: `<crate directory>/src/lib.rs` in {src_dirs}.
2. `cargo check` passes in `{mock_dir}/`.
3. `cargo mock` regenerated the docs and they reflect the change.
4. The `cargo mock` lint pass has zero violations.

## When the mock is wrong

Stop implementing and run a design round to fix the mock (`design-round` skill), then `cargo mock` and continue.

## Never

- Diverge from the mock for speed. The mock is the spec.
- Modify mock docs without an unlocked doc changelist.
- Modify mock source without a source changelist.
- Take an exception.
"#,
        &mock_rel,
        project_name,
        crate_prefix,
    );

    let mut skills = vec![
        BuiltinSkill {
            dir_name: "design-round".to_string(),
            skill_name: "design-round".to_string(),
            skill_description: "Guide for running design round conversations. Use this when starting a new design round, discussing design topics with the user, or when the user asks to brainstorm or design a new subsystem, feature, or architectural change.".to_string(),
            body: design_round_body,
            files: vec![],
        },
        BuiltinSkill {
            dir_name: "mockup-workflow".to_string(),
            skill_name: "mockup-workflow".to_string(),
            skill_description: "Guide for the mockup-first design and documentation workflow. Use this skill whenever changing framework design, adding or modifying traits, types, crate structure, or regenerating documentation. Also use when asked to implement features in real crates -- the mock must be updated first.".to_string(),
            body: mockup_workflow_body,
            files: vec![],
        },
        // The reasoning under the `canon-design-code-chain` rule. The rule
        // carries the three tiers, the mutation order and the two standing
        // requirements, which is what a session has to have in front of it; the
        // why, and where the canon actually lives, is fetched when somebody is
        // working on canon rather than carried by everybody who is not.
        BuiltinSkill {
            dir_name: "canon-tiers".to_string(),
            skill_name: "canon-tiers".to_string(),
            skill_description: "Use when writing, changing or citing canon, a design, or the code beneath them, and when deciding which of the three a document is. Covers where the canon lives (typed registry rows, with the canon documents as a rendered view), what supersession means, the mutation order at length, and which parts are built as against intended.".to_string(),
            body: substitute_builtin_vars(
                include_str!("skills/canon-tiers/SKILL.md"),
                &mock_rel,
                project_name,
                crate_prefix,
            )
            // Both tokens are gated on the same fact the rule gates on, which
            // is the whole of what the earlier round got wrong. It fixed the
            // rule and left the skill substituting a directory unconditionally,
            // so a project with no such directory read "this project has
            // declared no canon" in one file and "`<mock>/canon/` is the
            // reading surface" in the other, loaded beside it.
            .replace("{canon_location}", &canon_location)
            .replace("{canon_view_paragraphs}", &canon_view_paragraphs),
            files: vec![],
        },
        // The half of the reference syntax a session needs only once it is
        // writing one. The rule carries the syntax, the heading-over-line-number
        // preference and this project's live roots and namespaces; the field
        // types, the three functions and the postfix chains are fetched.
        BuiltinSkill {
            dir_name: "reference-syntax".to_string(),
            skill_name: "reference-syntax".to_string(),
            skill_description: "Use when writing a reference, a citation, or a registry field that holds a row, and when a reference is reported as pointing nowhere. Covers the field types that hold rows, pathof / sourcesof / refsto and what each answers, the postfix chains that narrow a result, and how a citation is addressed.".to_string(),
            body: substitute_builtin_vars(
                include_str!("skills/reference-syntax/SKILL.md"),
                &mock_rel,
                project_name,
                crate_prefix,
            ),
            files: vec![],
        },
        // How the test history merges and what the commit gate refuses; the rule
        // names the moment, this is fetched when a merge or a commit meets one.
        BuiltinSkill {
            dir_name: "test-history".to_string(),
            skill_name: "test-history".to_string(),
            skill_description: "Use when a merge or rebase touches a file under test-history/, when a commit is refused for nulling a member's green record, or when merge-test-history refuses a file. Covers how the merge driver settles a green, a binary's pass and per-test timings, what it refuses, and what the pre-commit guard accepts.".to_string(),
            body: substitute_builtin_vars(
                include_str!("skills/test-history/SKILL.md"),
                &mock_rel,
                project_name,
                crate_prefix,
            ),
            files: vec![],
        },
        // How merges are automated: the driver for files that only grow, and how a
        // merge commit is judged inside a design round. Fetched when a merge meets one.
        BuiltinSkill {
            dir_name: "merging".to_string(),
            skill_name: "merging".to_string(),
            skill_description: "Use when a merge conflicts or a merge commit is refused by a phase gate, when a file only ever grows (a list of mod lines, a table of strings, a log) and two branches added to it, or when naming a file in append_only. Covers the append merge driver, what it keeps and when it leaves a file to git, and how a merge commit is judged inside a design round.".to_string(),
            body: substitute_builtin_vars(
                include_str!("skills/merging/SKILL.md"),
                &mock_rel,
                project_name,
                crate_prefix,
            ),
            files: vec![],
        },
        BuiltinSkill {
            dir_name: "real-code-guard".to_string(),
            skill_name: "real-code-guard".to_string(),
            skill_description: "Enforces mock-first workflow when implementing features in real crates. Use this whenever modifying, creating, or refactoring code in a real crate, the examples/ directory, or any Rust source file outside the mock workspace. Also use when asked to implement a feature or make a change to the framework.".to_string(),
            body: real_code_guard_body,
            files: vec![],
        },
    ];

    // On by default, from `mock/agent/config.toml`. Sketching and
    // benchmarking are fundamental to mockspace as a harness (the mistakes
    // they prevent are made in the first thirty seconds of a task, by every
    // consumer), so absent keys mean on; the knob exists because no builtin
    // prose lands in a consumer's context without one.
    if cfg.agent.sketching_skill {
        skills.push(BuiltinSkill {
            dir_name: "sketching".to_string(),
            skill_name: "sketching".to_string(),
            skill_description: "Use BEFORE running any feasibility probe: does it compile, does the trait solve, does this shape work, is this feature gate actually needed, what exactly does the compiler say. Covers sketches and spikes. Directs the probe into the mock directory's research/sketches/ with a FINDINGS.md carrying a WORKS / FAILS / INCONCLUSIVE outcome, instead of a throwaway rustc run in a temp directory whose result nobody can re-run and whose toolchain was probably wrong.".to_string(),
            body: substitute_builtin_vars(
                include_str!("skills/sketching/SKILL.md"),
                &mock_rel,
                project_name,
                crate_prefix,
            ),
            files: vec![],
        });
    }
    if cfg.agent.benchmarking_skill {
        skills.push(BuiltinSkill {
            dir_name: "benchmarking".to_string(),
            skill_name: "benchmarking".to_string(),
            skill_description: "Use BEFORE writing any timing loop or making any claim that one thing is faster, cheaper or smaller than another. Any number that feeds a decision comes from the mock directory's benches/ under the mockspace bench framework: per-variant cdylib isolation, a shared realistic workload, calibrated repetition, and a committed CSV plus findings trail. Covers what to measure, how to keep the comparison honest, and what the deliverable is.".to_string(),
            body: substitute_builtin_vars(
                include_str!("skills/benchmarking/SKILL.md"),
                &mock_rel,
                project_name,
                crate_prefix,
            ),
            files: vec![],
        });
    }

    // Opt-in, from `mock/agent/config.toml`. The flow presumes a human
    // answering design questions in the loop, and a skill that leads a
    // conversation nobody is having is noise in the agent's context.
    if cfg.agent.accelerated_interactive_design_talks {
        skills.push(BuiltinSkill {
            dir_name: "design-talk".to_string(),
            skill_name: "design-talk".to_string(),
            skill_description: "Use when open design questions must be resolved before implementation begins (for example \"let's discuss the design\", \"talk it through with me first\", or resolving parked design decisions). Leads the conversation, consumes topics filed for this round from every ref before starting, writes one topic file per topic as each settles, and re-presents everything for one confirmation before any changelist opens.".to_string(),
            body: substitute_builtin_vars(
                include_str!("skills/design-talk/SKILL.md"),
                &mock_rel,
                project_name,
                crate_prefix,
            ),
            files: vec![
                SkillFile {
                    rel_path:   "scripts/consume-strays".to_string(),
                    contents:   include_str!("skills/design-talk/scripts/consume-strays")
                        .to_string(),
                    executable: true,
                },
                SkillFile {
                    rel_path:   "nut.toml".to_string(),
                    contents:   include_str!("skills/design-talk/nut.toml").to_string(),
                    executable: false,
                },
            ],
        });
    }

    // --- Builtin Preamble ---
    //
    // Hard-rooted reminders re-stamped on every rule load. Budget enforced by
    // `validate_bookend_size` + the unit test at the bottom of this module.

    let preamble = BUILTIN_PREAMBLE.to_string();

    // --- Builtin Postamble ---

    let postamble = BUILTIN_POSTAMBLE.to_string();

    // The declared source directories, in the prose that names where crates live.
    // Filled in last, so a rule written anywhere above can name them.
    let src_dirs_prose = if src_dirs_repo.is_empty() {
        "the declared source directories".to_string()
    } else {
        src_dirs_repo
            .iter()
            .map(|d| format!("`{d}/*/`"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    for r in &mut rules {
        r.body = r.body.replace("{src_dirs}", &src_dirs_prose);
    }
    for s in &mut skills {
        s.body = s.body.replace("{src_dirs}", &src_dirs_prose);
    }

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

#[cfg(test)]
mod canon_location_is_derived {
    //! The canon rule reads where the canon is, rather than asserting it.
    //!
    //! It used to name `<mock>/canon/` nine times whatever the project said,
    //! and ship the reserved-directory convention to a project with no such
    //! directory. One consumer declares `canon_paths = ["mock/registry/*.toml"]` two
    //! lines from where the rule was read, and every session there loaded a
    //! rule describing a tree that does not exist.
    use super::*;

    fn rule_for(canon_paths: Vec<String>) -> BuiltinRule {
        let cfg = cfg_for(canon_paths);
        generate_builtin_templates(&cfg, &LintPack::default())
            .rules
            .into_iter()
            .find(|r| r.name == "canon-design-code-chain")
            .expect("the rule is always minted")
    }

    fn cfg_for(canon_paths: Vec<String>) -> crate::config::Config {
        // Through a real config file rather than a hand-built struct, so the
        // test exercises the same `canon_paths` parse a project does.
        let tmp = tempfile::tempdir().unwrap();
        let mock = tmp.path().join("mock");
        std::fs::create_dir_all(&mock).unwrap();
        let declared = if canon_paths.is_empty() {
            String::new()
        } else {
            format!(
                "canon_paths = [{}]\n",
                canon_paths
                    .iter()
                    .map(|p| format!("\"{p}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        // A declared namespace, so the fixture is a project that HAS a canon.
        // Without it every arm here would be testing the case where the project
        // has declared none, which is a different question and has its own arm.
        std::fs::write(
            mock.join("mockspace.toml"),
            format!(
                "project_name = \"probe\"\n{declared}\n[[registry.namespace]]\nkey = \
                 \"ruling\"\ndescription = \"a thing op has stated\"\n"
            ),
        )
        .unwrap();
        let cfg = crate::config::Config::from_dir(&mock);
        assert_eq!(
            cfg.canon_paths, canon_paths,
            "control: the config parsed what we wrote"
        );
        cfg
    }

    #[test]
    fn a_project_declaring_its_own_canon_is_told_where_it_is() {
        let r = rule_for(vec!["mock/registry/*.toml".into()]);
        assert!(r.body.contains("mock/registry/*.toml"), "{}", r.body);
        assert!(
            r.apply_to.iter().any(|p| p == "mock/registry/*.toml"),
            "and the rule applies to it: {:?}",
            r.apply_to
        );
    }

    #[test]
    fn and_is_not_told_about_a_directory_it_does_not_have() {
        // the arm that matters. the tier argument is universal and stays; the
        // reading surface describes a tree this project has none of, and
        // shipping it is the defect.
        let r = rule_for(vec!["mock/registry/*.toml".into()]);
        assert!(
            !r.body.contains("is the reading surface"),
            "the reading surface must not ship to a project without one: {}",
            r.body
        );
        assert!(
            !r.body.contains("/canon/examples/"),
            "the examples convention goes with it: {}",
            r.body
        );

        // **What the universal half is, checked where it actually lives.**
        //
        // The first version asserted only the title and one line of the intro
        // prose, both of which sit *before* the cut point, so they passed while
        // the cut took nine sections and left one. The mutation order went with
        // them, which is the most load-bearing part of this rule, in the one
        // project the change was made for. A law that cannot reach what the
        // code does is not a law.
        for kept in [
            "The canon, design, code chain",
            "Code is last",
            "typed rows",
            "### The mutation order",
            "### Two rules that hold now",
            "canon-tiers",
        ] {
            assert!(
                r.body.contains(kept),
                "`{kept}` is universal and must survive the cut: {}",
                r.body
            );
        }

        // and something was actually cut, else every arm above passes on a
        // body the cut never touched
        let full = rule_for(Vec::new()).body;
        assert!(
            r.body.len() < full.len(),
            "the cut must remove something: {} against {}",
            r.body.len(),
            full.len()
        );
    }

    /// A project with no canon at all is told so, rather than sent to a path.
    ///
    /// `canon_paths` empty was read as "the default directory", so a project
    /// that has declared no canon anywhere got a rule naming `<mock>/registry/`
    /// as where its canon lives and `<mock>/canon/` as where it is read, and
    /// every agent session in such a repository loaded two addresses that
    /// resolve to nothing.
    ///
    /// How many that is at any moment is a question for the tree, not for this
    /// comment: `git show origin/dev:mockspace.toml | grep -c canon_paths` in
    /// each consumer answers it. An earlier version wrote a figure here and
    /// called it measured, and the figure was wrong.
    ///
    /// The builtins are why the predicate is not `is_empty`: `with_builtins`
    /// prepends `vocab` and `reference` to every project, so a check on the
    /// list being non-empty is true everywhere and decides nothing.
    #[test]
    fn a_project_declaring_no_canon_is_sent_nowhere() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = tmp.path().join("mock");
        std::fs::create_dir_all(&mock).unwrap();
        std::fs::write(mock.join("mockspace.toml"), "project_name = \"probe\"\n").unwrap();
        let cfg = crate::config::Config::from_dir(&mock);
        assert!(
            cfg.canon_paths.is_empty(),
            "control: the fixture declares no canon paths"
        );
        assert!(
            !cfg.registry_namespaces.is_empty(),
            "control: the builtins are there, which is why `is_empty` is the wrong test"
        );

        let r = generate_builtin_templates(&cfg, &LintPack::default())
            .rules
            .into_iter()
            .find(|r| r.name == "canon-design-code-chain")
            .expect("the rule is always minted");
        assert!(
            r.body.contains("nowhere this project has declared"),
            "a project with no canon is told it has none: {}",
            r.body
        );
        assert!(
            !r.body.contains("is the reading surface"),
            "and is not sent to a reading surface it does not have: {}",
            r.body
        );
    }

    #[test]
    fn declaring_nothing_names_no_reading_surface() {
        // `canon_paths` decides and a registry namespace does not, so this
        // fixture, which declares a namespace and no canon, is a project that
        // has said nothing about where its canon lives.
        //
        // It read the other way for one round: the namespace was taken as a
        // declaration, so the project was sent to `<mock>/canon/` and to
        // `<mock>/registry/`, neither of which it had named.
        let r = rule_for(Vec::new());
        assert!(!r.body.contains("is the reading surface"), "{}", r.body);
        assert!(
            !r.apply_to.iter().any(|p| p.ends_with("/canon/**")),
            "and the rule does not attach itself to that directory either: {:?}",
            r.apply_to
        );
    }

    #[test]
    fn a_project_whose_canon_is_that_directory_keeps_it_too() {
        // declaring the conventional location explicitly must not lose the
        // reading surface, else the check is on emptiness rather than on
        // location.
        let r = rule_for(vec!["mock/canon/**".into()]);
        assert!(r.body.contains("is the reading surface"), "{}", r.body);
    }

    /// The canon is rows, and the documents beside them are a view.
    ///
    /// The rule said the opposite for as long as it existed: it named a
    /// directory of prose as the canon and said a canon file is `.md` rather
    /// than `.md.tmpl`. Both are pinned here because the old wording is what an
    /// agent will reach for from memory.
    #[test]
    fn the_canon_is_rows_and_the_documents_are_a_view() {
        // A project that does keep the reserved directory, since the second
        // half of this is about what the documents there are.
        let r = rule_for(vec!["mock/canon/**".into()]);
        assert!(
            r.body.contains("typed rows"),
            "the canon is rows: {}",
            r.body
        );
        assert!(
            r.body.contains("never the authority"),
            "and the documents beside them are not: {}",
            r.body
        );
        assert!(
            !r.body.contains("A canon file is `.md`, not `.md.tmpl`"),
            "the old claim must not come back: {}",
            r.body
        );
    }

    /// What the card drops, the skill carries.
    ///
    /// A split that loses a section is worse than no split, and nothing else
    /// notices: the card reads complete either way.
    #[test]
    fn what_the_card_drops_the_skill_carries() {
        let cfg = cfg_for(vec!["mock/canon/**".into()]);
        let t = generate_builtin_templates(&cfg, &LintPack::default());
        let skill = t
            .skills
            .iter()
            .find(|s| s.dir_name == "canon-tiers")
            .expect("the skill ships beside the rule");
        for moved in [
            "The reproduction property",
            "The mutation order, at length",
            "Telling which tier something is",
            "What is built and what is not",
            "Where a superseded canon goes",
        ] {
            assert!(
                skill.body.contains(moved),
                "`{moved}` left the card and must be in the skill: {}",
                skill.body
            );
        }
        // The token substitution reaches the skill too. Without this the skill
        // ships `{mock_dir}` to every reader and nothing else reports it.
        assert!(
            !skill.body.contains("{mock_dir}"),
            "the skill's tokens are substituted: {}",
            skill.body
        );
        assert!(
            !skill.body.contains("{canon_location}"),
            "including the canon location: {}",
            skill.body
        );

        // Substituted is not the same as true, and asserting only the first is
        // what let the skill ship a sentence saying the declared canon is not
        // the authority. `{canon_location}` is a sentence ("`x`, which this
        // project declares as its canon"); the view slot wants a path, and the
        // one put in it was the sentence.
        assert!(
            !skill.body.contains("{canon_view_paragraphs}"),
            "the view paragraphs are substituted too: {}",
            skill.body
        );
        assert!(
            skill.body.contains("/canon/` is the reading surface"),
            "the reading surface is a path, not the canon's own location: {}",
            skill.body
        );
        assert!(
            !skill
                .body
                .contains("which this project declares as its canon is the reading surface"),
            "the canon location must not land in the view slot: {}",
            skill.body
        );
    }

    /// The skill's own reading surface, over all three config shapes.
    ///
    /// The arm above renders one shape, and one shape cannot catch this: the
    /// skill substituted its directory unconditionally, so its section came out
    /// byte-identical whatever the project declared, and an assertion made
    /// against the one shape where that answer is right passes on all three.
    ///
    /// The rule is checked in the same arm because the defect was the two
    /// disagreeing rather than either alone. A project reads both in one
    /// session, and being told it has no canon in one file and sent to
    /// `<mock>/canon/` in the other is worse than either sentence by itself.
    #[test]
    fn the_skill_and_the_rule_answer_the_same_way_on_every_shape() {
        // Three shapes, and two facts per shape rather than one. Checking only
        // the view sentence let a disagreement through: with nothing declared
        // both files said `false` to it while the rule said the project has no
        // canon and the skill said its rows are the whole of it, which is a
        // claim that it has some.
        for (canon_paths, keeps_the_surface, declares_a_canon, shape) in [
            (
                vec!["mock/registry/*.toml".to_string()],
                false,
                true,
                "a registry canon",
            ),
            (
                vec!["mock/canon/**".to_string()],
                true,
                true,
                "the reserved directory",
            ),
            (Vec::new(), false, false, "nothing declared"),
        ] {
            let cfg = cfg_for(canon_paths);
            let t = generate_builtin_templates(&cfg, &LintPack::default());
            let skill = &t
                .skills
                .iter()
                .find(|s| s.dir_name == "canon-tiers")
                .expect("the skill ships beside the rule")
                .body;
            let rule = &t
                .rules
                .iter()
                .find(|r| r.name == "canon-design-code-chain")
                .expect("the rule is always minted")
                .body;
            assert_eq!(
                skill.contains("is the reading surface"),
                keeps_the_surface,
                "the skill is wrong about the reading surface on {shape}: {skill}"
            );
            assert_eq!(
                rule.contains("is the reading surface"),
                keeps_the_surface,
                "the rule is wrong about the reading surface on {shape}: {rule}"
            );
            // The second fact. `nowhere this project has declared` is the
            // sentence both files carry when there is nothing to name, and
            // neither may say anything about rows the project has beside it.
            let nowhere = "nowhere this project has declared";
            assert_eq!(
                !rule.contains(nowhere),
                declares_a_canon,
                "the rule is wrong about whether a canon is declared on {shape}: {rule}"
            );
            assert_eq!(
                !skill.contains(nowhere),
                declares_a_canon,
                "the skill is wrong about whether a canon is declared on {shape}: {skill}"
            );
            assert_eq!(
                skill.contains("The rows are the whole of it"),
                declares_a_canon && !keeps_the_surface,
                "the skill claims rows the project has not declared on {shape}: {skill}"
            );
        }
    }

    /// And the skill names the same place the rule does, rather than a second
    /// address of its own.
    ///
    /// It carried `The canon is typed rows in <mock>/registry/` as a literal,
    /// so a project whose canon is somewhere else was told two different
    /// things by two files generated in the same run.
    #[test]
    fn the_skill_names_the_declared_canon_and_invents_no_second_address() {
        let cfg = cfg_for(vec!["mock/rulings/*.toml".to_string()]);
        let t = generate_builtin_templates(&cfg, &LintPack::default());
        let skill = &t
            .skills
            .iter()
            .find(|s| s.dir_name == "canon-tiers")
            .expect("the skill ships beside the rule")
            .body;
        assert!(
            skill.contains("mock/rulings/*.toml"),
            "the skill names what the project declared: {skill}"
        );
        assert!(
            !skill.contains("typed rows in `mock/registry/`"),
            "and does not name a directory the project never mentioned: {skill}"
        );
    }
}

/// The first sentence of `text`, cut at 80 characters on a word, so a generated rule names
/// what a namespace is without carrying its whole definition; `cargo mock` has the rest.
fn short_sentence(text: &str) -> String {
    let first = text
        .split(". ")
        .next()
        .unwrap_or(text)
        .trim_end_matches('.');
    if first.chars().count() <= 80 {
        return first.to_string();
    }
    let cut: String = first.chars().take(80).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{cut}...")
}

#[cfg(test)]
mod short_sentence_tests {
    use super::short_sentence;

    #[test]
    fn a_description_is_cut_to_its_first_sentence_and_to_eighty_characters() {
        assert_eq!(
            short_sentence("A short one. And a second sentence."),
            "A short one"
        );
        let long = "word ".repeat(40);
        let cut = short_sentence(&long);
        assert!(cut.chars().count() <= 83, "{cut}");
        assert!(cut.ends_with("..."), "{cut}");
        assert!(!cut.contains("wor..."), "cut on a word: {cut}");
    }
}
