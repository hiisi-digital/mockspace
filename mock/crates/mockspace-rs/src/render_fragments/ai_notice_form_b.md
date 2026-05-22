<!-- mockspace:ai-notice-form-b -->
## Responsible tooling

- Never use a coding agent unless you know what you are doing, and be
  aware of the environmental and social implications of large-scale model
  inference. Be responsible and minimise the use of agents where not
  needed.
- Prefer agents as assistants over autonomous agents. A human in the
  loop catches mistakes, provides context, and takes responsibility.
  Autonomous agents should be the exception, not the norm.
- The `Co-Authored-By` byline marks **autonomous** agent work: work
  produced with NO human in the conversational loop at the time of
  writing. Examples: a cron-driven agent running through the night, an
  agent triggered by a PR-open webhook that commits without human
  review, an agent invoked by another agent with no human in the
  chain. In those cases the agent IS the author and the byline is
  mandatory transparency.
- For agent-as-assistant work (a human is in the loop, reading agent
  output, redirecting choices, approving direction), there is **NO
  byline regardless of how much code or prose the agent produced**.
  The human's direction is what makes it their commit; the agent is a
  tool they ran. Volume of keystrokes does not shift authorship.
  "I chose to sketch witness propagation; the agent wrote the sketch I
  asked for" is assistant work. "I told the agent to fix any failing
  tests overnight; it picked which ones and how" is autonomous work.
- The bright-line test: **was a human in the conversational loop at
  the time of the work, reading, redirecting, and approving direction?**
  If yes, NO byline, even if the agent wrote every keystroke. If no,
  byline mandatory.
- When unsure, skip the byline. Misattributing assistant work as
  autonomous pollutes the audit trail with false signals and inflates
  the agent's apparent contribution. The cost of a missing byline on
  genuinely autonomous work is low (the human can add it in a
  follow-up); the cost of a wrong byline on assistant work is harder
  to undo and visibly wrong to anyone who knows the work was directed.
