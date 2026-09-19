# What taking the anchor off the trailer net costs, on a real corpus

`attribution_is_attribution_trailer` in `lib/attribution.sh` strips
`[[:space:]#]*` off the front of a line before it tests the trailer pattern,
because the anchor was the hole: a byline behind one leading space or one `#`
failed the match, and git stores both verbatim. The widening has a price, and
the price is that `author` and `committer` sit in the key pattern as bare
words, so with the anchor gone an indented `author: Jane` reads as a trailer
where before it read as nothing.

These two scripts answer what that costs, and the comment in
`lib/attribution.sh` points at them instead of carrying a number. A count is
true when it is measured and drifts on the next merge, so what belongs in a
header is the instrument rather than its answer.

| script | corpus |
|---|---|
| `01_commit_messages.sh` | every commit message reachable from every ref, `git log --all` |
| `02_tracked_files.sh` | the tracked content of one ref, `git grep` over the tree |

Each takes a repository path, defaulting to the one it sits in, and each states
the case that must fail before it runs: a planted `# Co-Authored-By: Jane Roe`
that the widening must newly match, and a planted `fix: a thing` that neither
form may match. A run whose control does not hold stops rather than reporting.

## What the run committed here found

The `.out` files beside each script are the runs, over this repository and over
the clause-dev workspace repository.

Over commit messages the widening newly matches nothing, in either repository:
531 commits and 18478 lines here, 3493 commits and 33130 lines there, and 187
lines matched by both forms against 0 matched only by the wider one. That is
the reassuring half and it is also the half that was already believed.

Over tracked file content the answer is different, and that is the part worth
having written down. Three lines here are matched only by the wider form, and
two of them are the shape the header describes rather than an abstraction of
it:

    mock/crates/mockspace-core/src/io/ref_write.rs:147
        author:        signature.clone(),
    mock/crates/mockspace-core/src/io/ref_write.rs:148
        committer:     signature,

A struct literal, indented, with a field named for the thing the key pattern
is looking for. The third is `tests/attribution_test.sh:205`, which spells a
byline on purpose to find one, so the net is right about it. Over clause-dev's
tree the widening newly matches nothing.

So the cost is real, it is not zero, and nothing downstream of these three
lines refuses anything: the trailer path is called on commit messages and on
markdown, and the rust source it matched is neither. What the run says is that
a caller pointing this net at arbitrary source would get a false refusal today,
in this repository, at a line somebody can go and read.

## The claim this replaces, and why it was wrong

The header said the widening was measured over both repositories' whole
histories and matched nothing new. Two things were wrong with it. It named no
corpus, so "nothing new" could mean commit messages or file content and the
answer differs between them. And the mockspace clone it was measured in was
shallow, thirteen commits deep against the 531 the repository actually has, so
"whole histories" described a measurement that had not been taken. Running
`git rev-parse --is-shallow-repository` costs nothing and would have caught it,
which is why `01_commit_messages.sh` refuses a shallow clone rather than
reporting a small number that looks like an answer.
