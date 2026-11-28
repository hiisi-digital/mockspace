# A cloud container's git proxy and custom refs

**Outcome:** fails

Dated 23 November 2026, by refs-lane, for roadmap step 9 (data refs and the shadow tree on orphan custom refs only, a cloud push path for them probed and built). Measured from a Claude Code cloud container whose outbound git goes through the session's proxy, against `hiisi-digital/mockspace` and `orgrinrt/muisti`, both attached to the session with push access.

## What was asked

Whether a cloud container can keep the mock trees on `refs/mock/*` of the member's own remote, as op's F1 answer has them, or whether the branch form `refs/heads/mock/<ns>` that mockspace `dev` defaulted to is forced by the host.

## What was measured

`probe.sh` runs each step once. By hand, on 23 November 2026:

| Step | Result |
|---|---|
| `git push origin <commit>:refs/mock/probe-refs-lane` | refused: `RPC failed; HTTP 403`, `send-pack: unexpected disconnect`, nothing written |
| `git push origin <commit>:refs/heads/mock/probe-refs-lane` | accepted, the branch made |
| `git push origin --delete refs/heads/mock/probe-refs-lane` | refused: the same HTTP 403, the branch left on the remote |
| `git fetch origin refs/pull/1/head:<local>` (muisti) | accepted: a ref outside `refs/heads` is fetched by its exact name |

The 403 is the proxy's and not GitHub's: GitHub takes a push to a custom ref from a clone with a token, and the refusal is an HTTP status on the smart-HTTP request, before any `remote:` line. The 2 November record in kaski's ticket `the_cloud_git_proxy_refuses_pushes_to_refs_outside_refs` saw the same on `refs/mock/probe` and `refs/notes/mock-probe`. So the proxy passes reads of any ref and writes of branches only: it creates and moves `refs/heads/*`, and deletes nothing.

## What follows

- The branch form was never the host's rule, only this proxy's. The remote form is a setting of the route, not of where the trees live, so mockspace keeps every tree on `refs/mock/*` and nothing on `refs/heads/*`, and the fallback is gone.
- Reading needs nothing: a cloud clone fetches `refs/mock/<ns>` and `refs/mock/+/*` by name through the proxy.
- Writing needs a route around the proxy. The store pushes to the URL the clone's git config names under `mockspace.refsPushUrl` where it is set, and reads back from the declared remote, so the check after the push still sees a relay that took a push and lost it (`mockspace-store`, `RefStore::push_target`, and its tests in `tests/custom_refs.rs`).
- The relay itself is a smart-HTTP endpoint on the console's Worker, the host the forge routes already use: it takes `git-receive-pack` for a repository the caller may write, refuses any ref update outside `refs/mock/`, and forwards the pack to GitHub with the Worker's token. A git push then goes there unchanged, and the cloud bootstrap sets `mockspace.refsPushUrl` for each clone. It lives in applets, which this job does not hold; until it is deployed, a cloud container reads every tree and writes none, and a write is made from a machine with direct access.
- The probe branch `refs/heads/mock/probe-refs-lane` on `hiisi-digital/mockspace` could not be deleted from the container and is left for `forge-route delete-branch`.

## Not measured

- A push through the relay, which does not exist yet.
- Whether GitHub's own API (`POST /repos/{owner}/{repo}/git/refs`) would take `refs/mock/...` for a commit the remote already holds; it would move muisti's existing tip without a pack, but cannot carry a new commit.
