# Dissolve the proxy and the build.rs bootstrap: the launcher as the sole entry

**Date:** 2026-07-19
**Supersedes-in-spirit:** `202607191140_launcher-vs-buildrs.md` (the first sketch). This is the concrete
design op asked for: parity first, then dissolve the proxy.
**Status when written:** design for confirmation. High blast radius (every consumer repo). Not
implemented.

**Status now:** implemented, and landed by a route other than this document, which sat on an unmerged
branch while the work shipped. `src/bootstrap/mod.rs` records that the build.rs bootstrap, the
`.cargo` alias and the generated proxy crate are gone and that `bootstrap_from_buildscript` survives
as a tombstone. The file lands as audit trail; the status line above is preserved as what it said at
the time rather than rewritten.

## What the bootstrap does today (the parity audit)

`bootstrap_from_buildscript()` (run from each consumer's `build.rs`) does, in order:

1. **Guard A**: skip when building inside the cargo cache (a dependency build).
2. **Guard B**: under sudo, parse the invoking user's ids, fail closed if unidentifiable.
3. find the mock dir (ancestor `mockspace.toml`) and repo root (ancestor `.git`).
4. **`run()`**, the core, which does six things:
   - `ensure_cargo_alias`: generate the **proxy crate** at `target/mockspace-proxy/` (pinned mockspace git
     dep, resolved from `mock/Cargo.lock`) and write the `.cargo/config.toml` `mock` alias, plus the
     mock-dir alias.
   - `ensure_generated_hooks`: the real validator hooks at `mock/target/hooks/`.
   - `ensure_durable_hooks`: the fallback hooks at `~/.config/mockspace/hooks-v<N>/`.
   - `ensure_launcher`: the `mock` / `cargo-mock` launcher in `$CARGO_HOME/bin`.
   - `ensure_gitignore`: add `target/` to `.gitignore`.
   - `check_activation`: `git config core.hooksPath` + `mockspace.mockdir`.
5. **Sudo repair**: chown everything just written back to the invoking user.
6. **rerun-if-changed** triggers (build.rs only).

The proxy exists for exactly two jobs: **pin the mockspace version per repo** (its git rev, read from the
lockfile) and **be a `cargo run` target**. Both are dissolvable.

## The answer: dissolve the proxy

Yes. And dissolving it removes the build.rs requirement and the `.cargo` alias in the same move, because
all three exist only to answer "how does `cargo mock` find and run the correctly-pinned mockspace." A
launcher that resolves the pin and runs a per-version cached build answers that directly.

### The new shape

**The pin moves to `mockspace.toml`.** A `[mockspace]` block declares the source explicitly:

```toml
[mockspace]
git = "ssh://git@github.com/hiisi-digital/mockspace.git"
rev = "<sha>"        # or branch = "dev" / tag = "0.0.0-d05"
```

This replaces the implicit pin-via-build-dependency-lockfile. It is more honest (the version a repo runs is
stated where a reader looks) and it removes the reason the mock crates build-depend on mockspace at all.

**The launcher build-caches mockspace per version in the home config dir** (op's suggestion, adopted). On
invocation the launcher:

1. finds the repo root and mock dir by walking up.
2. reads `[mockspace]` from `mockspace.toml` and forms a version key (hash of git+rev).
3. if `~/.cache/mockspace/builds/<key>/bin/mockspace` is missing, builds it once:
   `cargo install --git <url> --rev <rev> --root ~/.cache/mockspace/builds/<key> mockspace`. Cached and
   shared across every repo pinned to that version.
4. execs `~/.cache/mockspace/builds/<key>/bin/mockspace --dir "$root/<mockdir>" "$@"`, absolute paths, so
   cwd never matters.

No `target/mockspace-proxy`. No `.cargo/config.toml` alias. No `build.rs` bootstrap call. `cargo mock`
works because `cargo-mock` is on PATH (cargo's external-subcommand convention); `mock` works as the short
form.

### Parity mapping under the dissolve

| bootstrap action today | under the launcher |
|---|---|
| proxy crate | GONE (launcher runs the cached per-version binary) |
| `.cargo/config.toml` alias + mock-dir alias | GONE (`cargo-mock` on PATH) |
| build.rs call + rerun triggers | GONE (launcher is the trigger) |
| generated validator hooks | STAYS, written by `run()` on invocation |
| durable fallback hooks | STAYS |
| gitignore | STAYS |
| activation (`core.hooksPath`) | STAYS |
| launcher install | becomes launcher self-update (rewrites itself when the version's mockspace ships a newer launcher) |
| cache-dir guard | GONE (no dependency-build path to guard) |
| sudo guard + repair | STAYS where the launcher writes as root; smaller surface (no proxy/alias/target to chown) |

Everything load-bearing (hooks, durability, activation, sudo safety) is preserved. Only the
proxy/alias/build.rs plumbing is removed, and that plumbing was the fragile part (the cwd bug, the
target-clean disappearance, the cache poisoning all trace back to it).

## The chicken-and-egg, solved

The one hard problem: the launcher must be on PATH before it can do anything, and it is what installs
everything.

- **Existing repos**: already have the launcher. `ensure_launcher` (shipped in #267) installed `mock` /
  `cargo-mock` on the last build.rs bootstrap. They migrate by dropping the build-dep and adding the
  `[mockspace]` pin; the launcher takes over.
- **Fresh clone / new machine**: one bootstrap step installs the launcher. Options for that step:
  1. `cargo install --git <mockspace> --bin cargo-mock` (a tiny published-by-git launcher bin).
  2. a committed `mock/install.sh` a reader runs once (writes the shell launcher to `$CARGO_HOME/bin`).
  3. a curl one-liner in the repo README.

  The launcher is version-agnostic and rarely changes, so this is a genuine once-per-machine cost, the
  rustup tradeoff op has accepted.

## The migration (high blast radius)

Every consumer repo (arvo, hilavitkutin, notko, vehje, viola x3, mockspace, the lint pack) changes:

1. Add `[mockspace]` pin to `mockspace.toml`.
2. Remove the `mockspace` build-dependency and the `bootstrap_from_buildscript()` call from `build.rs`
   (often the whole `build.rs`).
3. First `cargo mock` run cleans up the now-orphan `target/mockspace-proxy` and the `.cargo/config.toml`
   alias (a one-time `bootstrap::run` migration path).

This is coordinated, reversible per repo (keep build.rs until the launcher is proven), and staged: mockspace
itself first, then one consumer as the canary, then the rest.

## Risks, stated

- The launcher becomes load-bearing and now **builds mockspace** (network + compile). A build failure or a
  bad pin blocks `cargo mock` entirely. Mitigation: clear errors, and the cached binary persists so only a
  version bump triggers a rebuild.
- **Version-cache growth** in `~/.cache/mockspace/builds/`. Mitigation: a `mock gc` that keeps the N most
  recent, or prunes builds no repo pins.
- **The one-time launcher install** is a new manual step for a fresh machine. Mitigated by the existing
  install path during the transition, but it is a real change from zero-touch.
- **Losing build-time currency**: build.rs re-bootstrapped on every build; the launcher bootstraps on every
  `cargo mock`. In practice hooks/activation only need setup once and on version change, so this is not a
  regression, but it is a behaviour change worth naming.

## What becomes easier (op's "a whole lot")

The cwd bug (topic that shipped a two-config workaround) disappears. The target-clean gate-vanish
disappears (no proxy to lose; the cached binary is in home). The cargo-cache poisoning surface disappears
(no dependency-build bootstrap). The `.cargo/config.toml` committed-file churn disappears. Three of the
rounds shipped today were patching symptoms of the proxy/build.rs design; this removes the cause.

## Three refinements (op, 2026-07-19)

### 1. A fallback gate for contributors without cargo-mock

A contributor who has not installed cargo-mock must still be blocked from committing changes **inside
`mock/`**, with a message telling them to install it. Changes **outside `mock/`** pass freely, so a
third-party contributor can touch the real code without understanding mockspace at all. This is the
"fail closed for the design surface, open for everything else" requirement.

The mechanism is a **committed, pure-shell hook** rather than the home-config-only durable hook. Commit the
hooks to `mock/hooks/` (version-controlled, present on clone, no cargo-mock dependency for the fallback
path); `core.hooksPath` points there. The hook logic:

- staged change touches `mock/`, and cargo-mock is available: delegate full validation to it.
- staged change touches `mock/`, and cargo-mock is absent: **block** with
  "this changes `mock/`, which the mockspace gate governs. Install it: `cargo install cargo-mock`. Changes
  outside `mock/` are unaffected."
- staged change is outside `mock/`: allow.

This also simplifies the durability story: committed `mock/hooks/` survives a `target/` clean AND a fresh
clone (present in the tree), so the home-config durable-hook layer from round `202607191001` is subsumed.
The one irreducible residual is git's own: `core.hooksPath` is local config, not cloned, so a brand-new
clone has no gate until the path is set (auto by the first cargo-mock run, or one documented
`git config core.hooksPath mock/hooks`).

### 2. Distribution: publish cargo-mock to crates.io

`cargo install cargo-mock` from crates.io. The thin, version-agnostic launcher is a small published crate;
it git-installs the per-repo pinned mockspace itself. mockspace-the-engine stays `publish = false`; only the
launcher publishes.

### 3. Auto-migration in the cargo-mock binary (op's insight, correct)

The migration is not manual per repo. cargo-mock, on its first run in a repo, detects and removes the legacy
artifacts itself: the orphan `target/mockspace-proxy`, the `.cargo/config.toml` `mock` alias (and the
mock-dir alias), and the `build.rs` bootstrap call (deleting `build.rs` when it holds only that call, else
removing the call plus the build-dependency). It writes the `[mockspace]` pin into `mockspace.toml` from the
resolved version and sets `core.hooksPath`.

The one apparent circularity dissolves: while the alias still exists, `cargo mock` runs the alias (the old
proxy), not the launcher, because cargo resolves an alias before an external `cargo-mock` on PATH. But
`mock` (the short form) is a direct PATH binary and bypasses the alias entirely. So the migration runs on
the first `mock` invocation, removes the alias, and from then on `cargo mock` reaches the launcher too. The
"manual" step reduces to "run `mock` once instead of `cargo mock`," and even that is smoothable with a
printed hint if someone runs the stale alias. Nothing is missed; the auto-migration is the plan.

## Recommendation

Path B (dissolve), with the three refinements. Build the mockspace side first (launcher does pin-resolve +
per-version home cache + bootstrap parity + auto-migration; committed `mock/hooks/` with the no-cargo-mock
fallback; publish the launcher), prove it on mockspace itself, then let the auto-migration carry the
consumer repos as each is touched. The only genuinely manual, coordinated act is publishing the launcher and
running `mock` once per repo, both cheap.

## Domain review response (2026-07-19)

A neutral architect review found five real gaps. Resolutions, each folded into the design:

**1. Custom lints were a third proxy job the design missed (load-bearing).** `ensure_proxy_crate` compiles
repo-specific `mock/lints/*.rs` and `[lint-crates]` deps into the per-repo binary via a `[patch]` block and
`generate_custom_lint_main`. Verified in use: arvo and hilavitkutin each ship 6 `mock/lints/*.rs`; most repos
reference the shared lint pack. A binary keyed only on mockspace `(git, rev)` and shared across repos cannot
carry repo-specific lints.

Resolution, v1: the cache key is a hash of the **full compilation input**, not just mockspace's revision:
`hash(mockspace git+rev, resolved [lint-crates] deps, mock/lints/*.rs bytes)`. Repos with identical lint
configuration share a binary (all repos using only the stack pack at the same pin share one); repos with
custom lints (arvo, hilavitkutin) get their own keyed binary. Correctness preserved; sharing is per
lint-config rather than universal, still far fewer builds than per-repo proxies.

Resolution, v2 (future): runtime-loaded lint dylibs at the cdylib boundary v2 already designs
(`lint-catalog-cdylib-boundary` research), so the mockspace binary is truly shared and lints load at run
time. The v1 keyed-binary approach is the bridge; the v2 dylib approach is the end state.

**2. The `mock/hooks/` gate cannot protect a fresh clone by a stranger, and CI is the real backstop.**
Correct, and it is the same truth Fiedler established earlier: client-side hooks are opt-in
(`core.hooksPath` is local, never cloned), so they can never gate an untrusted fresh clone. The honest
framing: the committed `mock/hooks/` gate is defense-in-depth for a **developer who has set up once** (it
catches their own out-of-discipline commits and prompts install), NOT a security boundary against strangers.
The real boundary for untrusted contributions is CI plus branch protection, which the design now names
explicitly as the backstop. The gate's value is real but bounded, and the design must not claim more.

**3. Branch pins have no cache semantics.** `branch = "dev"` carries no rev; the cache key needs one.
Resolution: reuse the existing remote-check-TTL machinery (`REMOTE_CHECK_TTL`, `git_ls_remote_head`,
`proxy_auto_update`). A branch pin resolves to a concrete rev via `git ls-remote`, keys the cache by that
resolved rev, and re-resolves only when the TTL expires (24h default), exactly as the proxy freshness check
works today. A rev or tag pin is immutable and never re-checked. So branch pins get periodic currency
without a network round-trip per invocation; the staleness-detection mechanism that exists today gets a
direct successor rather than being dropped.

**4. Concurrency and disk growth were unaddressed.** Resolution: a flock per version key during the build
(v2 already specifies `.git/mockspace/.lock` flock semantics; the build cache reuses the pattern under
`~/.cache/mockspace/builds/.locks/<key>`), building into a temp dir and atomically renaming the finished
`bin/` into place so a racing reader never sees a half-built binary. `cargo install` installs only the
binary to `--root/bin` (its build tree is a temp target it discards), so growth is one binary per key, not a
full build tree; a `mock cache prune` (aligned with v2's `mock cache prune` and 90/365-day eviction policy)
handles retention. Both mechanisms are named in v2 already; the build cache adopts them rather than
inventing.

**5. Auto-migration must not silently rewrite build.rs.** Resolution: migration is an explicit `mock
migrate` command with a `--dry-run` default preview, not a silent action on every invocation. It removes the
proxy and alias unconditionally (regenerable, safe), but only deletes `build.rs` when its content is a byte
-exact match to the known bootstrap template; any other `build.rs` is left in place with a warning naming the
line to remove by hand. Every mutation is one reviewable commit the developer approves, not an invisible
edit.

The review's verdict was "sound with these fixes"; with the five resolved, the approach stands. Its
one-line summary of what the design gets right (the proxy conflates pinning and run-target; today's bugs
trace to the build.rs/proxy design; the alias-precedence migration reasoning is correct) is retained.

## The v1 / v2 split at the meta and schema level

op's requirement: the launcher handles both v1 and v2, and later performs the v1-to-v2 migration. So the
split must be crisp at the schema level, and the dissolve-proxy design must not conflict with v2's
fundamentals. Both checked against the v2 spec (`202605181400_mockspace-v2-spec.md`).

### The discriminator

A repo is **v1** or **v2**, detected in this order:

1. an explicit `[workflow] version = 1 | 2` field in `mockspace.toml` (authoritative when present).
2. else, presence of `refs/mock/round/*` orphan refs and a `.mock/` rendered surface implies v2.
3. else, flat `mock/design_rounds/*.md` files and a committed `mock/mockspace.toml` imply v1.

Every workspace repo is v1 today. v2 is specced but unbuilt. The launcher reads the discriminator once per
invocation and dispatches to the v1 or v2 engine path.

### Where state and config live, per version (and why the build cache is version-agnostic)

| concern | v1 | v2 |
|---|---|---|
| round state | flat files in `mock/design_rounds/` | orphan `refs/mock/round/<slug>` + `.phase` blob + manifests |
| config | committed `mock/mockspace.toml` | harness ref, rendered to `.mock/mockspace.toml` + `mockspace.lock` |
| the pin | `mock/mockspace.toml [mockspace]` | harness-ref config / `mockspace.lock` |
| hooks | committed `mock/hooks/` (op's refinement) | rendered `.mock/hooks/` from the harness ref |
| per-project internals | `mock/target/` (regenerable) | `.git/mockspace/` (index.bin, locks, undo) |
| machine content cache | (n/a today) | `~/.cache/mockspace/` (imports, helpers) |
| per-developer config | (n/a today) | `~/.config/mockspace/` (trust.toml) |
| **the per-version mockspace build cache** | **`~/.cache/mockspace/builds/<key>/`** | **same** |

The build cache is **version-agnostic**: both v1 and v2 launchers resolve a pin and cache a built binary the
same way, in `~/.cache/mockspace/builds/`. This is the one shared mechanism, and it sits in v2's own
taxonomy slot (`~/.cache`, machine-global cache) rather than the config dir. The earlier draft placed it in
`~/.config/mockspace/builds/`, which conflicted with v2's reservation of `~/.config` for per-developer config
(trust). Corrected to `~/.cache`.

### Conflicts with v2 fundamentals, checked

- **Build cache dir**: was `~/.config`, corrected to `~/.cache` to match v2's XDG split. Resolved.
- **Hooks**: the v1 committed `mock/hooks/` refinement diverges from v2's rendered `.mock/hooks/`. This is a
  deliberate per-version difference, not a conflict: v1 has no `.mock/` rendered surface, so committed hooks
  are the v1-correct shape; v2 renders hooks from the harness ref into the gitignored `.mock/hooks/`. The
  launcher, knowing the version, points `core.hooksPath` at the right one. The durable-hooks-in-`~/.config`
  approach from round `202607191001` is retired in favour of the version-appropriate location, which is the
  one piece of today's shipped work this design supersedes.
- **Discovery**: v2 walks up for `.git` and honours `MOCK_ROOT`. The launcher must honour `MOCK_ROOT` too
  (it does not yet). Folded in.
- **The `~/.cache/mockspace/helpers/` slot** ("baseline helper scripts extracted from binary on first run")
  is where v2 already envisions the binary dropping shell helpers. The launcher shell script itself is such
  a helper; extracting/refreshing it there aligns with v2 rather than fighting it.

### The launcher's expanded responsibilities

1. resolve repo root (walk up for `.git`, honour `MOCK_ROOT`).
2. detect v1 vs v2 (the discriminator above).
3. resolve the pin (v1: `mock/mockspace.toml`; v2: harness ref / lock).
4. resolve a branch pin to a rev with TTL currency (finding 3).
5. compute the full-input cache key (finding 1) and build-cache the binary under a flock (finding 4).
6. run the binary with the version-appropriate arguments.
7. `mock migrate` (explicit): legacy proxy/alias/build.rs cleanup for v1 (finding 5).
8. later: `mock migrate --to v2` (flat files to refs), when v2 is built.

### Scope for the implementation-after-compaction

Build the v1 path only, per op. The v2 columns above are the compatibility contract the v1 launcher is
written not to preclude: the discriminator is checked (defaulting to v1), the build cache and `MOCK_ROOT`
handling are v2-shaped from the start (they cost nothing extra), and the `mock migrate --to v2` command is a
named stub, not built. Nothing in the v1 launcher hardcodes an assumption v2 would have to unwind.
