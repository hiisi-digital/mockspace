# Mock trees: the shadow tree, proposals and promotions, on the build plan

Status: a plan, not a design, written 26 November 2026. It extends `../202611120700_data-ref-architecture/018_build_plan.md` with what the maintainer decided between 22 and 25 November about the shadow tree, proposals, promotions and the boundary between the internal side and the public tree. 018's rounds keep their numbers and their content; this note adds to them, adds three rounds after round 5 (5a, 5b, 5c) and three pieces of outside work (O5 to O7), and says where earlier text no longer holds. It opens no round and changes no code. The maintainer's own words for each decision below are kept outside this repository, keyed by the decision numbers of section 2.

## 1. What governs

The order of 018 stands: the maintainer's words in `000_op.md` govern, 017 under them as amended there, 018 under both. The decisions of section 2 are later than all three and govern them where they differ; section 5 names each place. 017 is still unratified, so each round's topic file reads its part of 017 and of this note against the maintainer's words once more before anything is designed from it. No canon is declared in this repository, so nothing here is derived from canon; a call that rests on no decision is marked derived.

## 2. The decisions this note rests on

- D1, 23 November. Crate sources never enter the shadow side. The shadow side holds only the current proposal, what is to be patched over the live public tree.
- D2, 23 November. The shadow tree also holds any number of full `.tmpl` files, kept there for good and generated into place on the public side.
- D3, 23 November. The public tree is a plain git repository with no extra procedure. Outside contributors use ordinary pull requests, or merge into `dev` locally where they have the rights.
- D4, 23 November. Provisional stamps are made under a standing weekly blessing for the coordinating role, not a blessing per run and not by anyone but the requester.
- D5, 23 November. A full release is refused while any provisional stamp in it is unconfirmed by a human.
- D6, 23 November. A promotion request is what an agent files for verified changes. A CLI writes one markdown file holding the whole promotion, its diffs and the squash commit in full, so no single review surface is required. The public commit's author is whoever approved it. Promotions stay small, touch only `dev` and `main`, and replace feature branches on the public tree: feature branches are made off the shadow tree and stay there, and nothing in the agent pipeline is a pull request.
- D7, 23 November. A stamp is a git-native record of approval, and a hook denies without one. Trusted agents may stamp provisionally; the internal record shows the agent, and the public commit carries the human in the loop.
- D8, 23 November. A provisional stamp is the one path where an agent uses a human's identity without a human review in between, by running the same review the human would and approving under the overseeing human's name.
- D9, 23 November. Promotions squash onto `dev`; `dev` to `main` stays a merge commit.
- D10, 23 November. No forge feature is relied on anywhere. `dev` to `main` is described for any host and falls back to the internal flow on the promotion machinery.
- D11, 23 November. A push straight to the public trunk is held off by detection in every clone plus a push gateway, and the git hooks reinstall themselves at session start and are checked before use.
- D12, 23 November. No signing key is required; the human identity is enough, with a public GPG or SSH key optional where one exists.
- D13, 23 November. The public tree carries no `.tmpl` file and no generation notice. Promotion always resolves a template to static text, and some templates are declared instance-only and never reach the public tree.
- D14, 23 November. One cut per repository emits the public history, the mock trees and the shadow tree together. The shadow side keeps the full filtered history of what it holds; crate sources and their history are dropped from it.
- D15, 23 November. The forge rulesets go only after the public boundary guards, the gateway and `promote verify` stand.
- D16, 24 November. No branch reaching the public side carries a marker, a stamp or any name of a process. Such records live only in the mock trees.
- D17, 24 November. A byline requirement or refusal is configuration of a repository, never a built-in default.
- D18, 25 November. mockspace has no concept of identity. It has a members table per repository, with roles and a type (`human`, `agent::assisted`, `agent::autonomous`), and a commit on a mock tree from an identity not in the table is refused, except the commit that adds that identity. The type comes from the same configuration as bylines.
- D19, 22 and 23 November. Mock refs are orphan refs outside `refs/heads/`, fetchable by exact name and not discoverable without the engine; after the cut the git tree holds nothing of the engine's side.

