//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock help`, and what to say when a subcommand is not recognised.
//!
//! # Why help must not need a project
//!
//! `mock --help` used to print `no mockspace.toml found. Run from a mockspace
//! directory or use --dir <path>`. That is the correct answer to "run the
//! workflow here" and the wrong answer to "what does this tool do", which is the
//! one question a reader outside a project is most likely to ask, and often the
//! first thing anyone types. So help resolves before any project discovery and
//! works anywhere.
//!
//! # Why an unknown subcommand is an error
//!
//! An unrecognised positional argument used to fall through to the default run,
//! meaning `mock lck` silently did a full generate instead of locking, and a typo
//! looked like success. It is now an error that names the nearest real subcommand,
//! since the reader already knows what they meant and only needs the spelling.

use std::process::ExitCode;

use mockspace_lint_rules::tool::ArgSpec;

/// One builtin subcommand, described the same way a project [`Tool`] is.
///
/// [`Tool`]: mockspace_lint_rules::tool::Tool
///
/// Before this carried `args` and `help`, a builtin was a name and a summary
/// and nothing else: a project [`Tool`] could declare its positional
/// arguments and a longer help body, and a builtin could not, so the same
/// engine described its own commands in a thinner shape than it required of
/// everyone else's. `mock tools` (see [`crate::tool_catalogue`]) is what
/// exposed the asymmetry: it has one enumeration to write, and that
/// enumeration needs one shape both populations already fit.
///
/// **Declared, not enforced.** A [`Tool`]'s `args` are checked against a real
/// invocation by [`mockspace_lint_rules::tool::missing_required`] and
/// [`mockspace_lint_rules::tool::contract_faults`]; a builtin's `args` are
/// not checked against anything, because a builtin is dispatched by hand in
/// `dispatch.rs` rather than through the tool contract. See
/// `every_builtin_declaring_a_required_argument_is_named_here_deliberately`
/// below for what that means and why it is pinned rather than merely noted.
pub(crate) struct Cmd {
    pub(crate) name:    &'static str,
    pub(crate) summary: &'static str,
    /// Declared positional arguments, in the same [`ArgSpec`] shape a project
    /// tool uses. Most builtins take their input as flags rather than
    /// positionals and declare none; `panel` is the exception and takes a
    /// verb. The test named above pins the exact set that declares anything,
    /// so a second one cannot appear unnoticed, and the field exists so a
    /// builtin that does take one has somewhere honest to say so,
    /// and so [`usage_from`](mockspace_lint_rules::tool::usage_from) renders
    /// a builtin's usage line exactly the way it renders a tool's.
    pub(crate) args:    &'static [ArgSpec],
    /// Longer help, shown by `mock tools --long`. Empty is allowed: not
    /// every builtin needs more said about it than the summary already says.
    pub(crate) help:    &'static str,
}

