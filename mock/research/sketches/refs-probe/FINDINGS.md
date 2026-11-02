# Refs probe: findings

Probe `probe.sh`, output `probe.out`, run on 2 November 2026 with git 2.43.0. It holds for: a local scratch repository and a local bare repository on one filesystem, reached as `file://` URLs, plain git commands only, no forge, no proxy, no hook on the bare side.

## What was asked

Whether data can live on `refs/mock/registry/<namespace>` or, as a fallback, on `refs/heads/mock/<namespace>`, moved with explicit refspecs, and what the lease push does when two writers race.

## What held

Cases 1 to 3, 5 to 7 and 11 of `probe.out`.

1. A ref outside `refs/heads` pushes to a bare repository by explicit refspec, the same as a branch does (case 1).
2. `git ls-remote origin 'refs/mock/*'` selects the namespace; the branch form is selected by `refs/heads/mock/*` (case 2).
3. A default clone does not carry `refs/mock/*`. The refspec is what brings it, so the store has to pass it (case 3a, 3b).
4. A shallow fetch with the refspec (`--depth 1 +refs/mock/registry/probe:refs/mock/registry/probe`) gives the tip and one commit of history, both into an empty repository and into a shallow clone (case 5a, 5b).
5. The branch form fetches into a private local ref (`+refs/heads/mock/probe:refs/mock-local/branch-form`). The local ref and the remote ref are a pair, so moving a namespace from the ref form to the branch form changes the remote half only (case 6).
6. `--force-with-lease=<ref>:<expected>` moves the ref when the remote still holds the expectation (case 7).
7. After a lost race the loser fetches the new tip and pushes a commit that has both tips as parents, on a lease of the new tip. The remote tip is that merge commit (case 11, 11b).

## Cases that must fail, and did

* Case 4: fetching a ref the remote does not hold exits 128 with `couldn't find remote ref`. Exit 128 is also what an unreachable remote gives (case 10), so the exit code alone does not tell an absent namespace from a broken remote. The store asks `ls-remote` which of the two it is, and does not parse the fetch's words.
* Case 8: a second writer holding the old tip and pushing with a lease on it is refused, `! [rejected] ... (stale info)`, exit 1. The remote keeps the winner's commit (8b).
* Case 9: a plain push of a diverged commit is refused (`non-fast-forward` or `fetch first`), exit 1.
* Case 10: a fetch from a path that is not a repository exits 128 (`does not appear to be a git repository`) and leaves no local ref behind.

## What this does not show, and what was measured elsewhere

The cloud container's proxy and GitHub are not in this probe, and the probe cannot tell whether either accepts a push or a fetch of a ref outside `refs/heads`.

Measured by console-coord on 2 November 2026 at 11:30 UTC, not by this probe, and relayed to the job on the channel (message 27): from a hookless bare repository in a cloud container, pushing through the proxy to `orgrinrt/kaski`, `refs/mock/probe` and `refs/notes/mock-probe` were both refused ("the remote end hung up unexpectedly", twice each), `refs/heads/mock/probe` was accepted, and deleting it by push hung up as well. So in cloud containers a data ref takes the branch form. `mockspace-store` makes the branch form its default remote form (`RemoteForm::Branch`) and keeps `refs/mock/<namespace>` for a host that takes it, a self-hosted one. This probe has not repeated that measurement.

Also untested: a bare repository over ssh, an `insteadOf` rewrite, and any server-side hook. A host that hides `refs/mock/*` from `ls-remote` would read as an empty namespace to the store's absent-ref check; that case needs a real host to settle.

## Rerun

`mock/research/sketches/refs-probe/probe.sh` (a nutshell script, run it directly). It exits 0 when every case held. The must-fail cases assert that git refuses; a git that accepts a stale lease makes the probe fail.
