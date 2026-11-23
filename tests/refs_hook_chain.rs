//! A hook of the person's own where the engine's goes is chained, never refused: `instantiate` moves
//! it beside the engine's as `<event>.chained`, the engine's runs it first with the same arguments
//! and input and exits with its status, and a second `instantiate` leaves both as they are. The
//! hooks met in the field are git-lfs's `post-checkout` and a workspace's shim that regenerates its
//! agent files after a pull.

#[path = "common/ambient.rs"]
mod ambient;
#[path = "common/exec.rs"]
mod exec;
mod refs_e2e;

use std::path::{Path, PathBuf};

use refs_e2e::{Clone, fixture};

/// What `git lfs install` writes as a repository's `post-checkout`.
const LFS_POST_CHECKOUT: &str = "#!/bin/sh\ncommand -v git-lfs >/dev/null 2>&1 || { printf >&2 \"\\n%s\\n\\n\" \"This repository is \
                                 configured for Git LFS but 'git-lfs' was not found on your path.\"; exit 2; }\ngit lfs \
                                 post-checkout \"$@\"\n";

fn hooks(c: &Clone) -> PathBuf {
    c.hooks_dir()
}

/// A program at `<base>/shims/<name>` that appends its arguments, and its input where `input`, to
/// `<base>/<name>.log` and exits with `status`; the log's path.
fn logger(c: &Clone, name: &str, input: bool, status: u8) -> PathBuf {
    let log = c.w.base().join(format!("{name}.log"));
    let read = if input { "cat >> \"$log\"\n" } else { "" };
    exec::write_executable(
        &c.w.base().join("shims").join(name),
        format!("#!/bin/sh\nlog='{}'\necho \"args: $*\" >> \"$log\"\n{read}exit {status}\n", log.display()),
    );
    log
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

/// A checkout that moves HEAD from one commit to another, as a branch switch does.
fn move_head(c: &Clone) -> refs_e2e::Out {
    let o = c.git_out(&["checkout", "-q", "--detach", "HEAD~1"]);
    assert!(read(&hooks(c).join("post-checkout")).contains("# chains: post-checkout.chained"), "{}", o.all());
    o
}

// Mutation: refuse the hook of the person's own, or write over it, or chain it and drop its
// arguments or its status. git-lfs then never checks out the large files a branch switch brings,
// or the failure it reports is swallowed.
#[test]
fn git_lfs_s_post_checkout_is_chained_and_still_runs_with_its_arguments_and_status() {
    let w = fixture::migrated_remote();
    let c = w.clone_of("a");
    let lfs = logger(&c, "git-lfs", false, 0);
    exec::write_executable(&hooks(&c).join("post-checkout"), LFS_POST_CHECKOUT);
    let o = c.run(&["data-refs", "instantiate"]);
    assert!(o.ok, "instantiate goes on over git-lfs's hook: {}", o.all());
    assert_eq!(read(&hooks(&c).join("post-checkout.chained")), LFS_POST_CHECKOUT, "git-lfs's hook is kept whole, beside");
    c.ok(&["data-refs", "verify"]);
    let from = c.git(&["rev-parse", "HEAD"]).trim().to_string();
    let o = move_head(&c);
    assert!(o.ok, "{}", o.all());
    let to = c.git(&["rev-parse", "HEAD"]).trim().to_string();
    assert_eq!(read(&lfs), format!("args: post-checkout {from} {to} 1\n"), "git-lfs ran once, with git's arguments");
    // a failing chained hook fails the checkout as it did before it was chained
    logger(&c, "git-lfs", false, 3);
    let o = c.git_out(&["checkout", "-q", "--detach", &from]);
    assert!(!o.ok, "the status of git-lfs's hook is the hook's: {}", o.all());
}

// Mutation: as above, for the shim a workspace writes in every clone to regenerate after a pull.
#[test]
fn a_workspace_s_regen_shim_is_chained_and_still_starts_the_regeneration() {
    let w = fixture::migrated_remote();
    let c = w.clone_of("a");
    let regen = logger(&c, "regen-on-pull", false, 0);
    let shim = format!(
        "#!/bin/sh\n# regen-on-pull: written by regen-hooks\nb=\"{}\"\n[ -x \"$b\" ] || exit 0\nexec \"$b\" post-checkout \"$@\"\n",
        c.w.base().join("shims/regen-on-pull").display()
    );
    exec::write_executable(&hooks(&c).join("post-checkout"), &shim);
    c.ok(&["data-refs", "instantiate"]);
    assert_eq!(read(&hooks(&c).join("post-checkout.chained")), shim);
    assert!(move_head(&c).ok);
    assert!(read(&regen).starts_with("args: post-checkout "), "the shim ran: {}", read(&regen));
}

// Mutation: chain the engine's own hook to itself, or move the chained one again, on a second run.
// The person's hook is then lost, or runs twice, or the hook is written on every command.
#[test]
fn an_already_chained_hook_stays_put() {
    let w = fixture::migrated_remote();
    let c = w.clone_of("a");
    logger(&c, "git-lfs", false, 0);
    exec::write_executable(&hooks(&c).join("post-checkout"), LFS_POST_CHECKOUT);
    c.ok(&["data-refs", "instantiate"]);
    let names = ["post-checkout", "post-checkout.chained", "reference-transaction"];
    let first: Vec<_> = names.iter().map(|n| (read(&hooks(&c).join(n)), std::fs::metadata(hooks(&c).join(n)).unwrap().modified().unwrap())).collect();
    let _ = c.run(&["data-refs", "instantiate"]);
    c.ok(&["data-refs", "verify"]);
    let again: Vec<_> = names.iter().map(|n| (read(&hooks(&c).join(n)), std::fs::metadata(hooks(&c).join(n)).unwrap().modified().unwrap())).collect();
    assert_eq!(again, first, "nothing was written again");
    assert!(!hooks(&c).join("post-checkout.chained.chained").exists());
}

// Mutation: refuse wherever the chained place is taken. `git lfs update --force` writes git-lfs's
// hook over the engine's, the same text as the one chained, and every `instantiate` then refuses
// until the person deletes a file by hand.
#[test]
fn the_chained_hook_written_back_over_the_engine_s_is_taken_as_chained_and_the_engine_s_written_again() {
    let w = fixture::migrated_remote();
    let c = w.clone_of("a");
    logger(&c, "git-lfs", false, 0);
    exec::write_executable(&hooks(&c).join("post-checkout"), LFS_POST_CHECKOUT);
    c.ok(&["data-refs", "instantiate"]);
    exec::write_executable(&hooks(&c).join("post-checkout"), LFS_POST_CHECKOUT);
    let v = c.fails(&["data-refs", "verify"]);
    assert!(v.all().contains("writes the engine's again"), "{}", v.all());
    let o = c.run(&["data-refs", "instantiate"]);
    assert!(!o.all().contains("a hook of your own"), "not refused: {}", o.all());
    assert!(read(&hooks(&c).join("post-checkout")).contains("# chains: post-checkout.chained"));
    assert_eq!(read(&hooks(&c).join("post-checkout.chained")), LFS_POST_CHECKOUT);
    c.ok(&["data-refs", "verify"]);
}

// Mutation: give the chained `reference-transaction` hook no input, or let it take the input the
// copy reads. Its own work is then done on nothing, or the copy never sees the move of `ORIG_HEAD`.
#[test]
fn a_chained_reference_transaction_hook_reads_the_same_input() {
    let w = fixture::migrated_remote();
    let c = w.clone_of("a");
    let log = logger(&c, "their-ref-hook", true, 0);
    exec::write_executable(&hooks(&c).join("reference-transaction"), format!("#!/bin/sh\nexec '{}' \"$@\"\n", c.w.base().join("shims/their-ref-hook").display()));
    c.ok(&["data-refs", "instantiate"]);
    c.git(&["branch", "chained-sees-this"]);
    assert!(read(&log).contains("refs/heads/chained-sees-this"), "the chained hook read the input: {}", read(&log));
    c.ok(&["data-refs", "verify"]);
}