/// Every subcommand, in the order help lists them: the round's own lifecycle
/// first, then everything else, because that order is the workflow.
const COMMANDS: &[Cmd] = &[
    Cmd {
        name:    "lock",
        summary: "seal the active changelist, advancing the round's phase",
        args:    &[],
        help:    "Seals the active changelist (doc or src), advancing the round \
                  from DOC to DRAFT or from IMPL to CLOSED. Accepts --auto-commit \
                  to commit the rename with git plumbing (unsigned, no hooks run).",
    },
    Cmd {
        name:    "unlock",
        summary: "deprecate the src changelist and reopen the doc one",
        args:    &[],
        help:    "Deprecates the src changelist and reopens the doc one. \
                  Destructive to the changelist bookkeeping only: source is left \
                  exactly as it is. Accepts --auto-commit.",
    },
    Cmd {
        name:    "deprecate",
        summary: "supersede the active changelist, keeping it as an audit trail",
        args:    &[],
        help:    "Supersedes the active changelist rather than deleting it, so it \
                  stays as an audit trail. In IMPL this also unlocks the doc \
                  changelist. A stub (a bare title) beside a locked changelist \
                  of its kind is retired instead, in any phase, and nothing \
                  else happens on that run; one with text is refused. Accepts --auto-commit.",
    },
    Cmd {
        name:    "close",
        summary: "archive a finished round into a directory named by its id",
        args:    &[],
        help:    "Archives a finished round (both changelists locked) into \
                  design_rounds/<id>/ with .meta and .history. Takes the \
                  changelists, the files that start with the id of one, and \
                  the files one names, and any flat file given as \
                  --take <file> (repeatable); any other flat file stays and \
                  is listed. Valid only from CLOSED. Accepts --auto-commit.",
    },
    Cmd {
        name:    "archive",
        summary: "archive an abandoned round without closing it",
        args:    &[],
        help:    "Archives an abandoned round into an <id>-abandoned/ \
                  subdirectory at any phase, without requiring it to reach \
                  CLOSED first. Where the round has changelists it takes what \
                  close takes and is named by their id, and leaves the other \
                  flat files. In the TOPIC phase, with no changelist, it takes \
                  every flat file and is named by the earliest of them. \
                  Accepts --auto-commit.",
    },
    Cmd {
        name:    "round-id",
        summary: "print a fresh design round id",
        args:    &[],
        help:    "Prints one id for a round you are about to open, and nothing else: \
                  the minute, YYYYMMDDHHMM, then a four character tag, \
                  YYYYMMDDHHMM-tttt. Name every file of the round with it, \
                  <id>_topic.<name>.md and <id>_changelist.doc.md, instead of a \
                  bare minute that another branch may have taken. The tag is drawn \
                  from the branch, host and time of the call, and walked past any \
                  id the working tree, the round tags or origin/dev and origin/main \
                  already hold. Ids written without a tag stay valid and sort with \
                  the new ones by minute.",
    },
    Cmd {
        name:    "rename-round",
        summary: "give a round a new id, when two rounds share one",
        args:    &[],
        help:    "`rename-round <old id or directory> [<new id>]` renames one round: the closed directory (or the flat files of the \
                  active round) with the old id, every file inside that starts with \
                  it, the `round:` line of .meta, and the local round/<old>/* tags. \
                  The new id defaults to a fresh one on the same minute. Run it on \
                  the branch to rename, before merging, when a merge was refused with \
                  a round id collision. An abandoned round keeps its -abandoned and loses \
                  its -N count. A closed round that origin/dev or origin/main \
                  already holds exactly as committed has landed and is refused. \
                  Accepts --auto-commit.",
    },
    Cmd {
        name:    "merge-round-meta",
        summary: "refuse a merge of two closed rounds that share an id, as git's merge driver",
        args:    &[],
        help:    "The merge driver `cargo mock` installs for \
                  <mock>/design_rounds/*/.meta; git calls it as `merge-round-meta %O \
                  %A %B %P`. A .meta added on both sides with different content is \
                  two rounds with one id: it exits 1 and names the id and `cargo mock \
                  rename-round <id>`. Anything else merges as text. It needs no \
                  project and installs nothing: the wiring is done by `cargo mock`.",
    },
    Cmd {
        name:    "status",
        summary: "show the current round, its phase, and what may be edited",
        args:    &[],
        help:    "Reports whether the mockspace git hooks are active, and how to \
                  flip that if not.",
    },
    Cmd {
        name:    "check",
        summary: "readiness report: git, phase, build, tests, lints",
        args:    &[],
        help:    "A full readiness report: working-tree cleanliness, branch/remote \
                  sync, round phase, build, tests, and lints, without regenerating \
                  anything.",
    },
    Cmd {
        name:    "check-message",
        summary: "lint one commit message or forge body against the configured policy",
        args:    &[],
        help:    "Lints one authored message against the configured message \
                  policy. --domain selects which policy (see the domain tokens in \
                  the error this prints when it is missing or wrong). --file \
                  <path> reads the message from a file; with no --file it reads \
                  the message from stdin, which is how a hook passes text it \
                  extracted from a command. --author and --committer each take \
                  an identity as git writes it, `Name <mailbox>`, and hand it to \
                  the lints as who the commit is by; the commit-msg hook reads \
                  both from `git var`. With --batch every record may carry its \
                  own pair, which is how the pre-push hook sends them.",
    },
    Cmd {
        name:    "check-edit",
        summary: "ask one crate lint whether an edit not yet made would leave its file clean",
        args:    &[],
        help:    "Asks the per-crate lint named by --lint about the file named by \
                  --file, as the file would read after an edit, through the same \
                  pass the gate gives every crate: the same configuration, binding, \
                  filters and lint:allow markers. The generated hook adds --lint-only, which \
                  skips the gate's own setup and changes no verdict. Stdin is the text the edit \
                  writes; with --replace <file> it replaces the text in that file \
                  in the target (every occurrence with --all, else the one), and \
                  without it it is the target's whole new text. --edits <file> is \
                  a JSON list of {old_string, new_string, replace_all} applied in \
                  order, a first empty old_string creating the file, and stands in \
                  for stdin, --replace and --all. An edit whose old text is absent \
                  from the file, or occurs more than once without --all, is one the \
                  editing tool refuses itself, so it gets no verdict and exits 0. \
                  Prints the lint's blocking findings in the target file and exits \
                  1 if there are any, 2 when the request cannot be answered: no \
                  per-crate lint answers to the name (a lint judging the crate as a \
                  whole is one), a required argument is missing, --file is not an \
                  absolute path, or an input cannot be read. --gate \
                  selects commit, push or build severities, commit by default, and \
                  anything else is refused. \
                  This is how an agent hook named after a lint reaches the lint.",
    },
    Cmd {
        name:    "query",
        summary: "query the registry",
        args:    &[],
        help:    "Runs a query expression (a root or namespace path, the same \
                  syntax a `{{ root::selector }}` reference uses) against the \
                  registry. The expression is the token right after `query`; an \
                  absent one queries nothing.",
    },
    Cmd {
        name:    "registry-append",
        summary: "add one string to a list field of a registry row",
        args:    &[],
        help:    "Takes `<namespace>::<id> <field> <value> [--local]` and adds the value \
                  to the row's `string[]` field, which is made where the row has none. The \
                  namespace and the field must be declared, the row must exist, and a \
                  value the field already holds is not added twice. Only the row's own \
                  lines change; the rest of the file is as it was. It prints nothing on \
                  success. A workspace may place a delegate at \
                  `.mockspace/hooks/registry-append` in the repository root or any \
                  directory above it, the nearest winning; it is run in the repository \
                  root as `registry-append <namespace>::<id> <field> <value>` with \
                  MOCKSPACE_REPO_ROOT and MOCKSPACE_MOCK_DIR set, and exit status 0 \
                  means it did the whole step, 3 that it leaves the step to mockspace, \
                  anything else a failure. `--local` skips the delegate, which is how \
                  the delegate writes the row itself.",
    },
    Cmd {
        name:    "merge-registry",
        summary: "merge registry TOML by row, as git's merge driver",
        args:    &[],
        help:    "The merge driver `cargo mock` installs for <mock>/registry/**/*.toml; \
                  git calls it as `merge-registry %O %A %B %P %L` (base, ours, theirs, \
                  path, marker length) and reads the result from the second. \
                  Rows are keyed by their [[namespace]] and `id`, never by file or position, so two branches \
                  that each added a row to the same spot both keep it. A row changed \
                  on one side takes that change, one deleted on one side and untouched \
                  on the other is deleted, and one changed differently on both sides \
                  (or deleted against a change) is a conflict, written with markers \
                  around that row alone and exit status 1. A file it cannot read as \
                  rows is merged as text, as git would without it. It needs no \
                  project and installs nothing: the wiring is done by `cargo mock`.",
    },
    Cmd {
        name:    "register-drivers",
        summary: "register the merge drivers in the user's git config",
        args:    &[],
        help:    "`register-drivers --global` writes the five merge drivers `cargo mock` \
                  registers per clone into the user's git config as well, with commands that \
                  name no clone: a clone made later, a worktree, or a container that never \
                  ran `cargo mock` there then calls them when its `.gitattributes` names one. \
                  Idempotent. Run inside a repository it also unsets that clone's older entry for the \
                  generated-file driver where the entry names a path. It writes to the user \
                  and clone config and nowhere else, and needs no \
                  project.",
    },
    Cmd {
        name:    "merge-additions",
        summary: "merge a file that only grows, as git's merge driver",
        args:    &[],
        help:    "The merge driver `cargo mock` installs for the files `append_only` in \
                  mockspace.toml names (and each crate's SHAME and BACKLOG templates); git \
                  calls it as `merge-additions %O %A %B %P %L` and reads the result from the \
                  second. When neither side changed or removed a line of the base, both \
                  sides' additions are kept as they are, each in the spot it was made in, \
                  ours before theirs where both added to one spot. When either side \
                  edited or removed a base line, or both added the same line to one spot, \
                  or the file has no base or no final newline, the file is merged as \
                  text, as git would without it, conflict markers and exit status 1 \
                  included. It never picks a side. It needs no project and installs \
                  nothing: the wiring is done by `cargo mock`.",
    },
    Cmd {
        name:    "merge-test-history",
        summary: "merge a member's test-history file, as git's merge driver",
        args:    &[],
        help:    "The merge driver `cargo mock` installs for <mock>/test-history/**/*.json; \
                  git calls it as `merge-test-history %O %A %B %P %L` and reads the \
                  result from the second. A green record (and a test binary's recorded \
                  pass) is kept where both sides hold the same one or only one side \
                  moved it from the base, and dropped where both moved it to different \
                  values, since neither fingerprint is the merged tree's: the next \
                  `cargo mock test` records it. It never picks between two greens \
                  that both moved. Per-test results and timings are unioned, a test both \
                  sides hold differently taking the later measurement (the side that \
                  moved it, else the side that last changed the file later (for a `git merge`; a rebase or cherry-pick has no second tip to compare yet), else the longer \
                  timing). Two files of different versions, members or flavours, or one \
                  that is not a record, are refused with a message and exit status 1, \
                  leaving the path conflicted. It needs no project and installs \
                  nothing: the wiring is done by `cargo mock`.",
    },
    Cmd {
        name:    "check-test-history",
        summary: "refuse a commit that nulls a green record it has no reason to",
        args:    &[],
        help:    "The pre-commit guard over <mock>/test-history/, run by the generated \
                  hook when a commit stages one of its files. A commit that removes the \
                  green record of a member whose inputs it does not touch (its own \
                  directory, the directories of the members compiled into its tests, \
                  the paths its sources read from outside, the workspace-wide files) is \
                  refused, naming the member: a run cut short leaves that. In a merge \
                  commit a null is accepted when `merge-test-history` would have \
                  written it. Exit status 0 passes, 1 refuses.",
    },
    Cmd {
        name:    "bench",
        summary: "run the bench harness",
        args:    &[],
        help:    "Runs the bench harness. Everything after `bench` (a variant \
                  name, flags such as --release) is forwarded to it verbatim, so \
                  its own usage governs from that point on.",
    },
    Cmd {
        name:    "panel",
        summary: "mint or consolidate a panel seat, or report a panel's state",
        args:    &[ArgSpec {
            name:        "verb",
            required:    true,
            description: "seat, consolidate, or status",
        }],
        help:    "Mints and consolidates panel seats against a formalised, \
                  indexed inventory file at <mock>/panel/<slug>.toml.\n\n\
                  `mock panel seat <slug> <persona> <topic...>` mints the next \
                  seat number (never counted or guessed; always one past the \
                  highest number already in the file). Refused once the seat \
                  cap (99, the last seat this ever allows) is reached, or once \
                  the consolidation cadence is due and has not been met.\n\n\
                  `mock panel consolidate <slug> <note...>` records a \
                  consolidation covering every seat minted so far. Refused \
                  when there is nothing new to consolidate.\n\n\
                  `mock panel status [slug]` reports one panel's seat count, \
                  whether it is open (has minted seats no consolidation \
                  covers yet), and how many seats remain before the next \
                  consolidation is due. With no slug, reports on every panel \
                  declared under <mock>/panel/.",
    },
    Cmd {
        name:    "test",
        summary: "run the tests of every tree mockspace owns, the members and the benches only where something changed",
        args:    &[],
        help:    "Runs the tests of each tree separately: the mock \
                  workspace's own members, every tool crate under \
                  <mock>/tools/, every bench crate under <mock>/benches/, and \
                  the generated lint crate carrying <mock>/lints/*.rs.\n\n\
                  Only the first is reached by a plain `cargo test` in <mock>/. \
                  The other three are compiled by mockspace outside the \
                  workspace, so a repository can hold tools, benches and lints \
                  with tests and run none of them while appearing to. A \
                  repository whose `members` list is empty runs nothing at \
                  all.\n\n\
                  The benches tree is the most expensive of the four (every \
                  arm is compiled on its own), so it runs only when its \
                  inputs moved since it last passed, or when it is asked \
                  for, with --benches or with --full or --no-cache, which \
                  run every member whatever its record says. Beside a -p \
                  naming crates it is left out unless --benches is given \
                  too. Its inputs are the files git tracks or would track \
                  under <mock>/benches/ (less results/ and history/, which \
                  `mock bench run` writes, build output, and a lockfile \
                  git does not track, which is the build's own), the files \
                  of every local crate its manifests depend on by path, the \
                  text of the manifests `mock bench` generates for arms \
                  that have none, and what every build reads: the \
                  workspace manifest, lockfile, toolchain pin, and cargo \
                  and nextest configuration. A tree that has never passed \
                  counts as moved, the record being kept under \
                  <mock>/target. A run leaving it out says so, and so does \
                  its last line, a failed run's included.\n\n\
                  The members run under cargo-nextest where it is installed, \
                  and under `cargo test` where it is not or with --plain. \
                  Under nextest a member runs only when its fingerprint moved \
                  since its suite last passed: its own files, the paths \
                  outside it its sources name as `../` literals (content, \
                  fixtures), the same for every member compiled into its \
                  tests, and the lockfile, toolchain and cargo configuration. \
                  Below the member it works per test binary: an integration \
                  test (`tests/<name>.rs`, `tests/<name>/main.rs`) is built \
                  from its own file, the modules it declares, the files its \
                  string literals name inside the member, and what its \
                  sources read from outside, and from the member's library \
                  and the libraries it depends on, never from another \
                  integration test. A binary that passed at inputs that have \
                  not moved is left out of a member that runs (listed as \
                  `reuse`), so a change confined to one test file runs that \
                  binary. Anything it cannot place widens: a file under \
                  tests/ no binary claims, the library, the manifest and the \
                  build script move every binary, and a binary that reads \
                  the environment or `file!()`, or declares a module inside a \
                  macro or an inline module, is rebuilt on every claimed \
                  file. A dependent is not moved by its dependency's tests. \
                  A test that took `[test] heavy_after_secs` (10) or longer \
                  last time is heavy: heavy tests run `[test] heavy_threads` \
                  (1) at a time, and one that passed at its binary's current \
                  fingerprint is skipped as cached. Every test is killed \
                  after `[test] timeout_secs` (900). The run ends with the \
                  slowest tests.\n\n\
                  --full runs every member asked for, every binary of it. \
                  --no-cache runs the cached heavy tests and the binaries it \
                  would leave out too. \
                  --cheap also defers every heavy \
                  test that has passed before, running only new ones and ones \
                  that failed last time, and leaves the members it deferred \
                  for owing them. -p and --workspace choose members as in \
                  cargo; with -p only the members tree runs, bench tree aside \
                  (--benches). The record is \
                  tracked, <mock>/test-history/<flavour>/<member>.json, \
                  committed with the work so a fresh clone starts warm; \
                  a test or binary that no longer exists is dropped from \
                  the run and pruned from the record; \
                  `--features x` has its own flavour. `[test] commit` gates a \
                  commit as a lint level does: warn runs --cheap, error also \
                  blocks, off and info run nothing. `[test.opt_level]` maps a \
                  member to the opt level (0 to 3, \"s\", \"z\") it is built \
                  at in the run's profile, --plain too; a change reruns \
                  members. Doc tests do not run under nextest; --plain \
                  reaches them.\n\n\
                  The members are built without debug info, whatever the \
                  manifest or a cargo config says, and a crate gets symbols \
                  (`[test] debug`, line-tables-only unless set) only where \
                  one is needed to trace a problem: a member whose tests \
                  failed the last time (kept under <mock>/target, not in the \
                  tracked record), or one named with --debug=<crate,...>, \
                  which also chooses it and runs it whether or not it \
                  passed. -p x --debug is the same for x; a bare --debug \
                  with no -p gives symbols to every member that runs, and \
                  --no-debug to none. A crate built with symbols is built \
                  again with its dependents, not with its dependencies or \
                  the rest, and the lean build stays beside it. A cargo \
                  config or CARGO_PROFILE_*_DEBUG that sets debug info is \
                  reported and overridden for the run, never edited; \
                  `[test] lean = false` lets it stand. Dependencies are \
                  lean too, one a manifest or a cargo config names for \
                  itself included. Rustflags that give debug info, -g or \
                  -C debuginfo= above none, in RUSTFLAGS, \
                  CARGO_ENCODED_RUSTFLAGS or a cargo config, give it to \
                  every crate and no profile setting undoes that: they \
                  are reported and left, and the plan stops saying the \
                  rest is lean.\n\n\
                  Everything else after `test` is forwarded to each cargo \
                  invocation, so `mock test --release` and `mock test -- \
                  --nocapture` both work. A filter after `--` or an -E runs, \
                  but leaves nothing recorded as passed whole.\n\n\
                  The lint tree needs the generated crate to exist; `mock \
                  check` generates it. It is reported as absent rather than \
                  generated here, so one path knows how.",
    },
    Cmd {
        name:    "pdf",
        summary: "render the design documents to PDF",
        args:    &[],
        help:    "Renders the design documents to PDF. Everything after `pdf` is \
                  forwarded to the renderer, aside from a --dir <path> already \
                  consumed to locate the mock workspace.",
    },
    Cmd {
        name:    "clean",
        summary: "remove generated output",
        args:    &[],
        help:    "Removes generated output (docs/ and the agent integration \
                  files), so the next run starts from nothing rather than \
                  merging into what is already there.",
    },
    Cmd {
        name:    "migrate",
        summary: "migrate this repo to the current mockspace conventions",
        args:    &[],
        help:    "Renames legacy YYYY-MM-DD_*.md design-round filenames to the \
                  compact timestamp format this repo's convention now expects. \
                  Accepts --auto-commit.",
    },
    Cmd {
        name:    "activate",
        summary: "point core.hooksPath at the mockspace gate",
        args:    &[],
        help:    "Sets core.hooksPath at the generated mockspace hooks. Whatever the \
                  repository keeps under .git/hooks/ still runs, after the gate has \
                  passed, with the same arguments and stdin.",
    },
    Cmd {
        name:    "deactivate",
        summary: "restore git's default hooks path",
        args:    &[],
        help:    "Unsets core.hooksPath, so git falls back to .git/hooks/ \
                  directly.",
    },
    Cmd {
        name:    "tools",
        summary: "list every subcommand and project tool, with usage and kind",
        args:    &[],
        help:    "Enumerates every builtin subcommand and every project tool \
                  declared under <mock>/tools/, each with its usage line and \
                  one-line summary, and each tool with its kind: a check that \
                  cannot be a lint, or a maker that writes files. --long also \
                  prints what each tool is held to (its reason, or the paths it \
                  writes and the root of each argument a path starts \
                  with), its declared \
                  arguments and its longer help text where it has one. This is \
                  the live answer: it is computed at the moment it is asked, \
                  from the same declared shape every tool and every builtin \
                  already carries, rather than from a list somebody \
                  hand-maintained and that a new tool would not appear in.",
    },
    Cmd {
        name:    "lints",
        summary: "list every lint, with the severity this project gave it",
        args:    &[],
        help:    "Enumerates every lint that runs here, of every kind, with the \
                  severity that actually governs it and whether that came from \
                  this project's [lints] tables or from a default nobody here \
                  chose. That is the builtins and the loaded pack together, \
                  because the engine runs both and a listing of either alone \
                  under-counts what is checking you. Two disagreements are \
                  reported because nothing else reports them: a lint the project \
                  never named, and a configured severity naming no lint at all, \
                  which reads like a live decision and governs nothing. Neither \
                  is presented as a fault. The set is read from the engine and \
                  the pack rather than from source, because a lint declares its \
                  own name and grepping for the convention under-counts.",
    },
    Cmd {
        name:    "help",
        summary: "show this message, or one command in full",
        args:    &[ArgSpec {
            name:        "name",
            required:    false,
            description: "a subcommand or project tool to describe in full",
        }],
        help:    "With a name, prints that command's entry as `mock tools --long` \
                  does: usage, summary, what a project tool is for, declared \
                  arguments and longer help. A builtin is answered without a \
                  project; a project tool needs its pack, so run that through \
                  `cargo mock help <name>`.",
    },
];