## 3. What of 018 stands unchanged

- What governs, the bar of `000_op.md:7` and `:9`, and the intent at `:69`: unchanged, and every round below is held to them.
- What the shipped code is: unchanged; nothing of the shipped engine is carried into the new rounds either.
- How the rounds are cut: unchanged, one feature a person can see working at its end, red tests first, one fixture world grown by each round.
- Round 2, the instance: unchanged.
- Round 5's template mechanism: `regenerate`, the produced set, carry back, invariant N1, the classes of 4.3.1 and the D12 driver stand as 018 and 017 4.3 have them; 5.1 and 5.2 below widen it.
- Known defects on feat/v2: unchanged; each is still read again at its round's opening.
- O1 to O4: unchanged.
- Probes P1 to P15: unchanged; section 7 adds three.
- What 018 does not settle: unchanged; section 8 adds the questions this note leaves.

## 4. What it adds, under the step each extends

### Round 1: names

- 1.1 Shadow branches (D6, D19). The shadow tree's trunk stays `refs/mock/+` (`000_op.md:172`). Since git refuses a ref with children, its feature branches live in a sibling namespace outside `refs/mock/`, `refs/mock-branch/+/<name>` as the candidate, with a fully qualified refspec, a working ref per worktree as 017 gives every mock tree, and a mirror fetched unforced. Only the shadow tree branches; data trees keep one tip. Derived: the name of the sibling namespace.
- 1.2 The promotion records (D7, D16). Requests, stamps, blessings and confirmations are rows of a mock tree of kind `registry` under the reserved namespace `promotions`, one file per record, so the store's merge hook and the row readers serve them and no fifth kind is added. Derived.
- Red tests added: neither a forge listing (`ls-remote` filtered to `refs/heads/` and `refs/tags/`) nor a plain clone shows a shadow branch or a record; `promotions` is refused as a data namespace; a branch refspec written to config never starts with a bare `+`.

### Round 3: hooks

- 3.1 The members table (D18). Its schema is a registry namespace with a row per member: identity, roles, type. `pre-commit` and `pre-push` refuse a commit on any mock tree whose author or committer has no row, unless that commit adds the row. The type is read from the byline configuration (D17).
- 3.2 Hooks in place (D11). `instantiate` and every engine command check that the git-shook entries are generated and current, and reinstall them where they are not; a commit or push through the engine refuses while they are missing.
- 3.3 The public refusals (D6, D11). `pre-push` refuses, for an identity of an agent type, any new `refs/heads/*` and any move of `dev` or `main` that is not the landing of a stamped promotion.
- Red tests added: an unlisted identity's commit on a mock tree is refused, and the commit adding it passes; a deleted hook entry is back after the next engine command; an agent-typed push of a new public branch, and of `dev` with no stamp, are both refused.

### Round 4: the remote

- 4.1 The guard and repair cover the shadow branches and the `promotions` tree as they cover any mock tree: unforced, repaired, and a rewind refused (D11). No purge of `promotions` is offered. Derived.

### Round 5: the shadow tree

- 5.1 The contents rule widened (D1, D2). 017 4.3 allows exactly four kinds of file in `mock://+/`; the proposal of round 5a is the fifth. Anything else, a crate source outside a proposal above all, is refused by `verify` and the lint.
- 5.2 Template visibility (D13). Every template declares itself public or instance-only. Regeneration writes an instance-only output only in an instance's ignored files, promotion never writes one into the public tree, and no public output carries a generation notice. Ticket `templates_declare_public_or_instance_only_and_public_files_c`.
- Red tests added: a crate source placed in the shadow tree outside a proposal is refused by `verify` and the lint; an instance-only output never appears in a promotion's tree; no public output matches the generation-notice patterns.

### Round 5a, new: the proposal

