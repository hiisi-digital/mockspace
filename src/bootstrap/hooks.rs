#![allow(unused_imports)]
use super::*;

/// Where generated hooks live. Build artifact, gitignored.
pub(crate) fn generated_hooks_dir(mock_dir: &Path) -> PathBuf {
    mock_dir.join("target").join("hooks")
}

pub(crate) fn ensure_generated_hooks(repo_root: &Path, mock_dir: &Path, actions: &mut Vec<String>) {
    let out_dir = generated_hooks_dir(mock_dir);
    let _ = fs::create_dir_all(&out_dir);

    let mock_rel = mock_dir
        .strip_prefix(repo_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| mock_dir.display().to_string());

    // Path from the generated hooks dir to .git/hooks/ (the user's hooks).
    // The generated hooks source these so user logic always runs.
    let git_dir = resolve_git_dir(repo_root);
    let user_hooks_dir = git_dir.join("hooks");

    for hook_name in HOOK_NAMES {
        let path = out_dir.join(hook_name);
        let user_hook = user_hooks_dir.join(hook_name);

        let content = gen_hook(hook_name, &mock_rel, &user_hook);
        let fingerprint = content_fingerprint(&content);
        let fp_line = format!("{MANAGED_MARKER} v{HOOK_VERSION} fp:{fingerprint:016x}");

        // Skip if already up-to-date.
        if path.exists() {
            if let Ok(current) = fs::read_to_string(&path) {
                if current.contains(&fp_line) {
                    continue;
                }
            }
        }

        let final_content = content.replacen(MANAGED_MARKER, &fp_line, 1);

        if let Err(e) = fs::write(&path, &final_content) {
            actions.push(format!("failed to write {hook_name} hook: {e}"));
            continue;
        }

        #[cfg(unix)]
        {
            if let Ok(meta) = fs::metadata(&path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&path, perms);
            }
        }

        actions.push(format!("generated {hook_name} hook"));
    }
}

pub(crate) fn gen_hook(name: &str, mock_rel: &str, user_hook: &Path) -> String {
    match name {
        "pre-commit" => gen_pre_commit(mock_rel, user_hook),
        "pre-push" => gen_pre_push(mock_rel, user_hook),
        "commit-msg" => gen_commit_msg(user_hook),
        _ => String::new(),
    }
}

/// The commit-msg body: hand the message to the configured message lints.
///
/// Replaces a hardcoded `grep -E` that was baked into two hook layers under a
/// comment conceding the copies "MUST stay in sync". They could not, and the
/// baked pattern contradicted configuration outright, rejecting unconditionally
/// what `[attribution] autonomous` was meant to require. Policy now lives in one
/// place and every surface reaches it through the same command.
///
/// With no launcher installed this fails closed and says how to install one,
/// rather than falling back to a second policy that can disagree with the first.
pub(crate) fn message_commit_msg_body() -> String {
    r##"MSG_FILE="$1"
[ -z "$MSG_FILE" ] && exit 0
[ -f "$MSG_FILE" ] || exit 0

launcher=""
if command -v mock >/dev/null 2>&1; then launcher="mock"
elif command -v cargo-mock >/dev/null 2>&1; then launcher="cargo-mock"
fi

if [ -z "$launcher" ]; then
    echo "" >&2
    echo "BLOCKED: the message gate cannot run: no mockspace launcher on PATH." >&2
    echo "" >&2
    echo "  commit-message policy (authorship trailers, commit style) is configured" >&2
    echo "  in mock/agent/config.toml and enforced by the engine, so it needs the" >&2
    echo "  launcher. Guessing a weaker policy here would contradict the configured" >&2
    echo "  one, so the gate refuses instead." >&2
    echo "" >&2
    echo "  install it:  cargo install cargo-mock" >&2
    exit 1
fi

"$launcher" check-message --domain commit-message --gate commit --file "$MSG_FILE" || exit 1
"##
    .to_string()
}