/// Whether `arg` asks for help in any of its spellings.
#[must_use]
pub(crate) fn is_help_request(arg: &str) -> bool {
    matches!(arg, "help" | "--help" | "-h" | "-?")
}

/// Every known subcommand name, for suggestion and validation.
#[must_use]
pub(crate) fn known_commands() -> Vec<&'static str> {
    COMMANDS.iter().map(|c| c.name).collect()
}

/// Every builtin subcommand, in full, for anything that needs more than the
/// bare name: [`crate::tool_catalogue`] is the one caller today.
#[must_use]
pub(crate) fn commands() -> &'static [Cmd] {
    COMMANDS
}

/// Print the help text.
pub(crate) fn print_help() -> ExitCode {
    println!("mock: the mockspace design-round workflow engine.");
    println!();
    println!("USAGE");
    println!("    mock [subcommand] [options]");
    println!();
    println!("    With no subcommand: check, parse, lint, and regenerate the");
    println!("    documents and agent files for the current project.");
    println!();
    println!("SUBCOMMANDS");
    let width = COMMANDS.iter().map(|c| c.name.len()).max().unwrap_or(0);
    for c in COMMANDS {
        println!("    {:<width$}  {}", c.name, c.summary, width = width);
    }
    println!();
    println!("OPTIONS");
    println!("    --dir <path>       the mock workspace to act on, instead of");
    println!("                       searching upward from the working directory");
    println!("    --scope <crates>   limit linting to a comma-separated crate list");
    println!("    --doc-only         limit linting to documentation");
    println!("    --lint-only        lint without regenerating anything");
    println!("    --generate-only    regenerate without checking or linting; the merge");
    println!("                       hooks run this over the merged tree");
    println!("    --stage            with --generate-only, stage what it generated");
    println!("    --commit-regenerated");
    println!("                       with --generate-only, commit what differs from HEAD;");
    println!("                       --commit-regenerated=rebase names the commit for a rebase");
    println!("    --commit           lint at the commit gate's severities");
    println!("    --strict           lint at the push gate's severities");
    println!("    --auto-commit      commit the state transition a subcommand makes");
    println!("                       (writes the commit with git plumbing, so no hook");
    println!("                       runs and the commit is not signed)");
    println!("    -h, --help         show this message");
    println!();
    println!("    --scope and --doc-only are verified against what is staged: a");
    println!("    narrower claim than the staged set is refused rather than obeyed.");
    println!();
    println!("Configuration lives in mockspace.toml at the repo root.");
    println!();
    println!("    `--nuke` takes a tier down so it can be written again from the");
    println!("    one above it. `--nuke` alone takes the source, leaving the designs;");
    println!("    `--nuke=docs` takes the designs and the source under them, which is");
    println!("    the order the design chain requires. It names every file first and");
    println!("    then asks; `--y` answers for you. It refuses a tree with anything");
    println!("    uncommitted, because git is the only thing that gives any of it back.");
    ExitCode::SUCCESS
}