- Feature (D1, D6). A change to the git tree's sources is held in the shadow tree as the current proposal against a recorded public base, in the form question Q1 settles. A feature branch of the shadow tree carries its proposal and its template edits together, and lands into `+` by an ordinary git merge. When public `dev` moves (a human's commit, a contributor's pull request), `update` carries the proposal onto the new tip through git's three-way merge (`000_op.md:51`); a conflict goes to `status` and nothing is overwritten, the same protection as invariant N1. In an instance the git tree shows the public tip with the proposal applied, so the build and the gate run on exactly what a promotion would write.
- Crates: `mockspace-store`, `mockspace-cli`, `mockspace-builtin-lints`, `mockspace-test-fixtures`.
- Red tests first: a proposal round-trips byte for byte through commit, push, fetch and a second clone; the public tip moves under a pending proposal and the proposal is carried onto it, and a conflicting move leaves both texts recoverable; no shadow commit's tree holds a crate source outside a proposal, asserted over the whole shadow history of the fixture; a plain clone fetches no proposal; an instance's build reads the patched tree.
- Ticket: none in this repository's lane (missing).

### Round 5b, new: promotion and verify

- Feature (D6, D7, D9, D12, D16). `promote request` takes the proposal (whole or in a slice, by Q3) and the public outputs of the templates, runs the gate, renders twice and compares, builds and tests the resulting public tree standalone, runs the boundary lints over it and its message, refuses while a git dependency of that tree is not on the dependency's public branch, and checks the configured size limits. It records the whole public commit to be: parent, tree, author (the approver), committer, dates and message, so the commit's id is known before it exists. `promote report` writes the one markdown file, a pure function of the request and the two trees. `promote approve` writes a stamp naming the request, the commit id and the report's digest, under the approver's identity, into `promotions` and nowhere else. `promote land` writes that exact commit and pushes it unforced; a stamp is void once public `dev` moves. After landing, the proposal is empty of what was promoted.
- `promote verify`, the one check of the public line since the cut (D3, D16, D18): no commit is authored or committed by an identity of an agent type, no commit carries a byline the configuration refuses, a marker, a record id or an internal path, every stamp names a commit that exists with the stamped tree, and unconfirmed provisional stamps are listed. A human's own commit or merge needs nothing more. Hooks run the shallow form, which reads commit headers and records; `verify --deep` recomputes each stamped tree.
- Crates: `mockspace-cli` (the verbs), a crate for the records and the check as pure logic with no git call and no clock of its own, so a gateway can run the same check (derived), `mockspace-template`, `mockspace-builtin-lints`, `mockspace-test-fixtures`.
- Red tests first: the report written twice is byte-identical; landing writes the commit id the stamp names; a stamp goes void when `dev` moves; no public commit's headers or message hold a record id, a stamp or a member's agent identity; a planted agent-authored commit, a refused byline and a marker are each found by `verify`; a human's plain merge passes it; an over-limit request is refused; a request over a dependency revision not on its public branch is refused.
- Tickets: `mock_promote_verify_is_the_one_check_of_the_public_line_stam`, `a_git_native_approval_stamp_checked_by_a_hook_gates_every_pr`, `one_projection_from_the_shadow_line_to_the_public_tree_share`, `a_promotion_is_refused_while_a_git_dependency_of_its_tree_is`. The request and report verbs have none in this repository's lane (missing).

### Round 5c, new: provisional stamps and releases

- Feature (D4, D5, D7, D8, D9). `bless`, run only by a member of type `human`, writes a standing blessing that lasts one week and names the role whose members may stamp under it. A provisional stamp names its blessing; the public commit's author is the blessing's human, and the stamp's own record carries the stamping member. `verify` lists every provisional stamp until a human confirms it. A release is a promotion to `main` of kind `release`: a merge commit whose second parent is on public `dev`, its tag named in the stamp. A full release is refused while any provisional stamp since the last full release is unconfirmed.
- Crates: as round 5b.
- Red tests first: a provisional stamp after the blessing's week, by a member outside its role, or with no blessing, is refused; the public commit of a provisional promotion is authored by the blessing's human; a full release is refused over an unconfirmed provisional stamp and passes once it is confirmed; a squash to `main` is refused.
- Ticket: none in this repository's lane (missing).

### Round 6: the migrations