/// The pre-push message-scan body. Expects `$PREPUSH_STDIN` to hold the captured
/// `<local-ref> <local-sha> <remote-ref> <remote-sha>` lines.
///
/// Every message being pushed goes through the same configured lints the
/// commit-msg gate uses, at the push tier, so a project can warn locally and
/// block before anything is shared.
///
/// Each message is validated on its own. `check-message` parses its input as
/// one message with a subject line, so a batch of them cannot be concatenated
/// and checked in a single call: the blob's first line becomes the subject and
/// every later subject is read as body text.
pub(crate) fn message_prepush_scan_body() -> String {
    r##"launcher=""
if command -v mock >/dev/null 2>&1; then launcher="mock"
elif command -v cargo-mock >/dev/null 2>&1; then launcher="cargo-mock"
fi

if [ -z "$launcher" ]; then
    echo "" >&2
    echo "BLOCKED: the message gate cannot run: no mockspace launcher on PATH." >&2
    echo "  install it:  cargo install cargo-mock" >&2
    exit 1
fi

# Collect the revisions being pushed, not their messages. Each message is
# validated on its own below: `check-message` parses its input as ONE message
# with a subject line, so concatenating N of them makes the blob's own first
# line the subject (always empty, since the accumulator starts empty) and
# reads every subject after the first as body text. That combination blocked
# every push while checking no subject at all.
PUSH_REVS=""
while IFS=' ' read -r _bl_ref bl_local _bl_rref bl_remote; do
    if [ -z "$bl_local" ]; then
        continue
    fi
    if [ "$bl_local" = "0000000000000000000000000000000000000000" ]; then
        continue
    fi
    if [ "$bl_remote" = "0000000000000000000000000000000000000000" ] || ! git cat-file -e "$bl_remote" 2>/dev/null; then
        # New branch or unknown remote: scan commits not already on any remote.
        # Bounded by `--not --remotes`; when the remote has never been fetched
        # this widens to all local history, which is the safe (over-)inclusive
        # direction for a gate of this kind.
        RANGE_REVS=$(git rev-list "$bl_local" --not --remotes 2>/dev/null || true)
    else
        RANGE_REVS=$(git rev-list "$bl_remote".."$bl_local" 2>/dev/null || true)
    fi
    PUSH_REVS="$PUSH_REVS
$RANGE_REVS"
done <<< "$PREPUSH_STDIN"

# Several refs in one push routinely share commits; validate each once.
PUSH_REVS=$(printf '%s\n' "$PUSH_REVS" | grep -v '^[[:space:]]*$' | sort -u || true)

if [ -n "$PUSH_REVS" ]; then
    # One batched call rather than one per commit: launcher startup dominates
    # (measured at 468ms), so a few hundred commits would cost minutes spawned
    # per message. `--batch` splits on NUL and labels each record with the sha
    # before an 0x1f, so a rejection names the commit it came from.
    printf '%s\n' "$PUSH_REVS" \
        | while IFS= read -r rev; do
            [ -z "$rev" ] && continue
            printf '%s\037' "$(git log -1 --format='%h %s' "$rev" 2>/dev/null)"
            git log -1 --format=%B "$rev" 2>/dev/null
            printf '\000'
        done \
        | "$launcher" check-message --domain commit-message --gate push --batch || exit 1
fi
"##
    .to_string()
}

/// Generate the commit-msg hook: rejects agent authorship bylines and
/// tool-advertising trailers in the commit message.
///
/// A git hook fires regardless of how the commit is made (bash `git`, an
/// editor, an MCP git tool), so it closes the gap that a bash-only PreToolUse
/// hook leaves open. Byline attribution is reserved for genuinely headless
/// autonomous work; a commit made under human direction carries none. Human
/// co-authors are kept.
pub(crate) fn gen_commit_msg(user_hook: &Path) -> String {
    let user_section = source_user_hook(user_hook);
    let byline = message_commit_msg_body();

    format!(
        r##"#!/usr/bin/env bash
{MANAGED_MARKER}
# Generated by mockspace. User hooks sourced from .git/hooks/.

set -e

{user_section}
{byline}
exit 0
"##
    )
}

/// Generate the source-user-hook preamble. This runs the user's original
/// `.git/hooks/<name>` if it exists, so their hooks always execute
/// regardless of whether mockspace is active.
pub(crate) fn source_user_hook(user_hook: &Path) -> String {
    let path = user_hook.display();
    format!(
        r#"# Run the user's original hook if it exists.
USER_HOOK="{path}"
if [ -x "$USER_HOOK" ]; then
    "$USER_HOOK" "$@" || exit $?
fi
"#
    )
}

