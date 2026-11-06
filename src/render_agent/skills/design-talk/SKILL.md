# Design talk flow

Run when open design questions must be settled before implementation. The agent leads, the maintainer decides. Output: topic files kept in the repository, and an agreed path for the implementation flow.

## Steps

0. Consume what was filed for this round: run `scripts/consume-strays` from inside the repository. It scans every ref for flat topic files stranded on branches nobody is working on and for archived round directories this branch cannot see, and pulls both in.
   - A topic left open in TOPIC phase is a filing for the next round, and this is it.
   - Never start a round by branching fresh from trunk and leaving them behind. Never stash one, never move one to another branch, never close a round leaving one unmentioned.
   - Name every pulled-in topic in this round's changelists, with what it changes or why it needs nothing. Out of scope: re-file it as a fresh flat topic after this round closes.
1. Ground before asking. Read the relevant notes, rules, tasks and source. Resolve factual cruxes yourself (research tracking issues or specifications, targeted greps, or one neutrally briefed expert dispatch per hard question) before putting anything to a person. A question resting on a wrong premise wastes the talk.
2. Set the agenda: topics, their order (largest blast radius where it fits best), what grounding still runs in the background. Short.
3. One unit per question, driven with `AskUserQuestion`, one substantive decision per call (a closely coupled small pair may share one). Each option names its concrete implication and cost. Mark the agent's own lean and still give real, balanced alternatives. Say what the agent is unsure about.
4. Capture as you go. As each topic firms, write a flat TOPIC-phase file `<mock>/design_rounds/<id>_topic.<slug>.md` (`<id>` from `cargo mock round-id`, taken once for the round) and commit it.
   - A topic file is a transcript: when the talk returns to a covered subject, append a section. New file when the subject changes, not when a question is answered.
   - Under about 300 words it is a fragment of another file; over about 2000 words it carries two subjects, split it.
   - Never edit or delete what is recorded; appending is expected. Each entry records the decision, the reasoning, the alternatives considered and what else it reaches.
   - Do not open doc or src changelists yet.
5. Re-present before lock. Before moving any topic toward the doc changelist, run one consolidation `AskUserQuestion`: the decisions made, what each reaches, and an explicit list of everything the agent is still wary of. This is the single confirmation gate.
6. Once confirmed, the implementation flow takes over, with the topic files as the durable record.

## Notes

- A talk may seed several topics; each becomes its own round later.
- A question cascading into a fresh design problem is surfaced as its own unit.
- An expert dispatched for grounding gets a neutral brief with no hint of the wanted answer. A question put to a person does not; both need real, balanced alternatives.

See `design-round/SKILL.md` (phases, what opens the next) and `mockup-workflow/SKILL.md` (round file layout and naming).