- 6.1 The cut (D14, D19). One migration per repository writes the public history, the mock trees and the shadow tree together. The shadow tree takes the filtered history of the templates and nothing of crate sources; the public tree ends with no `.tmpl` file, no generation notice and no engine configuration, and a generated file's public history continues under its output name. The filtering of the history itself is done before the migration by a rehouse, outside this plan.
- Red tests added: after the migration no public commit holds a `.tmpl` path or an engine file; the history of a generated README is reachable under its output name; no shadow commit holds a crate source.

### Round 7: acceptance

- 7.1 Scenarios added: a proposal made on a shadow branch, landed into `+`, promoted, reported, approved by a human and landed, while a contributor's pull request moves public `dev` underneath it; a week of provisional stamps under one blessing, then a full release refused and then allowed; a direct agent push to `dev` refused by the hook, by the gateway, and found by `verify` on fetch where both were bypassed.

### Outside work

- O5, the push gateway (D11). Agent sessions push only through a gateway that runs round 5b's check over each push: mock trees and shadow branches only when they move forward, `promotions` only with requests and provisional stamps, `dev` and `main` only as a stamped promotion, no other public branch, no deletion. It lands in the repository that hosts the gateway, after 5b. Checked by that repository's tests over pushes of each kind.
- O6, the release tool over promotions (D9, D5). The release tool drives a release through 5b and 5c and opens nothing on a forge but a mirror. Lands in that tool's repository after 5c.
- O7, the forge reliances swept and the rulesets removed (D10, D15). Only after round 3's refusals, O5 and 5b stand, and after the repositories are on the migrated form. Checked by a listing of each host's rulesets and protections coming back empty, and by every landing passing with them gone.
- The built-in rules gain the shadow tree, proposals and promotions in neutral terms, each landing with its own round, since a built-in may not describe a mechanism the source lacks. Identity and bylines stay out of the built-ins (D17, D18). Ticket `mockspace_builds_in_the_identity_boundary_and_promotion_rule`.

## 5. What it replaces

- 017 4.3 (`017_consolidation.md:237` to `:241`), "exactly one of" four kinds: now five, with the proposal (D1, D2), by 5.1.
- The note of ticket `design_the_shadow_tree_promotions_and_the_credential_boundar`, that the shadow tree holds source: replaced by D1. Its layout `refs/mock/+/dev` and `refs/mock/+/work/*` would rename the trunk `000_op.md:172` fixes; 1.1 keeps the trunk and moves the branches beside it.
- Ticket `human_commits_on_the_public_tree_are_carried_back_into_the_s`: with no crate source on the shadow side, a human's commit needs no carrying back (D3). What remains is round 5's carry back of edited generated output and round 5a's carrying of a pending proposal onto the new tip.
- A design in which the public tree is a function of a shadow commit holding every source, with the sources entering the shadow line in one cut commit, an adoption verb and stamp for human and contributor commits, a blessing per run, and a public line where every first-parent commit must be stamped: replaced by D1, D3, D4 and 5b's `verify`. Its approver denylist drawn from identity data outside the repository is replaced by the members table (D18).
- A reading that the shadow side keeps no history: D14 keeps the full filtered history of what it holds, and only the crate sources are dropped.

## 6. The steps in build order

