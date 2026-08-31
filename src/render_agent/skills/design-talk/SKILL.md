# Design talk flow

Run this when open design questions have to be settled before implementation can
proceed. The agent leads and the maintainer decides. The output is topic files
that stay in the repository as a permanent record, plus an agreed path the later
implementation flow works from.

The talk is the synchronous decision conversation that happens *before* the
work, with a person in the loop. Reviewing something already built is a
different job and is not this.

## Steps

0. Consume what was filed for this round, before anything else. Run
   `scripts/consume-strays` from inside the repository. It scans every ref for
   flat topic files stranded on branches nobody is working on, and for archived
   round directories this branch cannot see, and pulls both in. A topic left
   open in TOPIC phase is a filing: somebody found something while working
   elsewhere and left it in the way so the next round would have to address it,
   and this is that round. Three ways they vanish, all observed: a topic on a
   branch that never merged, a topic swept into another round's archive by
   `close`, and an archive present only on an unmerged branch. Never start a
   round by branching fresh from the trunk and leaving them behind, never stash
   one, never move one to another branch, and never close a round leaving one
   unmentioned. Every topic the script pulls in is named in this round's
   changelists, with what it changes or why it needs nothing. If one is
   genuinely out of scope it is re-filed as a fresh flat topic after this round
   closes, so the next round inherits it the same way.

1. Ground before asking. Read the relevant notes, rules, tasks and the actual
   source. Resolve factual cruxes yourself, by research on tracking issues or
   specifications, by targeted greps, or by one neutrally briefed expert
   dispatch per hard question, BEFORE putting anything to a person. Their time
   is for decisions, not for facts the agent can gather. A question resting on a
   wrong premise, a feature not actually enabled, a file that moved, an
   inventory line that was a misread, wastes the talk and costs trust in
   everything else on the agenda.

2. Set the agenda. Open with a compact orientation: the topics, the order they
   come in, with the biggest blast radius placed where it fits best, and what
   grounding is still running in the background. Keep it short. The questions
   carry the substance.

3. One unit per question. Drive with AskUserQuestion, one substantive decision
   per call. Closely coupled small pairs may share a call, but prefer one. Each
   option names its concrete implication and its cost. The agent's own
   recommendation is welcome here, unlike in an expert dispatch where it is
   forbidden: mark the lean and still give real, balanced alternatives. Say
   plainly what the agent is unsure about, so it can be weighed rather than
   discovered later.

4. Capture as you go, and let a topic file accrete. As each topic firms, write a
   flat TOPIC-phase file `<mock>/design_rounds/YYYYMMDDHHMM_topic.<slug>.md` and
   commit it. **A topic file is a transcript, not a decision record.** When the
   talk returns to a subject the file already covers, append a section to that
   file rather than starting a new one. Start a new file when the *subject*
   changes, not when a question is answered.

   Size is the check that catches this going wrong. **Below roughly 300 words a
   topic file is almost certainly a fragment of another one**, and most of what
   is there will be heading and metadata rather than content; find the file it
   belongs to and append instead. **Above roughly 2000 words it is carrying more
   than one subject** and wants splitting along the seam. Between those, one
   file per subject, added to as the talk comes back to it.

   The freeze on a committed topic is against **rewriting**, not against
   accretion. Never edit or delete what is already recorded, because that is the
   audit trail; adding a later section is how a transcript works and is
   expected. Each entry records the decision, the reasoning, the alternatives
   considered, and what else the decision reaches. Do not open doc or src
   changelists yet. The talk produces topics, not implementation.

5. Re-present before lock. Before moving any topic toward implementation, from
   the doc changelist onward, run one consolidation AskUserQuestion: restate the
   decisions made, what each one reaches, and an explicit list of everything the
   agent is still wary or unsure about. This is the single confirmation gate,
   and it is confirmed or redirected there.

6. Then proceed. Once confirmed, the talk is done. The implementation flow takes
   over on the agreed path, and the topic files are the durable record it works
   from.

## Notes

- Topic files are the deliverable of the talk, not a side effect. They make each
  decision permanent and reviewable in the repository that owns the work.
- **A round holds one topic file per subject, added to as the talk returns to
  it.** Commit each as soon as its subject settles; there is no waiting for the
  talk to end.
- A talk may seed several topics. Each becomes its own round later, so the talk
  is not lockstep with the round count.
- If a question cascades into a fresh design problem mid-talk, surface it as its
  own unit rather than letting one question sprawl.
- An expert dispatched for grounding gets a neutral brief, with no hint of which
  answer is wanted. A question put to a person does not: they asked the agent to
  lead and to recommend. Both still need real, balanced alternatives.

## See also

`design-round/SKILL.md` (the phases a topic file belongs to, and what opens the
next one), `mockup-workflow/SKILL.md` (where the round files live and how they
are named).