/// Report an unrecognised subcommand, suggesting the nearest known one.
///
/// `tools` is the project's own `<mock>/tools/` directory names. They are
/// subcommands this binary cannot know at compile time, and leaving them out
/// would tell a reader who mistyped one that their tool does not exist.
pub(crate) fn unknown_subcommand(given: &str, tools: &[String]) -> ExitCode {
    eprintln!("mock: `{given}` is not a subcommand.");
    if let Some(s) = suggest_among(given, tools) {
        eprintln!();
        eprintln!("  did you mean `mock {s}`?");
    }
    eprintln!();
    eprintln!("available subcommands:");
    for name in known_commands() {
        eprintln!("  {name}");
    }
    if !tools.is_empty() {
        eprintln!();
        eprintln!("tools in this project:");
        for name in tools {
            eprintln!("  {name}");
        }
    }
    eprintln!("\n(run `cargo mock` with no subcommand to regenerate docs and agent rules)");
    ExitCode::from(2)
}

/// The nearest name among the builtins and this project's tools.
///
/// A tool is as likely to be mistyped as a builtin, and it is the one the
/// reader is less sure of the spelling of, since they may have written it
/// themselves that morning.
fn suggest_among(given: &str, tools: &[String]) -> Option<String> {
    let builtin = crate::entry::suggest_subcommand(given).map(str::to_string);
    let lowered = given.to_ascii_lowercase();
    let nearest_tool = tools
        .iter()
        .map(|t| {
            (
                t,
                crate::entry::levenshtein(&lowered, &t.to_ascii_lowercase()),
            )
        })
        .min_by_key(|(_, d)| *d)
        .filter(|(t, d)| *d <= (lowered.len() / 2).max(2) || t.starts_with(&lowered));
    match (builtin, nearest_tool) {
        (Some(b), Some((t, dt))) => {
            let db = crate::entry::levenshtein(&lowered, &b.to_ascii_lowercase());
            Some(if dt < db { t.clone() } else { b })
        },
        (Some(b), None) => Some(b),
        (None, Some((t, _))) => Some(t.clone()),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_help_spelling_is_recognised() {
        for arg in ["help", "--help", "-h", "-?"] {
            assert!(is_help_request(arg), "{arg} should ask for help");
        }
        for arg in ["lock", "--strict", "helper", "-x"] {
            assert!(!is_help_request(arg), "{arg} should not ask for help");
        }
    }

    #[test]
    fn a_close_typo_gets_the_command_it_meant() {
        assert_eq!(crate::entry::suggest_subcommand("lck"), Some("lock"));
        assert_eq!(crate::entry::suggest_subcommand("helpp"), Some("help"));
        assert_eq!(crate::entry::suggest_subcommand("statsu"), Some("status"));
        assert_eq!(
            crate::entry::suggest_subcommand("depreciate"),
            Some("deprecate")
        );
        assert_eq!(
            crate::entry::suggest_subcommand("actiavte"),
            Some("activate")
        );
    }

    #[test]
    fn an_abbreviation_gets_the_command_it_starts() {
        // Someone typing `dep` has not misspelled anything, they stopped early,
        // and a distance threshold alone would reject it.
        assert_eq!(crate::entry::suggest_subcommand("dep"), Some("deprecate"));
        assert_eq!(crate::entry::suggest_subcommand("chec"), Some("check"));
    }

    #[test]
    fn nonsense_gets_no_suggestion() {
        // A wrong guess is worse than none: it sends the reader to try something
        // they did not mean, and they may not notice.
        assert_eq!(crate::entry::suggest_subcommand("xyzzy"), None);
        assert_eq!(
            crate::entry::suggest_subcommand("frobnicate-everything"),
            None
        );
    }

    #[test]
    fn a_single_character_does_not_get_a_wild_guess() {
        // Distance one from many names at once, so any pick would be arbitrary.
        assert_eq!(crate::entry::suggest_subcommand("q"), None);
    }

    #[test]
    fn the_suggestion_is_case_insensitive() {
        assert_eq!(crate::entry::suggest_subcommand("LOCK"), Some("lock"));
        assert_eq!(crate::entry::suggest_subcommand("Lck"), Some("lock"));
    }

    #[test]
    fn every_command_suggests_itself() {
        // The sanity property: nothing in the list is unreachable, which would
        // mean a real command whose exact spelling gets "did you mean" for
        // something else.
        for name in known_commands() {
            assert_eq!(
                crate::entry::suggest_subcommand(name),
                Some(name),
                "{name} should suggest itself"
            );
        }
    }

    #[test]
    fn every_listed_command_has_a_summary() {
        // Help is the surface a stranger reads first; a blank column there is
        // worse than the command being undocumented elsewhere.
        for c in COMMANDS {
            assert!(!c.summary.is_empty(), "{} has no summary", c.name);
            assert!(
                !c.summary.ends_with('.'),
                "{}'s summary should not end with a period",
                c.name
            );
        }
    }

    #[test]
    fn a_mistyped_tool_name_suggests_the_tool() {
        // A tool is a subcommand this binary cannot know at compile time, so
        // without this the reader who mistyped their own tool is told it does
        // not exist.
        let tools = vec!["phrase-search".to_string(), "corpus-talk".to_string()];
        assert_eq!(
            super::suggest_among("phrase-serach", &tools),
            Some("phrase-search".to_string())
        );
        assert_eq!(
            super::suggest_among("corpus", &tools),
            Some("corpus-talk".to_string())
        );
    }

    #[test]
    fn a_builtin_still_wins_when_it_is_the_nearer_name() {
        // The case that must fail if tools were simply preferred: `stat` is
        // one edit from a builtin and nowhere near either tool.
        let tools = vec!["phrase-search".to_string(), "corpus-talk".to_string()];
        assert_eq!(
            super::suggest_among("stauts", &tools),
            Some("status".to_string())
        );
        assert_eq!(
            super::suggest_among("lck", &tools),
            Some("lock".to_string())
        );
    }

    #[test]
    fn nonsense_gets_no_suggestion_even_with_tools_present() {
        // The negative arm. Adding a candidate set must not make every input
        // resolve to whichever tool happened to be nearest.
        let tools = vec!["phrase-search".to_string()];
        assert_eq!(super::suggest_among("xyzzy", &tools), None);
        assert_eq!(super::suggest_among("frobnicate-everything", &tools), None);
    }

    #[test]
    fn with_no_tools_the_suggestion_is_exactly_what_it_was_before() {
        // Existing consumers have no tools directory, so this pins that the
        // whole mechanism is inert for them.
        for probe in ["lck", "helpp", "statsu", "dep", "xyzzy", "q"] {
            assert_eq!(
                super::suggest_among(probe, &[]).as_deref(),
                crate::entry::suggest_subcommand(probe),
                "`{probe}` must be unchanged when a project declares no tools"
            );
        }
    }

    #[test]
    fn help_lists_itself_so_the_listing_is_complete() {
        assert!(known_commands().contains(&"help"));
    }
}