1. O1 and O2, as 018. Unchanged.
2. Round 1 with 1.1 and 1.2. Needs O2 and P15. Mockspace. Checked by round 1's red tests and 1.1's listing and plain-clone tests. Tickets: `the_shadow_branches_refspecs_written_into_a_clone_and_their`, `data_refs_and_the_shadow_tree_are_orphan_custom_refs_never_u`.
3. Round 2, as 018. Unchanged.
4. Round 3 with 3.1 to 3.3. Needs 2 and O1. Mockspace. Checked by 3's added red tests. Tickets: `a_contributors_table_in_the_registry_and_every_attribution_a`, `the_git_hooks_reinstall_themselves_at_session_start_and_mock`.
5. Round 4 with 4.1. Needs 2 and 3. Mockspace. Checked by round 4's remote rows run over a shadow branch and `promotions`.
6. Round 5 with 5.1 and 5.2. Needs 2, 3 and P4. Mockspace. Checked by the contents lint and the visibility rows. Tickets: `the_mockspace_generated_merge_driver_keeps_ours_and_drops_a`, `templates_declare_public_or_instance_only_and_public_files_c`.
7. Round 5a. Needs 5 and the answers to Q1 and Q3 before its DOC locks. Mockspace. Checked by the whole-history assertion that no crate source sits outside a proposal. Ticket missing.
8. Round 5b. Needs 4 (nothing is published before the guard) and 5a, and P16. Mockspace. Checked by `verify`'s planted-commit rows and the void-stamp row. Tickets as listed under 5b; the request and report tickets missing.
9. Round 5c. Needs 5b and Q2. Mockspace. Checked by the blessing and release rows. Ticket missing.
10. O5, the gateway. Needs 5b and P17, P18. The gateway's repository. Checked there by pushes of each kind; its ticket is held there.
11. O6, the release tool. Needs 5c. The release tool's repository; its ticket is held there.
12. Round 6 with 6.1. Needs 1 to 5c, since the one cut writes the shadow tree and the records together. Mockspace. Checked by 6.1's rows and round 6's byte-for-byte rows.
13. Round 7 with 7.1. Needs all. Mockspace. Its scenarios are the check.
14. O7. Needs 4, 8, 10 and the migrated repositories. Outside mockspace; its tickets are held there.
15. O4, the consumers' port, as 018, after 13 is green on the merge.

Rounds 5a, 5b and 5c share their crates and run one after the other. O5 can run beside 5c once 5b has landed.

## 7. Probes added

- P16, the fetch-time check: whether `reference-transaction` sees a fetch's update of a remote-tracking ref of `dev` or `main`, and what it costs per git command against the bound of `000_op.md:105`. Before round 5b.
- P17, the check of 5b run inside the gateway's runtime on a real push, with the size of `promotions` it must read. Before O5.
- P18, whether a hosted session's own push path to a repository can be narrowed so the gateway is its only write route. Before O5; if it cannot, round 3's `pre-push` and `verify` on fetch are what stands, and O5 says so.

## 8. Questions only the maintainer can answer

- Q1. The form of a proposal.
  - a: the changed files themselves, at their git-tree paths in `mock://+/`, beside a record of the public base and of deletions. Keeps 017's identity mapping and holds nothing but changed files; round 5a writes its own three-way carry onto a moved tip from the recorded base, and the contents lint knows a fifth kind of file.
  - b: a commit range on top of a public commit, held under the shadow branch refs. Closest to git's own branches, so merges, rebases and blame need nothing new, and round 5a mostly configures git; but the range's commits reach the whole public tree by reference, which reads against D1 unless D1 is taken to forbid only sources of the shadow side's own.
  - c: a patch series (`git format-patch` output) stored as files in `mock://+/`. Plain text a human reads, but each carry onto a moved tip is `git am -3`, a conflict leaves a patch to edit rather than a file, and the build in an instance applies the series first.
- Q2. Who commits a promotion's public commit, given that its author is the approver (D6).
  - a: the approver as committer too. The public commit names one person, and landing by anyone writes the same object.
  - b: an identity of the tool or review surface that landed it, with the approver as author. The public line then shows a non-human committer, which `verify` must then allow by a declared exception.
  - c: whoever runs `land`. The id is no longer known when the report is written, so the stamp names the tree and parent instead of the commit, and 5b's void-stamp rule changes.
- Q3. How many proposals are current at once.
  - a: one per repository, the one on `+`; feature branches merge into it and promotions take contiguous slices of it. 5b's request needs one ordering rule, and a slow promotion holds back the rest.
  - b: one per shadow branch, each promoted on its own in any order. Promotions do not wait on each other, but every promotion carries the other pending proposals onto the new tip and can conflict there.
- Q4. May a member stamp provisionally a request it wrote itself, under the weekly blessing.
  - a: no; the stamper differs from the requester, plus tags and launched helpers counted as the requester. 5c gains one refusal row, and a lone stamper cannot unblock its own work.
  - b: yes; the blessing alone decides. 5c drops the row, and the human's later confirmation is the only second reading.