pub(crate) fn gen_pre_commit(mock_rel: &str, user_hook: &Path) -> String {
    let user_section = source_user_hook(user_hook);

    format!(
        r##"#!/usr/bin/env bash
{MANAGED_MARKER}
# Generated by mockspace. User hooks sourced from .git/hooks/.

set -e

{user_section}
MOCK_DIR="{mock_rel}"

# Only run mockspace validation when staged files touch the mock workspace.
STAGED=$(git diff --cached --name-only -- "$MOCK_DIR" 2>/dev/null || true)
[ -z "$STAGED" ] && exit 0

echo "pre-commit: mockspace changes detected, running validation..."

# Extract changed crate names from staged paths.
CHANGED_CRATES=$(echo "$STAGED" \
    | grep "^$MOCK_DIR/crates/" \
    | sed "s|^$MOCK_DIR/crates/||" \
    | cut -d/ -f1 \
    | sort -u \
    | tr '\n' ',' \
    | sed 's/,$//' \
    || true)

ARGS=(--lint-only --commit)

if [ -n "$CHANGED_CRATES" ]; then
    STAGED_RS=$(echo "$STAGED" \
        | grep "^$MOCK_DIR/crates/.*\.rs$" \
        || true)

    if [ -z "$STAGED_RS" ]; then
        echo "  crates: $CHANGED_CRATES (doc-only)"
        ARGS+=(--scope "$CHANGED_CRATES" --doc-only)
    else
        echo "  crates: $CHANGED_CRATES"
        ARGS+=(--scope "$CHANGED_CRATES")
    fi
else
    echo "  infrastructure-only (no crate files staged)"
    ARGS+=(--scope infra)
fi

if ! cargo mock "${{ARGS[@]}}" 2>&1; then
    echo ""
    echo "BLOCKED: mockspace validation failed."
    exit 1
fi

echo "pre-commit: validation passed."
"##
    )
}

pub(crate) fn gen_pre_push(mock_rel: &str, user_hook: &Path) -> String {
    let user_section = source_user_hook(user_hook);
    let byline = message_prepush_scan_body();

    format!(
        r##"#!/usr/bin/env bash
{MANAGED_MARKER}
# Generated by mockspace. User hooks sourced from .git/hooks/.

set -e

{user_section}
MOCK_DIR="{mock_rel}"

# Git feeds `<local-ref> <local-sha> <remote-ref> <remote-sha>` lines on
# stdin. Capture once so both the byline scan and the crate-diff loop below
# can read it (a here-string keeps the loop in this shell so its vars persist).
PREPUSH_STDIN=$(cat)

# Byline safety net (runs first, before any launcher work, so it holds
# regardless of install state). The commit-msg hook catches bylines at commit
# time; this catches anything that reached the branch another way.
{byline}
echo "pre-push: running mockspace validation..."

# Compute changed crates between remote and local across every ref being
# pushed. Git pre-push hooks receive `<local-ref> <local-sha> <remote-ref>
# <remote-sha>` lines on stdin (per `git help githooks`).
#
# - Existing branches: diff `<remote-sha>..<local-sha>` for crate-path
#   changes; union across all pushed refs.
# - New branches (remote-sha = all zeros): no upstream reference; fall
#   back to full project scope so we don't miss anything.
# `set -e` is in effect from the prelude. `&&-continue` short-circuit
# in the loop body would exit the script if the left side is false, so
# use explicit `if` blocks for every short-circuit.
#
# Empty stdin (no refs piped, rare with `push --tags` variants on some
# git versions) → the loop simply does not run; the post-loop fall-
# through handles full-scope correctly.
NEW_BRANCH=0
CHANGED_CRATES=""
while IFS=' ' read -r _local_ref local_sha _remote_ref remote_sha; do
    if [ -z "$local_sha" ]; then
        continue
    fi
    # Delete-only push (local_sha all zeros): nothing to lint.
    if [ "$local_sha" = "0000000000000000000000000000000000000000" ]; then
        continue
    fi
    if [ "$remote_sha" = "0000000000000000000000000000000000000000" ]; then
        NEW_BRANCH=1
        break
    fi
    # Unknown remote_sha (we never fetched the remote, or it's a stale
    # sha that no longer exists locally): cat-file -e returns non-zero.
    # Treat as new-branch-equivalent and force full scope; otherwise
    # `git diff` would fail closed and silently drop this ref's changes.
    if ! git cat-file -e "$remote_sha" 2>/dev/null; then
        NEW_BRANCH=1
        break
    fi
    PUSH_CHANGED=$(git diff --name-only "$remote_sha".."$local_sha" -- "$MOCK_DIR/crates/" 2>/dev/null \
        | sed "s|^$MOCK_DIR/crates/||" \
        | cut -d/ -f1 \
        | sort -u \
        | tr '\n' ',' \
        | sed 's/,$//' \
        || true)
    if [ -z "$PUSH_CHANGED" ]; then
        continue
    fi
    if [ -z "$CHANGED_CRATES" ]; then
        CHANGED_CRATES="$PUSH_CHANGED"
    else
        CHANGED_CRATES="$CHANGED_CRATES,$PUSH_CHANGED"
    fi
done <<< "$PREPUSH_STDIN"

# De-duplicate the comma-joined crate list (multiple refs may touch the
# same crate). tr-sort-uniq round trip.
if [ -n "$CHANGED_CRATES" ]; then
    CHANGED_CRATES=$(echo "$CHANGED_CRATES" \
        | tr ',' '\n' \
        | sort -u \
        | grep -v '^$' \
        | tr '\n' ',' \
        | sed 's/,$//')
fi

if grep -rq "Nuked by" "$MOCK_DIR/crates/"*/src/lib.rs 2>/dev/null; then
    echo "  nuked workspace: skipping source checks"
    ARGS=(--lint-only --strict --doc-only)
elif [ "$NEW_BRANCH" = "1" ] || [ -z "$CHANGED_CRATES" ]; then
    echo "  scope: full project ($([ "$NEW_BRANCH" = "1" ] && echo "new branch" || echo "no crate changes"))"
    ARGS=(--lint-only --strict)
else
    echo "  scope: $CHANGED_CRATES"
    ARGS=(--lint-only --strict --scope "$CHANGED_CRATES")
fi

if ! cargo mock "${{ARGS[@]}}" 2>&1; then
    echo ""
    echo "BLOCKED: mockspace validation failed."
    exit 1
fi

echo "pre-push: validation passed."
"##
    )
}

