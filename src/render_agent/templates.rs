#![allow(unused_imports)]
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
            ns.description
                .clone()
                .unwrap_or_else(|| ns.title())
                .trim_end_matches('.')
                .to_string(),
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
    // The directory convention still applies where nothing else is
    // declared, which is the common case and the default this ships.
    let canon_is_the_reserved_directory = declared_canon.is_empty()
        || declared_canon
            .iter()
            .any(|p| p.contains("/canon/") || p.trim_end_matches('/').ends_with("/canon"));
    let canon_location = if declared_canon.is_empty() {
        format!("`{mock_rel}/canon/`, a directory reserved for it alone")
    } else {
        let named = declared_canon
            .iter()
            .map(|p| format!("`{p}`"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{named}, which this project declares as its canon")
    };

    let mut rules = vec![
        BuiltinRule {
            name: "reference-syntax".to_string(),
            apply_to: vec!["**/*.md".to_string(), "**/*.md.tmpl".to_string(), "**/*.toml".to_string()],
            body: format!(
                "## Reference syntax\n\n                 Write a reference as `{{{{ root::selector }}}}` in any `*.md.tmpl`, and in any registry\n                 field declared to hold one. It resolves when documents are generated, and it is checked:\n                 a reference that points nowhere is reported rather than rendered as something that looks\n                 fine.\n\n                 The braces are required. They make a reference something you state rather than something\n                 the renderer guesses from a pattern, so prose about code is never rewritten by accident.\n                 References inside code fences are left alone.\n\n                 ### Roots in this project\n\n{roots_doc}\n                 A citation is `root::path::anchor`. The anchor is a heading (`#the-four-lanes`) or a line\n                 number. **Prefer the heading.** A line number fails silently: an edit above it shifts the\n                 target, the citation still resolves, and it now points at different content. A heading\n                 fails loudly when renamed, which is a report rather than a lie. Line numbers are honest\n                 only in a root declared frozen.\n\n                 A path may have any depth and the extension may be omitted, so `mock::DESIGN::12` finds\n                 `DESIGN.md.tmpl` without you tracking which form exists where. Two matches is an error\n                 rather than a guess.\n\n                 ### Registry namespaces in this project\n\n{ns_doc}\n                 A namespace is addressed by its own name: `law::keys`, `vocab::xpbd`. There is no\n                 prefix, because slot zero is either a declared root or a declared namespace and the\n                 two cannot collide. The older `reg::law::keys` still resolves.\n\n                 `{{{{ <ns> }}}}` renders that namespace's whole table inline. `{{{{ <ns>::<slug>::<field> }}}}`\n                 renders one field, which is how a document states a value once and every mention of it\n                 stays current instead of drifting into a copy.\n\n                 ### Fields that hold a row\n\n                 A field's declared `type` is either a builtin (`string`, `string[]`, `integer`,\n                 `boolean`, `ref`, `ref[]`) or **the name of a namespace**, and the second form makes\n                 the field hold references to rows in that namespace. `type = \"slot\"` holds one,\n                 `type = \"slot[]\"` holds several.\n\n                 The value is a bare slug, `\"display\"`, never `\"slot::display\"`: the type already says\n                 which namespace, and one thing written two ways is one thing that can disagree with\n                 itself. A slug naming no row is reported, so a relation cannot rot the way a\n                 hand-maintained list does. A type naming neither a builtin nor a namespace is reported\n                 too, rather than quietly becoming a string field that constrains nothing. So is a\n                 target declaring `value_field`, which renders a value rather than a link and so\n                 cannot carry a relation.\n\n                 Three functions answer questions about a thing rather than rendering it.\n                 `{{{{ pathof(x) }}}}` is where x is DECLARED, the file to open to change it: a crate's\n                 directory, a cited file, or the TOML a registry row sits in. `{{{{ sourcesof(x) }}}}` is\n                 what x RESTS ON, its provenance, plural because provenance is an array.\n                 `{{{{ refsto(x) }}}}` is what POINTS AT x, derived from the typed fields above rather\n                 than stored, so nothing has to be kept in step. It is the direction most questions are\n                 asked in, and an empty answer is the finding: nothing answers that row.\n\n                 A postfix chain narrows the result: `pathof(crates::store).dir()`. Four methods read a\n                 path (`dir`, `filename`, `stem`, `ext`) and three read a list (`first`, `last`,\n                 `count`), applied left to right. An unknown method is reported rather than ignored,\n                 because a method that silently does nothing reads as one that worked.\n\n                 Registry rows are identified by a snake_case slug, never a number. A number carries no\n                 meaning and so has to be managed: never reused, never renumbered, never reordered, since\n                 any of those silently repoints every reference to it.\n"
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
            name: "bench-and-sketch-discipline".to_string(),
            apply_to: vec![
                format!("{mock_rel}/benches/**"),
                format!("{mock_rel}/research/**"),
            ],
            body: substitute_builtin_vars(
                concat!(
                    "## Benches live in the bench framework, never in sketches\n",
                    "\n",
                    "Any benchmark, any measurement whose numbers feed a decision, goes in `{mock_dir}/benches/` ",
                    "using the mockspace bench framework to its full potential (`cargo mock bench init` scaffolds it; ",
                    "`cargo mock bench run` drives it; `bench.toml` registers benches and sizes; each variant is its ",
                    "own cdylib crate under `variants/` built on `mockspace-bench-core` + `mockspace-bench-macro`).\n",
                    "\n",
                    "Sketches (`{mock_dir}/research/sketches/`) are for checking simpler things: does it compile, does ",
                    "the trait solve, does the shape work at all, with a WORKS / FAILS / INCONCLUSIVE outcome. Sketches ",
                    "are NOT for benching. The moment a sketch would need a timer, an iteration loop, or a performance ",
                    "claim, it is a bench and it moves to `{mock_dir}/benches/` under the harness. A question with both ",
                    "halves splits: the feasibility half is a sketch, the measurement half is a bench.\n",
                    "\n",
                    "Why the framework and not a hand-rolled timing loop: each variant compiles and loads as its own ",
                    "cdylib, so no accidental LTO or inlining across variants can contaminate a comparison; the harness ",
                    "surrounds the measured call with a shared realistic workload so numbers approximate real calling ",
                    "context instead of an empty loop; warmups, cooldowns, and repeated runs are calibrated by design; ",
                    "and every run emits the CSV + meta + findings artifact trail that makes the result reproducible, ",
                    "auditable, and citable. A bare timing loop in a sketch or a one-off binary has none of that: its ",
                    "numbers are unreproducible one-offs that cannot decide anything. Do not re-roll timing helpers, ",
                    "bump providers, or stat collection inside a bench crate either: the framework is the reusable ",
                    "surface, use it.\n",
                    "\n",
                    "**Invoke the skill before starting, not after.** The `sketching` skill governs feasibility work ",
                    "and the `benchmarking` skill governs measurement, and each carries the conventions, the ",
                    "deliverable format and the failure modes for its side. This is not optional and it is not a ",
                    "reference to consult when stuck: the mistake both prevent is made in the first thirty seconds, ",
                    "when a quick probe in a temp directory or a quick timing loop in a scratch file feels cheaper ",
                    "than doing it properly. It is cheaper, and it produces a number or an answer that nobody can ",
                    "re-run, that was very possibly measured against the wrong toolchain or optimised away entirely, ",
                    "and that a decision then rests on.\n",
                ),
                &mock_rel, project_name, crate_prefix,
            ),
        },
        BuiltinRule {
            name: "canon-design-code-chain".to_string(),
            apply_to: vec![
                format!("{mock_rel}/*.md.tmpl"),
                format!("{mock_rel}/crates/**/*.md.tmpl"),
                format!("{mock_rel}/crates/**/*.rs"),
                format!("{mock_rel}/research/**"),
                format!("{mock_rel}/design_rounds/**"),
            ]
            .into_iter()
            .chain(if declared_canon.is_empty() {
                vec![format!("{mock_rel}/canon/**")]
            } else {
                declared_canon.clone()
            })
            .collect(),
            body: {
                let body = substitute_builtin_vars(
                concat!(
                    "## The canon, design, code chain\n",
                    "\n",
                    "Three tiers, and they govern each other in one direction only.\n",
                    "\n",
                    "**Canon is the theory: the intent, the choices made, and the reasoning behind them, in ",
                    "the abstract.** It governs what the design is and how the design converges. It is not a ",
                    "spec sheet. It lives at {canon_location}. Canon scattered across research notes in ",
                    "whatever shape each round left it is not canon; it is drift with a confident name.\n",
                    "\n",
                    "**Design is the spec.** In this project that is `{mock_dir}/crates/**/*.md.tmpl` and its ",
                    "siblings. A design says what a specific thing is, concretely enough that a competent ",
                    "reader can implement it. This is where concrete snippets, concrete namings, and backticks ",
                    "earn their place; they do not belong in the canon.\n",
                    "\n",
                    "**Code is last, and it is the tier that gets nuked.** Implementation is mechanical: it ",
                    "writes down what the design says, and the design only says what the canon told it to.\n",
                    "\n",
                    "### The shape of mock/canon/\n",
                    "\n",
                    "`{mock_dir}/canon/` mirrors `{mock_dir}/design_rounds/`, with one deliberate difference.\n",
                    "\n",
                    "Anything flat in `{mock_dir}/canon/` is the canon, exactly as anything flat in ",
                    "`{mock_dir}/design_rounds/` is the current round. `{mock_dir}/canon/archive/` is archived: ",
                    "locked, frozen, stale, and deprecated, the same treatment an archived design round gets. ",
                    "`{mock_dir}/canon/examples/` holds worked examples and demos linked from the canon; it is ",
                    "live, not archived.\n",
                    "\n",
                    "A canon file is `.md`, not `.md.tmpl`. This is derived rather than decided: it follows ",
                    "the same parallel as everything else in this section, `{mock_dir}/design_rounds/` files ",
                    "are `.md`, and canon is internal design authority the way a round is, not a template ",
                    "rendered into public docs. `.md.tmpl` exists for the rendered kind. Treat this as the ",
                    "derived answer, not the settled one, until stated otherwise.\n",
                    "\n",
                    "The difference from `{mock_dir}/design_rounds/` is deliberate, and it inverts a rule a ",
                    "reader already knows from that directory: there, any subdirectory means archived. In ",
                    "`{mock_dir}/canon/`, only `archive/` does, because canon needs other formalised ",
                    "subdirectories and `examples/` is the first; more may follow.\n",
                    "\n",
                    "An archived canon may be referenced, from canon itself, from design talks, and from ",
                    "research, as history. It may never be depended on. A design that names something under ",
                    "`archive/` is naming a dead authority, which is exactly what one of the two rules below ",
                    "forbids.\n",
                    "\n",
                    "### The reproduction property\n",
                    "\n",
                    "Nuke the code and lose nothing: the design says what the code was, so the code is a ",
                    "mechanical transcription and nothing more. Nuke the design and lose little: an equivalent ",
                    "design can be written from the canon, and it may differ from the original and still be ",
                    "valid. The canon is the only tier that is not reproducible from anything above it.\n",
                    "\n",
                    "Two acceptance tests follow from this. A design is good enough when two implementers, ",
                    "reading it independently, produce working implementations of the same thing. A canon is ",
                    "good enough when two designers, reading it independently, produce designs that yield ",
                    "equivalent working units.\n",
                    "\n",
                    "### The mutation order\n",
                    "\n",
                    "This is the enforceable part.\n",
                    "\n",
                    "To change the code: nothing has to be nuked first, because code is the leaf and nothing ",
                    "depends on it. That is the only sense in which it is free, and \"just change it\" ",
                    "overstates it into no constraints at all. Two still bind it, and neither belongs to the ",
                    "mutation order. The mockspace round ceremony applies in full: topic, doc changelist, ",
                    "lock, source changelist, lock, close; the phase gates enforce it. And nothing may appear ",
                    "in code that is not in the design; design first, code follows. A change that introduces ",
                    "something the design does not say is not a code change at all. **It is an undeclared ",
                    "design change wearing the leaf tier's freedom**, while actually mutating the tier above, ",
                    "and it is the most common failure here precisely because it does not feel like editing a ",
                    "design at the moment it happens.\n",
                    "\n",
                    "The leaf is unconstrained downward and fully constrained upward. That generalises: each ",
                    "tier is unconstrained toward what depends on it and constrained by what it depends on, ",
                    "the same statement the mutation order makes about the tiers above it, from the other ",
                    "direction.\n",
                    "\n",
                    "To change a design: the code under that design is nuked first, not migrated, not adapted, ",
                    "then rewritten from the changed design.\n",
                    "\n",
                    "To change a canon file: every design that declares that file is nuked first, and ",
                    "therefore the code beneath those designs. **Not every design in the project. Only the ",
                    "declared dependents.** The declaration each design carries (see the two rules below) is ",
                    "what makes that scoping possible; without it, the only honest blast radius would be ",
                    "everything.\n",
                    "\n",
                    "Two consequences follow from the same reasoning, and neither is an exception carved into ",
                    "the rule; both fall out of it. Adding a new canon file nukes nothing, since nothing ",
                    "declares it yet. Appending to an existing canon file also nukes nothing: the trigger is ",
                    "invalidation, not editing, and a purely additive change leaves every prior sentence ",
                    "standing, so a design already derived from that file is still derivable from it as it now ",
                    "reads. A modification or a deletion invalidates, and that file's declared dependents go.\n",
                    "\n",
                    "File granularity is the right unit because one file is one topic: one aspect of the ",
                    "canon, the way a chapter works in academic literature. A file carrying two unrelated ",
                    "topics drags an unrelated design into every nuke it never actually depended on. The fix ",
                    "for that is splitting the file, not refining the granularity below it.\n",
                    "\n",
                    "Canon is never deleted, only demoted. A superseded canon stays referenceable so the next ",
                    "canon can be built with the old one in view. That is the one exception to nuking, because ",
                    "the canon is the only tier carrying reasoning rather than consequence. It demotes the same ",
                    "way a design round closes: moved as a whole into `{mock_dir}/canon/archive/<timestamp>/`, a ",
                    "directory rather than a filename suffix, exactly parallel to ",
                    "`{mock_dir}/design_rounds/<timestamp>/`. That keeps `archive/` the single marker of ",
                    "archived status and lets a whole canon generation move as one unit, the way a round does, ",
                    "so the superseded canon is dated, kept, and marked stale rather than overwritten in ",
                    "place.\n",
                    "\n",
                    "A lower tier that survives a change above it becomes a claim about something that no ",
                    "longer exists. It still gets read, and it still gets defended, because it is concrete and ",
                    "detailed and looks authoritative next to the abstract statement that replaced it.\n",
                    "\n",
                    "### What this means for canon work\n",
                    "\n",
                    "Declaring a canon file's dependents stale is not a quality complaint. It is the ",
                    "precondition the mutation order requires: while a design that declares a canon file is ",
                    "live, that file is frozen, and nuking its declared dependents is what unfreezes it. An ",
                    "agent that consults a live dependent design or its shipped source while editing the canon ",
                    "file it declares is reattaching a tier that had to be detached for the edit to be ",
                    "permitted, and every observation it brings back is a fact about a document already ",
                    "declared dead.\n",
                    "\n",
                    "### Telling which tier a document is\n",
                    "\n",
                    "Ask what it costs to be wrong. If being wrong means the code is wrong, it is a design. If ",
                    "being wrong means every design built on it is wrong, it is canon. If it can be regenerated ",
                    "from the tier above without loss, it is not that tier.\n",
                    "\n",
                    "Ask what it survives. A canon survives a total rewrite of every implementation, in a ",
                    "different style, a different language, a different decade. A design does not, and is not ",
                    "meant to.\n",
                    "\n",
                    "### Two rules that hold now\n",
                    "\n",
                    "Two requirements are rules, not aspiration, whether or not a lint enforces them yet.\n",
                    "\n",
                    "**Every design document must declare the canon it relates to.** A design that serves no ",
                    "canon has no reason to exist: if a piece of the spec answers to nothing in the canon, ",
                    "that is a defect in the design itself, not a missing footnote. A design carrying no such ",
                    "declaration has not merely omitted paperwork; it has not justified itself.\n",
                    "\n",
                    "**Naming a canon that does not exist is a hard failure, and so is naming anything under ",
                    "`{mock_dir}/canon/archive/`.** The first is a design pointing at nothing. The second is a ",
                    "design depending on a dead, deprecated authority, exactly what the archive exists to keep ",
                    "from happening. Both are meant to fail every gate a document passes through: commit, ",
                    "push, and build.\n",
                    "\n",
                    "### What canon files owe each other is not formalised yet\n",
                    "\n",
                    "Canon files are expected, eventually, to declare relationships to one another, with ",
                    "invalidation cascading through those relations the way nuking a canon file already ",
                    "cascades to the designs that declare it. That is anticipated and **not specified**. It ",
                    "stays unspecified on purpose: how it should work in practice is not yet known, and the ",
                    "plan is to dogfood the three-tier shape first, see what actually happens across real ",
                    "canon work, and formalise the relation and cascade mechanism from that experience rather ",
                    "than from guesswork now. Do not invent a relation or cascade mechanism ahead of that. And ",
                    "do not read its absence as meaning canon files are independent of one another; absence ",
                    "here means undecided, not decided-independent.\n",
                    "\n",
                    "### No tooling enforces any of this yet\n",
                    "\n",
                    "`{mock_dir}/canon/` is reserved so every rule above has an address to hold itself ",
                    "against, not because any of them are mechanically enforced today. Three things remain ",
                    "unbuilt: the mutation-order guard (refusing an edit to a canon file outside `archive/` ",
                    "while any design that declares it remains under `{mock_dir}/crates/`), the ",
                    "design-declares-canon rule, and the failure on naming a missing or archived canon. ",
                    "**None of these has a lint, phase gate, or hook behind it yet.** mockspace's lint gate is ",
                    "expected to eventually carry the design-facing checks; it does not carry them today, and ",
                    "nothing currently stops any of the three violations. Every rule in this document binds on ",
                    "how canon and designs are written regardless, because that is what the rule says, not ",
                    "because something currently catches a violation of it.\n",
                ),
                    &mock_rel, project_name, crate_prefix,
                )
                .replace("{canon_location}", &canon_location);
                if canon_is_the_reserved_directory {
                    body
                } else {
                    drop_directory_convention(body)
                }
            },
        },
        BuiltinRule {
            name: "design-round-consumes-its-inheritance".to_string(),
            apply_to: vec![format!("{mock_rel}/design_rounds/**")],
            body: substitute_builtin_vars(
                concat!(
                    "## A round consumes what was filed for it, and never sheds it\n",
                    "\n",
                    "A topic file left open in TOPIC phase is a filing: somebody found something while working ",
                    "elsewhere and left it in the way so the next round would have to address it. This round is that ",
                    "next round. Every flat topic in `{mock_dir}/design_rounds/` is this round's work, whoever wrote it ",
                    "and whenever.\n",
                    "\n",
                    "**Every open topic is named in this round's changelists.** Say what it changes, or say why it ",
                    "needs nothing. A topic that turns out to be out of scope still gets that written down, with the ",
                    "reason. Silence is indistinguishable from having missed it.\n",
                    "\n",
                    "**Never shed one.** Do not stash it, do not delete it, do not move it to another branch, and do ",
                    "not close the round leaving it unmentioned. If it genuinely belongs to later work it is re-filed ",
                    "as a fresh flat topic after this round closes, so the next round inherits it the same way.\n",
                    "\n",
                    "### Three ways a filing disappears, all observed\n",
                    "\n",
                    "**A topic on a branch that never merged.** The next round branches from trunk, cannot see it, and ",
                    "reports the question as still open because the files say so. Starting a round by branching fresh ",
                    "from trunk drops every filing that has not landed there.\n",
                    "\n",
                    "**A topic swept into somebody else's archive.** `close` sweeps every loose flat file into the one ",
                    "subdirectory it is building, regardless of which round the file belonged to. A topic filed while ",
                    "another round is in flight is archived by that round's close, under that round's name.\n",
                    "\n",
                    "**An archive missing from a branch.** A closed round's directory is finished history and belongs ",
                    "on every branch. When it exists only on a branch that never merged, the entire paper trail for ",
                    "that design conversation is absent from trunk, and nothing reports it.\n",
                    "\n",
                    "### Before opening a changelist\n",
                    "\n",
                    "Check every ref for flat topics and archived round directories this branch cannot see, and pull ",
                    "in what it finds. Do this while still in TOPIC: consuming a topic into a round whose changelist is ",
                    "already written adds work the changelist does not cover, which is the failure this prevents rather ",
                    "than a way around it. If the round has moved past TOPIC, deprecate the changelist and return to ",
                    "TOPIC first.\n",
                    "\n",
                    "### A topic file is a transcript, and accretes\n",
                    "\n",
                    "One file per subject, added to as the round returns to it. A topic that settles in three ",
                    "lines does not earn a file: it is appended to the file for the subject it belongs to, under ",
                    "a new heading. Start a new file when the SUBJECT changes, not when a question is answered.\n",
                    "\n",
                    "Size is the check. Under roughly 300 words a topic file is almost certainly a fragment of ",
                    "another one, and most of what is in it will be heading and metadata rather than content; find ",
                    "the file it belongs to and append there. Over roughly 2000 words it is carrying more than one ",
                    "subject and wants splitting along the seam.\n",
                    "\n",
                    "The freeze on a committed topic is against **rewriting**, never against appending. Never edit ",
                    "or delete what is already recorded, because that is the audit trail. Adding a later section is ",
                    "how a transcript works and is expected.\n",
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

    // On by default, from `mock/agent/config.toml`. A generated snapshot of
    // `mock tools`, with the live command named as the thing to trust once
    // this snapshot is older than the last change to what tools exist. See
    // `crate::tool_catalogue` for why a snapshot rather than only the
    // instruction to run the command: this project's own convention
    // (`reference-syntax`, above) already generates a live-at-render-time
    // answer the same way, and an immediate answer beats a mandatory extra
    // command for the common case where nothing has changed since the last
    // `cargo mock`.
    if cfg.agent.tool_catalogue {
        let snapshot = crate::tool_catalogue::render_table(&crate::tool_catalogue::enumerate(pack));
        rules.push(BuiltinRule {
            name: "tool-catalogue".to_string(),
            apply_to: vec!["**".to_string()],
            body: format!(
                "## Tool catalogue\n\n{}\n\n{}\n\n```\n{}```\n",
                "Never hand-write, from memory, a list of the subcommands or project tools this mockspace exposes. Run `mock tools` for the summary below, or `mock tools --long` for full usage and declared arguments.",
                "The listing below is a snapshot taken at the last `cargo mock` run. It goes stale the moment a tool is added, renamed, or removed without a fresh run; that is exactly why the live command is named above rather than only this list. Prefer `mock tools` over this snapshot whenever the two might disagree.",
                snapshot
            ),
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
            body: format!(
                "## How a panel works\n\n{}\n\n{}\n\n{}\n\n{}\n\n{}",
                "A panel is several personas working one question: arguing, converging, proposing. They talk, they converge, they propose and present. They do not write canon.",
                "Every seat is minted, never counted or guessed. `mock panel seat <slug> <persona> <topic...>` allocates the next seat number from the panel's own inventory file, one past whatever is already recorded there. Two dispatches minting at once are serialised by a lock on the inventory, so the second waits for the first and reads the number the first wrote. The number never comes from a caller's own tally. Ninety-nine is the last seat a panel may ever mint; past it, the panel is over, not paused, and the next step is closing it or opening a new one for whatever remains.",
                "Consolidation is enforced, not optional. Once a panel has minted enough seats since its last consolidation, minting refuses until `mock panel consolidate <slug> <note...>` records what was decided. `mock panel status [slug]` reports whether a panel is presently open (has minted seats no consolidation covers yet) and how many seats remain before the next one is due.",
                "Everything is logged into one formalised inventory file per panel, `<mock>/panel/<slug>.toml`: every seat, every consolidation, in order. It is the whole ledger, and it is what `mock check` reads to decide whether canon is being touched while a panel is still open.",
                "No panel writes canon directly. A panel talks, converges, and proposes; only the coordinating human or agent adds what it produced into the project's canonical registries or documents, and only once the panel has consolidated. `mock check` refuses a change touching a configured canon path while any panel is open, as the mechanical half of this; the rest is discipline this rule states so every panel member reads it too."
            ),
        });
    }

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
            "- **A topic file is a transcript and accretes.** One file per subject, added to\n",
            "  as the talk returns to it. The freeze on a committed topic is against\n",
            "  REWRITING, not against appending: never edit or delete what is recorded, that\n",
            "  is the audit trail, but adding a later section is how a transcript works.\n",
            "  Start a new file when the SUBJECT changes, not when a question is answered.\n",
            "  Under roughly 300 words a file is a fragment of another one and belongs there\n",
            "  instead, since most of it will be heading rather than content; over roughly\n",
            "  2000 words it carries two subjects and wants splitting along the seam.\n",
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
        &mock_rel,
        project_name,
        crate_prefix,
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
            "- Never rewrite or delete what a committed topic already records; that is the\n",
            "  audit trail. Appending a later section to it is expected and is how a topic\n",
            "  file accretes across a talk.\n",
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
        &mock_rel,
        project_name,
        crate_prefix,
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
        BuiltinSkill {
            dir_name: "real-code-guard".to_string(),
            skill_name: "real-code-guard".to_string(),
            skill_description: "Enforces mock-first workflow when implementing features in real crates. Use this whenever modifying, creating, or refactoring code in the crates/ directory, examples/ directory, or any Rust source file outside the mock workspace. Also use when asked to implement a feature or make a change to the framework.".to_string(),
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
            skill_description: "Use BEFORE running any feasibility probe: does it compile, does the trait solve, does this shape work, is this feature gate actually needed, what exactly does the compiler say. Covers sketches and spikes. Directs the probe into mock/research/sketches/ with a FINDINGS.md carrying a WORKS / FAILS / INCONCLUSIVE outcome, instead of a throwaway rustc run in a temp directory whose result nobody can re-run and whose toolchain was probably wrong.".to_string(),
            body: include_str!("skills/sketching/SKILL.md").to_string(),
            files: vec![],
        });
    }
    if cfg.agent.benchmarking_skill {
        skills.push(BuiltinSkill {
            dir_name: "benchmarking".to_string(),
            skill_name: "benchmarking".to_string(),
            skill_description: "Use BEFORE writing any timing loop or making any claim that one thing is faster, cheaper or smaller than another. Any number that feeds a decision comes from mock/benches/ under the mockspace bench framework: per-variant cdylib isolation, a shared realistic workload, calibrated repetition, and a committed CSV plus findings trail. Covers what to measure, how to keep the comparison honest, and what the deliverable is.".to_string(),
            body: include_str!("skills/benchmarking/SKILL.md").to_string(),
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
            body: include_str!("skills/design-talk/SKILL.md").to_string(),
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

/// Everything from the directory-convention heading up to the next `##`.
///
/// The tier argument, canon governs design governs code and the mutation order
/// that follows from it, is true of every project. **The shape of the reserved
/// directory is not**: a project declaring its canon somewhere else, a typed
/// registry say, has no `archive/`, no `examples/`, and no `.md` files to have
/// an opinion about. Shipping that section to it describes a tree it does not
/// have, which is what this rule did to one project for as long as that project
/// has existed.
fn drop_directory_convention(body: String) -> String {
    let Some(start) = body.find("### The shape of ") else {
        return body;
    };
    let rest = &body[start ..];
    // the next top-level heading, since the cut section has `###` children
    let end = rest[3 ..].find("\n## ").map(|i| start + 3 + i + 1);
    match end {
        Some(e) => format!("{}{}", &body[.. start], &body[e ..]),
        None => body[.. start].to_string(),
    }
}

#[cfg(test)]
mod canon_location_is_derived {
    //! The canon rule reads where the canon is, rather than asserting it.
    //!
    //! It used to name `<mock>/canon/` nine times whatever the project said,
    //! and ship the reserved-directory convention to a project with no such
    //! directory. kamu declares `canon_paths = ["mock/registry/*.toml"]` two
    //! lines from where the rule was read, and every session there loaded a
    //! rule describing a tree that does not exist.
    use super::*;

    fn rule_for(canon_paths: Vec<String>) -> BuiltinRule {
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
        std::fs::write(
            mock.join("mockspace.toml"),
            format!("project_name = \"probe\"\n{declared}"),
        )
        .unwrap();
        let cfg = crate::config::Config::from_dir(&mock);
        assert_eq!(
            cfg.canon_paths, canon_paths,
            "control: the config parsed what we wrote"
        );
        generate_builtin_templates(&cfg, &LintPack::default())
            .rules
            .into_iter()
            .find(|r| r.name == "canon-design-code-chain")
            .expect("the rule is always minted")
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
        // reserved-directory convention describes a tree this project has none
        // of, and shipping it is the defect.
        let r = rule_for(vec!["mock/registry/*.toml".into()]);
        assert!(
            !r.body.contains("### The shape of"),
            "the directory convention must not ship: {}",
            r.body
        );
        for absent in ["/canon/archive/", "/canon/examples/"] {
            assert!(
                !r.body.contains(absent),
                "{absent} must not appear: {}",
                r.body
            );
        }
        // and the universal half survives the cut
        assert!(
            r.body.contains("The canon, design, code chain"),
            "{}",
            r.body
        );
        assert!(
            r.body.contains("Code is last"),
            "the tiers survive: {}",
            r.body
        );
    }

    #[test]
    fn declaring_nothing_keeps_the_reserved_directory_convention() {
        // the control. without it both arms above pass on a rule that simply
        // never mentions a canon directory at all.
        let r = rule_for(Vec::new());
        assert!(r.body.contains("### The shape of"), "{}", r.body);
        assert!(
            r.body.contains("a directory reserved for it alone"),
            "{}",
            r.body
        );
        assert!(
            r.apply_to.iter().any(|p| p.ends_with("/canon/**")),
            "{:?}",
            r.apply_to
        );
    }

    #[test]
    fn a_project_whose_canon_is_that_directory_keeps_it_too() {
        // declaring the conventional location explicitly must not lose the
        // convention, else the check is on emptiness rather than on location.
        let r = rule_for(vec!["mock/canon/**".into()]);
        assert!(r.body.contains("### The shape of"), "{}", r.body);
    }
}
