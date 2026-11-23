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
    "testing",
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
        "- `{}::` -> the registry\n",
        crate::registry::REGISTRY_ROOT
    ));
    roots_doc.push_str(&format!(
        "- `{}::<crate>` -> a crate, linked to its generated document; short or full name\n",
        crate::registry::CRATE_ROOT
    ));
    for ns in crate::registry::BUILTIN_NAMESPACES {
        roots_doc.push_str(&format!(
            "- `{ns}::<slug>` -> the builtin `{ns}` namespace\n"
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
                "## Reference syntax\n\n- Write `{{{{ root::step::step }}}}`, a path in the letkajenkka standard, in any `*.md.tmpl` and in a registry field declared to hold one. Braces required; code fences are left alone. A reference pointing nowhere is reported.\n- A citation is `root::path::anchor`; anchor on a heading (`the_four_lanes`), not a line number. Steps are lowercase names with `_`, no hyphen, dot or extension; the host folds `DESIGN.md` to `design`.\n- `{{{{ <ns> }}}}` renders a namespace's table; `{{{{ <ns>::<slug>::<field> }}}}` one field.\n- Rows are snake_case slugs, never numbers.\n- Relate rows through one `ref[]` field: `<ns>::<slug>` is a link, three or more segments a citation (`research::<dir>::<file>::<heading>`). Write a link once; `refsto` and `cargo mock links <ns>::<slug>` derive the other end. Bad links: `unknown-row-reference`, `malformed-row-reference`.\n\n### Roots\n\n{roots_doc}\n### Namespaces\n\n{ns_doc}\nThe `reference-syntax` skill: row-typed fields, `pathof` / `sourcesof` / `refsto`, postfix chains.\n"
            ),
        },
        BuiltinRule {
            name:     "generated-agent-rules".to_string(),
            apply_to: vec![
                "CLAUDE.md".to_string(),
                ".claude/**".to_string(),
                ".github/copilot-instructions.md".to_string(),
                ".github/instructions/**".to_string(),
                ".github/skills/**".to_string(),
                ".github/hooks/**".to_string(),
            ],
            body:     substitute_builtin_vars(
                r#"STOP. `.claude/`, `CLAUDE.md` and `.github/` agent files are generated: edit the templates in `{mock_dir}/agent/`, then run `cargo mock`."#,
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
                r#"STOP. `docs/` is generated: edit the templates in `{mock_dir}/` (root `*.md.tmpl`) or a crate's `DESIGN.md.tmpl` / `README.md.tmpl` in {src_dirs}, then run `cargo mock`.

- In a merge or rebase the `mockspace-generated` driver keeps this side and a regeneration commit follows. After cherry-pick, revert, `git am`, `git checkout -m` or a stash apply, run `cargo mock` once sources are resolved; never pick a side.
- Still refused: modify/delete of a generated file, an octopus merge over differing documents, `-X ours` or `-X theirs` on a hand-written `docs/` file.
- While `mockspace-kept-ours` stands in the git directory, push is refused and the next commit must regenerate.
- The `merging` skill has the rest."#,
                &mock_rel,
                project_name,
                crate_prefix,
            ),
        },
        BuiltinRule {
            name:     "registry-merge".to_string(),
            apply_to: vec![format!("{mock_rel}/registry/**/*.toml")],
            body:     substitute_builtin_vars(
                "Registry rows merge by `[[namespace]]` and `id`: both sides' additions stay; only a row changed two ways conflicts. The `reference-syntax` skill has the rest.",
                &mock_rel,
                project_name,
                crate_prefix,
            ),
        },
        BuiltinRule {
            name:     "mock-workspace".to_string(),
            apply_to: vec![format!("{mock_rel}/**")],
            body:     substitute_builtin_vars(
                r#"## Mock workspace

`{mock_dir}/` is the design source of truth for {project_name}.

Per crate, five documents:
1. `README.md.tmpl`: 3-10 line summary, inserted into root DESIGN.md.
2. `DESIGN.md.tmpl`: the shipping contract; every backticked type name exists in source (lint `design-doc-source-mismatch`).
3. `BACKLOG.md.tmpl`: deferred designs, names unbackticked; promoted into DESIGN.md.tmpl when shipped, never duplicated.
4. `SHAME.md.tmpl`: known gaps; a `## <key>` header with 50+ words silences the finding keyed `<key>`.
5. `DEEPDIVE_*.md.tmpl`: topic deep dives.

- Per-crate documents under {src_dirs} and Rust that declares something change only inside a design round (`design_rounds/`). Root documents, `Cargo.toml` and module-doc-only `.rs` do not.
- A changelist replacing a deprecated one carries `## Comparison to deprecated changelist` (lint `deprecation-comparison`).
- Every crate has `src/lib.rs` and `README.md.tmpl`; `Cargo.toml` dependencies follow the architecture layers.

The `mockup-workflow` skill: gate boundaries and the validation pipeline. The `merging` skill: a conflicting merge or a refused merge commit.
"#,
                &mock_rel,
                project_name,
                crate_prefix,
            ),
        },
        // Benches, sketches and tests: one short card each, loaded everywhere,
        // since the moment each fires (a timing loop, a probe, a test run) is
        // not tied to a path. The procedure is in the skill each one names.
        BuiltinRule {
            name:     "benches".to_string(),
            apply_to: vec!["**".to_string()],
            body:     substitute_builtin_vars(
                r#"## Benches: a number that feeds a decision comes from `{mock_dir}/benches/`

- No timing of your own: no `Instant::now()` loop, criterion, `cargo bench`, or timer in a sketch or test.
- `cargo mock bench add <bench> <arm>`, code in `<bench>/arms/<arm>/src/lib.rs`, config in `<bench>/bench.toml`, then `cargo mock bench run [<bench>...]`.
- The generated driver is the only runner; custom behaviour goes in `{mock_dir}/benches/src/lib.rs` (`pub fn hooks() -> Hooks`).
- Results: `{mock_dir}/benches/results/<bench>/<sweep>_n<point>/<machine class>/<run>/`, committed, never replaced.
- Quote a number with its run directory and profile.
- One function's disassembly as evidence: `cargo mock bench disasm <bench> <arm> <symbol>`, written under `results/<bench>/disasm/` and committed.
- A run builds the arms and the driver in the shared target and removes them once the results are written; `--keep-builds` keeps them for a second run.
- An arm with no path of its own for a target runs whatever that target has, under its own name; that result is the finding and stays.
- Before the first bench: the `benchmarking` skill.
"#,
                &mock_rel,
                project_name,
                crate_prefix,
            ),
        },
        BuiltinRule {
            name:     "sketches".to_string(),
            apply_to: vec!["**".to_string()],
            body:     substitute_builtin_vars(
                r#"## Sketches: a feasibility probe is a committed sketch

- Does it compile, solve, need the gate: `{mock_dir}/research/sketches/<YYYYMMDDHHMM>_<slug>/`, built under the repository's toolchain. Never `/tmp` or a bare `rustc`.
- Its `FINDINGS.md` has `**Outcome:**` `WORKS`, `FAILS WITH <error>` or `INCONCLUSIVE`, with each attempt's compiler output.
- Never deleted; a wrong one is renamed `*.deprecated.md` or superseded by a new sketch citing it. A timer makes it a bench.
- `cargo mock sketches [--open]` lists outcomes; `--open` only the unfinished or timing ones.
- Before the first probe: the `sketching` skill.
"#,
                &mock_rel,
                project_name,
                crate_prefix,
            ),
        },
        BuiltinRule {
            name:     "tests".to_string(),
            apply_to: vec!["**".to_string()],
            body:     substitute_builtin_vars(
                r#"## Tests: `cargo mock test`, never a bare `cargo test` in `{mock_dir}/`

- It runs members, tool crates, bench crates and lints, rerunning only what moved.
- One concern per binary, `tests/<name>.rs` or `tests/<name>/main.rs`; shared helpers in `tests/common/mod.rs` via `mod common;`.
- Fixtures by literal path; `env!`, `std::env` or `file!()` makes the binary rerun on every change.
- Commit `{mock_dir}/test-history/` with the work. `cargo mock test-units` shows the split.
- Flags, heavy tests, config: the `testing` skill.
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

Each tier governs the one below.

- **Canon is the theory**: intent and its reasoning, abstract, as typed rows. Lives at {canon_location}.
{canon_view}- **Design is the spec**: the `*.md.tmpl` in {src_dirs} and siblings; concrete names and snippets go here, not in canon.
- **Code is last**: it transcribes the design and is what gets nuked.

### The mutation order

- Changing code: nothing may appear in code that the design does not say; otherwise it is an undeclared design change wearing the leaf tier's freedom.
- Changing a design: nuke the code under it first, never migrate.
- Changing a canon row: every design that declares it is nuked first, with the code beneath.
- Adding a row nukes nothing: the trigger is invalidation, not editing.
- `cargo mock --nuke` takes source down, `--nuke=docs` designs too; it lists every file, asks (`--y` answers), refuses an uncommitted tree.
- Canon is never deleted, only demoted.

### Two rules that hold now

- Every design must declare the canon it relates to; one serving none has no reason to exist.
- Naming a canon that does not exist is a hard failure, as is depending on a superseded one: at commit, push and build.

The `canon-tiers` skill has the rest.
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
                r#"## A round consumes every topic filed for it

- Every flat topic in `{mock_dir}/design_rounds/` is this round's work; its changelists name each one, with what it changes or why nothing.
- Never stash, delete, move or leave one unmentioned. Later work is re-filed as a fresh flat topic after close.
- In TOPIC, before a changelist, pull in flat topics and archived rounds from every ref this branch cannot see.
- A topic file accretes: one per subject, appended, never edited.
- The `design-round` skill has the rest.
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
            body: "A crate's `README.md.tmpl` is 3-10 lines (purpose, what it depends on, what depends on it), inserted into DESIGN.md via {{crate_summaries}}. Detail goes in `DESIGN.md.tmpl` or `DEEPDIVE_*.md.tmpl`.".to_string(),
    });

    // What a lint written in this repository may read, and through what. The
    // hard error `lints-read-through-the-view` runs over `<mock>/lints/`, so the
    // rule loads where such a file is opened.
    rules.push(BuiltinRule {
            name: "lints-read-through-the-view".to_string(),
            apply_to: vec![format!("{mock_rel}/lints/**")],
            body: "## A lint reads only through `ctx.view()`\n\n- Methods: `text`, `bytes`, `list`, `walk`, `glob`, `exists`, `is_file`, `is_dir`, `rust`, `toml`, `git`, `git_in`, `run`; plus `ctx.source()`, `ctx.tree()`, `ctx.all_sources()`, `ctx.registry()`.\n- Findings are replayed while the view's answers hold, so a read around the view goes stale. The lint `lints-read-through-the-view` refuses `std::fs`, `std::process`, `std::env`, `read_dir`, walker crates and `Path::is_file()` in lint code (not tests or tools).\n- A lint judging the machine, not the tree, calls `view.untraced(reason)` first. Per-item work goes through `view.each(items, judge)`.\n- A lint is `Send + Sync`; tests build a context with `mockspace::testkit`.".to_string(),
    });

    // On by default, from `mock/agent/config.toml`. A generated snapshot of what
    // this repository itself declares: its project tools and the lints it set,
    // with `mock tools` and `mock lints` named as the live answer once the
    // snapshot is older than the last change. What is the same in every repository
    // is left to those commands, because this rule loads at session start
    // and every session there pays for its size. See `crate::tool_catalogue` for
    // why a snapshot rather than only the instruction to run the commands.
    if cfg.agent.catalogues {
        rules.push(BuiltinRule {
            name: "catalogues".to_string(),
            apply_to: vec!["**".to_string()],
            body: super::catalogue::body(cfg, pack),
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
        r#"# Design round

Topics, then a doc changelist, then a source changelist, then execution. No mockspace edit before the doc changelist exists. `cargo mock check` reports the phase.

## Round id

- `cargo mock round-id` once when the round opens; use what it prints in every file. Never type the minute. Form `YYYYMMDDHHMM-tttt` (older rounds: bare `YYYYMMDDHHMM`).
- Merge refused with `round id collision`: abort, switch to the branch to rename, `cargo mock rename-round <id> --auto-commit`, merge again.

## Files

- Open-round files are flat in `design_rounds/`: `{id}_topic.{name}.md`, `{id}_changelist.doc.md` (`.doc.lock.md` locked), `{id}_changelist.src.md` (`.src.lock.md` locked). Never create `design_rounds/<id>/` while open; the `mockspace-write-guard` hook denies it, so write the flat path.
- `cargo mock close` moves the round into `design_rounds/{id}/`, `cargo mock archive` into `{id}-abandoned/`. Closed rounds are frozen; with no round open, `cargo mock close --take <file>` files a flat topic under the newest closed round.
- `lock`, `close`, `archive` with `--auto-commit` commit only the round's files. Do not reset or restage after.

## Steps

1. TOPIC, per topic: search every `.md` in `docs/` for the concepts (prior rounds and deep dives too, with their generation timestamps), then read the mock source. Present what the docs say first, then where source diverges; every decision is the user's. Ask with `AskUserQuestion`, 2-4 options plus "Other", one question per call. Record each decision in the topic file.
2. DOC: commit `{id}_changelist.doc.md` (per crate, per file, section-level), then edit `*.md.tmpl` per it. Source stays blocked.
3. `cargo mock lock` (DOC to DRAFT) freezes the docs. Write and commit `{id}_changelist.src.md` (opens IMPL).
4. IMPL: edit `src/*.rs` per it, `cargo check`, `cargo mock`.
5. `cargo mock lock` (IMPL to CLOSED), then `cargo mock close`. Close takes the changelists, files sharing their id or named by one, and each `--take <file>`; other flat files stay and are listed.

## Rules

- Topic size: under ~300 words, append to its file; over ~2000, split. A topic file accretes; never edit what it records. New file per subject, not per answer.
- No new topic after a changelist is committed: `cargo mock deprecate` returns to TOPIC.
- Lints read the whole working tree, untracked included. Revert disallowed changes before committing.
- Revise by `cargo mock deprecate` (or `cargo mock unlock` for a locked pair), never an addendum; both print the "Comparison to deprecated changelist" section owed.

## A filing disappears in three ways

- A topic on a branch never merged.
- A topic swept into another round's archive: `close` takes only what its changelists name or share an id with; `archive` in TOPIC with no changelists takes every flat file.
- A closed round directory only on an unmerged branch.

Found after TOPIC: deprecate the changelist and return to TOPIC.
"#,
        &mock_rel,
        project_name,
        crate_prefix,
    );

    let mockup_workflow_body = substitute_builtin_vars(
        r#"# Mockup-first workflow

Design, documentation and API changes go through the mock workspace first.

## Applies to

A new trait, type, extension contract or crate; a changed signature; design docs; a feature in a real crate (mock first). A bug fix, a wording change, or code matching the existing mock skips the round.

## Documents

Per crate, in its directory under {src_dirs}: `README.md.tmpl` (3-10 lines, into root DESIGN.md), `DESIGN.md.tmpl` (overview), `DEEPDIVE_<SUBJECT>.md.tmpl` (into `<CRATE>_<SUBJECT>.md`). Root `DESIGN.md.tmpl` holds principles, summaries and links, never domain docs. Agent rules and skills: templates in `agent/`.

## Phases

| Phase | Gate file | Docs | Source |
|-------|-----------|------|--------|
| TOPIC | none | no | scaffolding |
| DOC | `*_changelist.doc.md` | yes | scaffolding |
| DRAFT | `*_changelist.doc.lock.md` | SHAME only | scaffolding |
| IMPL | `*_changelist.src.md` | SHAME only | yes |
| CLOSED | `*_changelist.src.lock.md` | no | scaffolding |

Staged, unstaged and untracked changes all count. Conversation and file layout: the `design-round` skill.

## What the gate covers

- Gated: per-crate `*.md.tmpl` under {src_dirs} (DOC) and `*.rs` that declares something (IMPL).
- Not gated: root `{mock_dir}/*.md.tmpl`, `Cargo.toml`, and scaffolding: a `.rs` of only module docs and inner attributes that HEAD does not hold declaring anything. Cutting a declaring file down to comments is a source change.
- A nuke passes in any phase only whole: `lib.rs` is the `cargo mock --nuke` stub with its marker and no other Rust file is under `src`.
- A new crate: DOC commits its directory, `Cargo.toml`, members entry and a docs-only `src/lib.rs`; `no-empty-crate` passes it while the doc changelist names it. IMPL commits the items.
- A plain `rm` of an untracked file HEAD does not hold passes in any phase; anything else keeps the phase rule.

## Validate and regenerate

- `cargo check` in `{mock_dir}`; the mock must compile.
- `cargo mock`: check, parse, lints, clean `docs/` top level, generate docs and agent files.
- Flags: `--lint-only`, `--generate-only` (`--stage`, `--commit-regenerated`), `--doc-only`, `--scope <crates>`, `--commit`, `--strict`, `--dir <path>`.
- `cargo mock check` is the readiness report; `cargo mock tools` / `cargo mock lints` list everything; `cargo mock help <command>` is one command's help. `cargo mock clean` removes output; `activate` / `deactivate` set `core.hooksPath`.
- Tests: `cargo mock test` (the `testing` skill). Real crates: the `real-code-guard` skill.

## Never

- Edit `docs/`, `.claude/` or `.github/` directly.
- Add to a real crate what the mock does not define, or reason from real code during design.
- Skip `cargo check` or `cargo mock`.
- Write discovered rules into memory, plan files or notes; write them into mockspace templates.
"#,
        &mock_rel,
        project_name,
        crate_prefix,
    );

    let real_code_guard_body = substitute_builtin_vars(
        r#"# Real code guard

Real crates, `examples/` and Rust outside `{mock_dir}/` are untouched until the mockspace is locked; then they match the mock's names, traits and contracts, adding real data structures, logic, platform code and tests.

Before touching real code:
1. The mock crate has the type, trait or contract (`<crate directory>/src/lib.rs` in {src_dirs}).
2. `cargo check` passes in `{mock_dir}/`.
3. `cargo mock` regenerated the docs, with zero lint findings.

When the mock is wrong, stop and run a design round (the `design-round` skill). Never diverge from the mock, and never take an exception.
"#,
        &mock_rel,
        project_name,
        crate_prefix,
    );

    let mut skills = vec![
        BuiltinSkill {
            dir_name: "design-round".to_string(),
            skill_name: "design-round".to_string(),
            skill_description: "Use when starting or running a design round, or designing a new subsystem, feature or architectural change with the user: round id, file layout, phases, topic rules.".to_string(),
            body: design_round_body,
            files: vec![],
        },
        BuiltinSkill {
            dir_name: "mockup-workflow".to_string(),
            skill_name: "mockup-workflow".to_string(),
            skill_description: "Use when changing design, traits, types or crate structure, regenerating docs, or implementing in a real crate (mock first): phases, what the gate covers, `cargo mock` flags.".to_string(),
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
            skill_description: "Use when writing, changing or citing canon, a design or the code beneath, or deciding which tier a document is: where canon lives, supersession, the mutation order.".to_string(),
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
            skill_description: "Use when writing a reference, citation or row-typed registry field, or when one points nowhere: field types, pathof / sourcesof / refsto, postfix chains, registry merges.".to_string(),
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
            skill_description: "Use when a merge or rebase touches test-history/, or a commit or merge-test-history refuses a test-history file.".to_string(),
            body: substitute_builtin_vars(
                include_str!("skills/test-history/SKILL.md"),
                &mock_rel,
                project_name,
                crate_prefix,
            ),
            files: vec![],
        },
        // How `cargo mock test` runs, selects and records, and how tests are laid
        // out so the selection can narrow; the `tests` rule names the moment.
        BuiltinSkill {
            dir_name: "testing".to_string(),
            skill_name: "testing".to_string(),
            skill_description: "Use before running, writing or moving a test: what `cargo mock test` runs and reruns, test layout, heavy tests, flags, `[test]` config.".to_string(),
            body: substitute_builtin_vars(
                include_str!("skills/testing/SKILL.md"),
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
            skill_description: "Use when a merge conflicts, a phase gate refuses a merge commit, two branches appended to a grow-only file, or naming a file in append_only.".to_string(),
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
            skill_description: "Use before changing Rust outside the mock workspace (a real crate, examples/): the mock-first checklist.".to_string(),
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
            skill_description: "Use BEFORE any feasibility probe (does it compile, does the bound solve, is the gate needed): the committed sketch and its FINDINGS.md.".to_string(),
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
            skill_description: "Use BEFORE any timing loop or claim that one thing is faster, cheaper or smaller: the bench harness, its config and the deliverable.".to_string(),
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
            skill_description: "Use when open design questions must be settled with the user before implementation (\"let's discuss the design\", parked decisions): leads the talk, one topic file per settled topic.".to_string(),
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
    // Hard-rooted reminders, said once in the always-loaded `mockspace` rule. Budget enforced by
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

/// Words whose period does not end a sentence.
const ABBREVIATIONS: &[&str] = &["e.g", "i.e", "etc", "vs", "cf"];

/// The first sentence of `text` on one line, at most 80 characters, so a generated rule names
/// what a namespace is without carrying its whole definition; `cargo mock` has the rest.
///
/// Whitespace, newlines included, folds to single spaces. A sentence ends at `.`, `?` or `!`
/// before a space or the end, outside a backtick span and not after an abbreviation. A cut
/// falls on a word and never inside a backtick span: it backs up before the span, or where the
/// first word alone is longer than the limit, keeps that word whole.
fn short_sentence(text: &str) -> String {
    const LIMIT: usize = 80;
    let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = folded.chars().collect();
    let mut ticks = 0usize;
    let mut end = chars.len();
    for (i, c) in chars.iter().enumerate() {
        if *c == '`' {
            ticks += 1;
        }
        let stops = matches!(c, '.' | '?' | '!') && chars.get(i + 1).is_none_or(|n| *n == ' ');
        if !stops || ticks % 2 == 1 {
            continue;
        }
        let word: String = chars[.. i]
            .iter()
            .rev()
            .take_while(|w| **w != ' ')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if *c == '.' && ABBREVIATIONS.contains(&word.to_lowercase().as_str()) {
            continue;
        }
        end = if *c == '.' { i } else { i + 1 };
        break;
    }
    let sentence = &chars[.. end];
    if sentence.len() <= LIMIT {
        return sentence.iter().collect();
    }
    let first_space = sentence
        .iter()
        .position(|c| *c == ' ')
        .unwrap_or(sentence.len());
    let last_space = sentence[.. LIMIT].iter().rposition(|c| *c == ' ');
    let mut cut = if sentence[LIMIT] == ' ' { LIMIT } else { last_space.unwrap_or(first_space) };
    if sentence[.. cut].iter().filter(|c| **c == '`').count() % 2 == 1 {
        let open = sentence[.. cut]
            .iter()
            .rposition(|c| *c == '`')
            .unwrap_or(0);
        // a span opening the sentence is kept whole, since there is nothing before it to keep
        let close = sentence
            .iter()
            .skip(1)
            .position(|c| *c == '`')
            .map_or(sentence.len(), |p| p + 2);
        cut = if open == 0 { close } else { open };
    }
    let head: String = sentence[.. cut].iter().collect();
    format!("{}...", head.trim_end_matches([' ', ',', ';', ':']))
}

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
        let tmp = mockspace_manifest::scratch::tempdir().unwrap();
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
        let tmp = mockspace_manifest::scratch::tempdir().unwrap();
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

#[cfg(test)]
mod short_sentence_tests {
    use super::short_sentence;

    #[test]
    fn a_sentence_ends_at_a_period_a_question_or_an_exclamation() {
        assert_eq!(
            short_sentence("A short one. And a second sentence."),
            "A short one"
        );
        assert_eq!(short_sentence("Trailing period."), "Trailing period");
        assert_eq!(
            short_sentence("Is it the one? Yes it is."),
            "Is it the one?"
        );
        assert_eq!(short_sentence("Stop! Then go."), "Stop!");
    }

    #[test]
    fn an_abbreviation_does_not_end_the_sentence() {
        assert_eq!(
            short_sentence("Uses e.g. a thing. Second."),
            "Uses e.g. a thing"
        );
        assert_eq!(
            short_sentence("Rows, i.e. slugs, and so on. Next."),
            "Rows, i.e. slugs, and so on"
        );
    }

    #[test]
    fn newlines_fold_to_spaces_so_the_bullet_stays_one_line() {
        let out = short_sentence("first line\nsecond   line\n\nthird");
        assert_eq!(out, "first line second line third");
    }

    #[test]
    fn a_period_inside_a_backtick_span_does_not_end_the_sentence() {
        assert_eq!(
            short_sentence("See `a. b` here. Next one."),
            "See `a. b` here"
        );
    }

    #[test]
    fn a_cut_never_leaves_a_backtick_span_open() {
        let text = format!(
            "{}`aaa bbb ccc ddd` and the rest of it goes on",
            "x ".repeat(38)
        );
        let out = short_sentence(&text);
        assert_eq!(out.matches('`').count() % 2, 0, "{out}");
        assert!(out.ends_with("x..."), "{out}");
        assert!(!out.contains("aaa"), "{out}");
    }

    #[test]
    fn a_cut_falls_on_a_word_and_within_the_limit_plus_the_ellipsis() {
        let out = short_sentence(&"word ".repeat(40));
        assert_eq!(out.chars().count(), 79 + 3, "{out}");
        assert!(out.ends_with("word..."), "{out}");
    }

    #[test]
    fn a_word_ending_exactly_on_the_limit_is_kept() {
        let text = format!("{} {} tail", "w".repeat(40), "z".repeat(39));
        let out = short_sentence(&text);
        assert!(out.ends_with(&format!("{}...", "z".repeat(39))), "{out}");
    }

    #[test]
    fn a_single_token_over_the_limit_is_kept_whole() {
        let out = short_sentence(&format!("{} tail", "a".repeat(100)));
        assert_eq!(out, format!("{}...", "a".repeat(100)));
    }
}
