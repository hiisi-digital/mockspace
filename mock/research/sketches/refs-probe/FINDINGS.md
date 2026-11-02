# Refs probe: findings

Probe `probe.sh`, output `probe.out`, run on 2 November 2026 with git 2.43.0. It holds for: a local scratch repository and a local bare repository on one filesystem, reached as `file://` URLs, plain git commands only, no forge, no proxy, no hook on the bare side.

## What was asked

Whether data can live on `refs/mock/registry/<namespace>` or, as a fallback, on `refs/heads/mock/<namespace>`, moved with explicit refspecs, and what the lease push does when two writers race.

## What held

Fifteen checks, all as expected (`probe.out`).

1. A ref outside `refs/heads` pushes to a bare repository by explicit refspec, the same as a branch does.
2. `git ls-remote origin 'refs/mock/*'` selects the namespace; the branch form is selected by `refs/heads/mock/*`.
3. A default clone does not carry `refs/mock/*`. The refspec is what brings it. This is the case that makes a missing refspec dangerous, and the store has to configure or pass it.
4. A shallow fetch with the refspec (`--depth 1 +refs/mock/registry/probe:refs/mock/registry/probe`) gives the tip and one commit of history, both into an empty repository and into a shallow clone.
5. The branch form fetches into a private local ref (`+refs/heads/mock/probe:refs/mock-local/branch-form`). The local ref and the remote ref are a pair, so switching a namespace from the ref form to the branch form changes the remote half only.
6. `--force-with-lease=<ref>:<expected>` moves the ref when the remote still holds the expectation.

## Cases that must fail, and did

4. Fetching a ref the remote does not hold exits 128 with `fatal: couldn't find remote ref refs/mock/registry/absent`. This text is how an absent namespace is told from an unreachable one.
8. A second writer holding the old tip and pushing with a lease on it is refused: `! [rejected] ... (stale info)`, exit 1. The remote keeps the winner's commit.
9. A plain push of a diverged commit is refused (`non-fast-forward` or `fetch first`), exit 1.
10. A fetch from a path that is not a repository exits 128 (`does not appear to be a git repository`) and leaves no local ref behind. Nothing here reads as an empty store; the exit code is the signal and stderr says why.

## The recovery

11. After the loss, the loser fetches the new tip, builds a commit with both tips as parents (or rebuilds its change on the new tip) and pushes with a lease on the new tip. Both racers' commits are ancestors of the remote tip. Writes on a data ref carry history, so nothing a racer wrote is dropped.

## What this does not show

The cloud container's proxy and GitHub are not in this probe, and the probe cannot tell whether either accepts a push or a fetch of a ref outside `refs/heads`. The coordinator's state file records that the proxy refuses tag pushes and ref deletion, and a tag is a ref outside `refs/heads`, so a refusal of `refs/mock/*` is plausible and untested. The branch form is the fallback, which `msg` already exercises daily with `refs/heads/coord/*`. The store therefore takes a (local ref, remote ref) pair per namespace and the form is a setting, never a code change.

Also untested: a bare repository over ssh, an `insteadOf` rewrite, and any server-side hook.

## Rerun

`mock/research/sketches/refs-probe/probe.sh` (a nutshell script, run it directly). It exits 0 when every case held. The must-fail cases assert that git refuses; a git that accepts a stale lease makes the probe fail.
