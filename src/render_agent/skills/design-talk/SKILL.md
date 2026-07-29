# Design talk flow

Op runs this when open design questions must be resolved before autonomous or normal implementation can proceed. The agent leads; op decides. The output is permanent paper-trail topic files in the repos the work touches, plus a clear agreed path the later implementation flow works from.

This is distinct from the `design-review` skill, which dispatches sub-agents to review a finished design or PR. This skill is for the synchronous decision conversation that happens before the work, with op in the loop.

## Steps

0. Consume what was filed for this round, before anything else. Run `scripts/consume-strays` from inside the repository. It scans every ref for flat topic files stranded on branches nobody is working on, and for archived round directories this branch cannot see, and pulls both in. A topic left open in TOPIC phase is a filing: somebody found something while working elsewhere and left it in the way so the next round would have to address it, and this is that round. Three ways they vanish, all observed: a topic on a branch that never merged, a topic swept into another round's archive by `close`, and an archive present only on an unmerged branch. Never start a round by branching fresh from trunk and leaving them behind, never stash one, never move one to another branch, and never close a round leaving one unmentioned. Every topic the script pulls in is named in this round's changelists, with what it changes or why it needs nothing. If one is genuinely out of scope it is re-filed as a fresh flat topic after this round closes, so the next round inherits it the same way.

1. Ground before asking. Read the relevant memos, rules, tasks, and the actual source. Resolve factual cruxes yourself (web research on tracking issues or specs, targeted greps, one neutral domain-expert dispatch per hard question) BEFORE putting anything to op. Op's time is for decisions, not for facts the agent can gather. A question that rests on a wrong premise (a feature not actually enabled, a file that moved, an inventory line that was a misread) wastes the talk and erodes trust in the agenda.

2. Set the agenda. Open with a compact orientation: the topics, the order (dependency-order, biggest-blast-radius decisions placed where they fit best), and what grounding is still running in the background. Keep it short; the questions carry the substance.

3. One unit per question. Drive with AskUserQuestion, one substantive decision per call. Closely-coupled small pairs may share a call, but prefer one. Each option names its concrete implication and cost. Presenting the agent's own recommendation is fine and expected here (unlike sub-agent prompts, where it is forbidden); mark the lean and still give real, balanced alternatives. State explicitly anything the agent is unsure about so op can weigh it.

4. Capture as you go, and let a topic file accrete. As each topic firms, write a flat mockspace TOPIC-phase file `mock/design_rounds/YYYYMMDDHHMM_topic.<slug>.md` into each touched repo, on a feature branch off dev, and commit it. **A topic file is a transcript, not a decision record.** When the talk returns to a subject the file already covers, append a section to that file rather than starting a new one. Start a new file when the *subject* changes, not when a question is answered.

   Size is the check that catches this going wrong. **Below roughly 300 words a topic file is almost certainly a fragment of another one**, and most of what is there will be heading and metadata rather than content; find the file it belongs to and append instead. **Above roughly 2000 words it is carrying more than one subject** and wants splitting along the seam. Between those, one file per subject, added to as the talk comes back to it.

   The freeze on a committed topic is against **rewriting**, not against accretion: never edit or delete what is already recorded, because that is the audit trail, but adding a later section is how a transcript works and is expected. Each entry records the decision, the reasoning, the alternatives considered, and the cross-repo consequences. Do not open doc or src CLs yet; the talk produces topics, not implementation.

5. Re-present before lock. Before moving any topic toward implementation (doc CL onward), run one consolidation AskUserQuestion: restate the decisions made, their cross-repo implications, and an explicit list of everything the agent is still wary or unsure about. This is the single confirmation gate. Op confirms or redirects.

6. Then proceed. Once confirmed, the talk is done. The implementation flow (autonomous or normal) takes over on the agreed path; the topic files are the durable record it works from.

## Notes

- Topic files are the deliverable of the talk, not a side effect. They make each decision permanent and reviewable in the repo that owns the work.
- **A round holds one topic file per subject, added to as the talk returns to it.** This is the intended flow. Commit each as soon as its subject settles; you do not wait for the talk to end. A committed topic is frozen against **rewriting**, never against **appending**: never edit or delete what is recorded, because that is the audit trail, but adding a later section is exactly how a transcript accretes. Start a new file when the subject genuinely changes. A file under roughly 300 words is a fragment of another one and should be appended there instead; a file over roughly 2000 words is carrying two subjects and wants splitting.
- A talk may seed several topics across several repos. Each becomes its own mockspace round later; the talk is not lockstep with round count.
- If a question cascades into a fresh design problem mid-talk, surface it as its own unit rather than letting one question sprawl.
- Sub-agents dispatched for grounding follow `subagent-prompt-neutrality.md`. Op-facing questions do not need neutrality (op asked the agent to lead and recommend), but they do need real, balanced alternatives.

## See also

`subagent-prompt-neutrality.md` (grounding dispatches), `mockspace-workflow.md` (TOPIC phase mechanics), `branch-pr-flow.md` (feature branch per repo), `.claude/skills/design-review/SKILL.md` (the sibling review flow).