// ──────────────────────────────────────────────────────────────────────
// Utilities
// ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod byline_hook_tests {
    use super::*;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    fn scratch(tag: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("mock_byline_{tag}_{}_{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Write `content` as an executable hook and run it (args, stdin, cwd, PATH
    /// all controllable). Returns the exit code.
    fn run_script(content: &str, args: &[&Path], stdin: &str, cwd: Option<&Path>, path_env: Option<&str>) -> i32 {
        let dir = scratch("hook");
        let hook = dir.join("hook.sh");
        std::fs::write(&hook, content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = std::fs::metadata(&hook).unwrap().permissions();
            p.set_mode(0o755);
            std::fs::set_permissions(&hook, p).unwrap();
        }
        let mut cmd = Command::new(&hook);
        cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
        if let Some(c) = cwd {
            cmd.current_dir(c);
        }
        if let Some(p) = path_env {
            cmd.env("PATH", p);
        }
        let mut child = cmd.spawn().unwrap();
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        let code = child.wait().unwrap().code().unwrap_or(-1);
        let _ = std::fs::remove_dir_all(&dir);
        code
    }

    /// Run `msg` through BOTH the generated and the durable commit-msg hooks,
    /// assert they agree (both layers must enforce), return the shared code.
    /// Run `msg` through the generated commit-msg hook.
    ///
    /// The generated hook is the byline authority. The durable hook no longer
    /// duplicates the check: it delegates to this one when the repo is
    /// initialised and blocks when it is not, which `durable_gate_tests` covers.
    /// Comparing the two layers, as this helper used to, asserted a duplication
    /// that was itself the problem.
    fn run_commit_msg(msg: &str) -> i32 {
        let dir = scratch("msg");
        let msgfile = dir.join("COMMIT_EDITMSG");
        std::fs::write(&msgfile, msg).unwrap();
        let code = run_script(
            &gen_commit_msg(&dir.join("no-user-hook")),
            &[&msgfile],
            "",
            None,
            None,
        );
        let _ = std::fs::remove_dir_all(&dir);
        code
    }

    /// A PATH with git + coreutils but WITHOUT the cargo bin, so the durable
    /// pre-push prelude takes the no-launcher branch deterministically.
    fn launcher_free_path() -> String {
        let out = Command::new("sh").arg("-c").arg("command -v git").output().unwrap();
        let gitpath = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let gitdir = Path::new(&gitpath).parent().map(|p| p.display().to_string()).unwrap_or_default();
        format!("{gitdir}:/usr/local/bin:/usr/bin:/bin")
    }

    /// A temp git repo with one commit carrying `msg`. Returns (dir, head_sha).
    fn repo_with_commit(msg: &str) -> (PathBuf, String) {
        let dir = scratch("repo");
        let git = |args: &[&str]| {
            Command::new("git")
                .current_dir(&dir)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .unwrap()
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("f.txt"), "x").unwrap();
        git(&["add", "."]);
        let mf = dir.join("MSG");
        std::fs::write(&mf, msg).unwrap();
        git(&["commit", "-q", "--no-verify", "-F", mf.to_str().unwrap()]);
        let out = git(&["rev-parse", "HEAD"]);
        let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (dir, sha)
    }

    /// Run the durable pre-push hook against a repo whose HEAD carries `msg`,
    /// as a new-branch push (remote-sha all zeros). Returns the exit code.
    fn run_durable_pre_push(msg: &str) -> i32 {
        let (repo, sha) = repo_with_commit(msg);
        let zero = "0".repeat(40);
        let stdin = format!("refs/heads/main {sha} refs/heads/main {zero}\n");
        let code = run_script(&mockspace_manifest::gate::durable_hook("pre-push", HOOK_VERSION), &[], &stdin, Some(&repo), Some(&launcher_free_path()));
        let _ = std::fs::remove_dir_all(&repo);
        code
    }

    // The hook's contract is now that it delegates to the configured message
    // lints. What those lints decide is their own concern and is tested where
    // they live, in the pack; duplicating policy assertions here would recreate
    // the very drift this change removed.

    /// Run the generated commit-msg hook with `launcher` first on PATH.
    ///
    /// `launcher` is a stub script standing in for `mock`, so the hook's own
    /// behaviour is observable without building or invoking the real engine.
    fn run_commit_msg_with_launcher(msg: &str, stub: Option<&str>) -> i32 {
        let dir = scratch("msg");
        let msgfile = dir.join("COMMIT_EDITMSG");
        std::fs::write(&msgfile, msg).unwrap();

        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        if let Some(body) = stub {
            let mock = bin.join("mock");
            std::fs::write(&mock, body).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&mock).unwrap().permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&mock, perms).unwrap();
            }
        }
        // Only the stub dir plus the bare minimum, so a real `mock` installed on
        // this machine cannot leak into the test.
        let path = format!("{}:/usr/bin:/bin", bin.display());
        let code = run_script(
            &gen_commit_msg(&dir.join("no-user-hook")),
            &[&msgfile],
            "",
            None,
            Some(&path),
        );
        let _ = std::fs::remove_dir_all(&dir);
        code
    }

    #[test]
    fn the_commit_msg_hook_hands_the_message_to_the_launcher() {
        // The stub records its arguments and passes, so this asserts the exact
        // command shape the engine is invoked with.
        let dir = scratch("args");
        let out = dir.join("args.txt");
        let stub = format!(
            "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" > {}\nexit 0\n",
            out.display()
        );
        assert_eq!(run_commit_msg_with_launcher("feat: x", Some(&stub)), 0);
        let recorded = std::fs::read_to_string(&out).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(recorded.contains("check-message"), "got: {recorded}");
        assert!(recorded.contains("--domain commit-message"), "got: {recorded}");
        assert!(recorded.contains("--gate commit"), "got: {recorded}");
        assert!(recorded.contains("--file"), "got: {recorded}");
    }

    #[test]
    fn the_commit_msg_hook_fails_when_the_lints_reject() {
        let stub = "#!/usr/bin/env bash\nexit 1\n";
        assert_eq!(run_commit_msg_with_launcher("feat: x", Some(stub)), 1);
    }

    #[test]
    fn the_commit_msg_hook_fails_closed_with_no_launcher() {
        // Guessing a weaker policy here would contradict the configured one, so
        // the gate refuses and says how to install the launcher instead. Same
        // treatment every other anomalous state gets.
        assert_eq!(run_commit_msg_with_launcher("feat: x", None), 1);
    }

    #[test]
    fn an_empty_or_missing_message_file_is_not_the_hooks_business() {
        // git calls commit-msg with a path; if there is nothing there, there is
        // nothing to lint, and erroring would block on git's own behaviour.
        let dir = scratch("nofile");
        let missing = dir.join("does-not-exist");
        let code = run_script(
            &gen_commit_msg(&dir.join("no-user-hook")),
            &[&missing],
            "",
            None,
            Some("/usr/bin:/bin"),
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(code, 0);
    }

    // The durable gate's own contract: discover, then delegate or block. It
    // carries no policy of its own, so none of these assert on byline content.

    #[test]
    fn durable_ignores_a_repo_it_does_not_govern() {
        // No mockspace.toml. The gate must not block, or it would hijack every
        // unrelated repo on the machine that ever had core.hooksPath set.
        assert_eq!(run_durable_pre_push("fix: normal commit"), 0);
    }

    /// A repo with a `mockspace.toml`, optionally with an executable generated
    /// hook stub, run through the durable pre-push. Returns the exit code.
    fn durable_in_project(config: &str, stub: Option<&str>) -> i32 {
        let (repo, sha) = repo_with_commit("fix: whatever");
        std::fs::write(repo.join("mockspace.toml"), config).unwrap();
        if let Some(body) = stub {
            let hooks = repo.join("mock/target/hooks");
            std::fs::create_dir_all(&hooks).unwrap();
            let path = hooks.join("pre-push");
            std::fs::write(&path, body).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut p = std::fs::metadata(&path).unwrap().permissions();
                p.set_mode(0o755);
                std::fs::set_permissions(&path, p).unwrap();
            }
        }
        let zero = "0".repeat(40);
        let stdin = format!("refs/heads/main {sha} refs/heads/main {zero}\n");
        let code = run_script(
            &mockspace_manifest::gate::durable_hook("pre-push", HOOK_VERSION),
            &[],
            &stdin,
            Some(&repo),
            Some(&launcher_free_path()),
        );
        let _ = std::fs::remove_dir_all(&repo);
        code
    }

    #[test]
    fn durable_delegates_to_the_generated_hook_when_initialised() {
        // A stub exiting 42 proves the delegation actually happened rather than
        // the durable hook deciding for itself.
        let code = durable_in_project(
            "mock_dir = \"mock\"\n",
            Some("#!/usr/bin/env bash\nexit 42\n"),
        );
        assert_eq!(code, 42, "durable pre-push did not delegate");
    }

    #[test]
    fn durable_blocks_an_uninitialised_project_at_all_scope() {
        let code = durable_in_project(
            "mock_dir = \"mock\"\nuninitialised_blocks = \"all\"\n",
            None,
        );
        assert_eq!(code, 1, "expected a block at 'all' scope");
    }

    #[test]
    fn durable_passes_an_uninitialised_project_outside_the_surface() {
        // Default `surface` scope with nothing staged under mock/: the gate
        // governs the design surface, so unrelated work passes.
        let code = durable_in_project("mock_dir = \"mock\"\n", None);
        assert_eq!(code, 0, "surface scope must not block work outside it");
    }

    #[test]
    fn the_generated_pre_push_hook_pipes_messages_to_the_launcher() {
        // Asserted on the generated text rather than by running it: the body
        // needs a real repo with pushable refs, which the durable tests cover.
        let h = gen_pre_push("mock", std::path::Path::new("/dev/null"));
        assert!(h.contains("check-message"));
        assert!(h.contains("--domain commit-message"));
        assert!(h.contains("--gate push"));
        // fails closed rather than guessing a policy
        assert!(h.contains("no mockspace launcher on PATH"));
        assert!(!h.contains("co-authored-by"), "policy must not be baked into the hook");
    }

    #[test]
    fn the_generated_pre_push_hook_validates_each_commit_message_separately() {
        // Regression: the gate used to concatenate every message in the range
        // into one blob and hand it to `check-message`, which parses its input
        // as a single message with a subject line. The blob's first line was
        // always empty (the accumulator starts empty), so every push was
        // rejected with "the commit subject is empty", while no subject after
        // the first was ever checked at all.
        let h = gen_pre_push("mock", std::path::Path::new("/dev/null"));

        // Revisions are collected, then each message is fetched on its own.
        assert!(h.contains("git rev-list"), "must collect revs, not messages");
        assert!(
            h.contains("git log -1 --format=%B \"$rev\""),
            "each message must be fetched on its own"
        );
        assert!(h.contains("--batch"), "messages are checked per commit in one call");

        // The accumulate-then-validate-once shape must not come back.
        assert!(
            !h.contains("PUSH_MSGS"),
            "messages must not be accumulated into a single blob"
        );

        // A failing commit is named, which the blob form could not do.
        assert!(h.contains("\\037"), "each record carries its commit label");
    }
}
