# ADR-0007: Architect and Implementer role separation

- **Status:** Accepted
- **Date:** 2026-04-09
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

Ken is built primarily through human–LLM collaboration. The human owner makes architectural decisions in dialogue with an LLM sparring partner running in a Claude Project. Implementation is delegated to a separate LLM context — Claude Code — that operates against the repository with file-system tools, build commands, and pull request authorship. This is a different operating model than a conventional software project with a single developer reading and writing code in one editor session, and it has different failure modes that need to be addressed at the governance layer.

The central failure mode is *architectural drift*: a situation where decisions accumulate implicitly during implementation, without ever being recorded as deliberate choices, until the codebase reflects an architecture nobody can explain. In a single-developer human project, this drift is bounded by the developer's memory and review habits. In an LLM-assisted project, the drift can happen much faster and at a much larger scale, because the LLM is willing to make hundreds of micro-decisions per session and has no continuity of memory across sessions to notice when those decisions add up to something the architect did not intend.

The mechanism that prevents this drift in conventional projects is review: a second pair of eyes that asks "why did you do this?" before code is merged. In a one-human-plus-LLM project, the human is necessarily the reviewer, but the human cannot review every line of every change at the rate the LLM produces them. A different mechanism is needed.

## Decision

Ken adopts a **strict separation between two roles**: the Architect, who decides, and the Implementer, who executes. The roles are held by different LLM contexts and operate under different rules.

**The Architect** is the LLM context running in the Claude Project, working in dialogue with the human owner. The Architect reasons about design, drafts ADRs, writes prompts for the Implementer, edits documentation, and answers strategic questions. The Architect does **not** write production code, does **not** run tests, and does **not** commit to the repository directly. The Architect's output is specifications and decisions, not implementations.

**The Implementer** is Claude Code, invoked through prompts prepared by the Architect. The Implementer reads ADRs, reads the prompt, writes Rust code, writes tests, runs builds, opens pull requests. The Implementer does **not** make architectural decisions, does **not** modify ADRs, does **not** modify any `CLAUDE.md` file, and does **not** invent specifications when the prompt is silent. When the Implementer encounters an unanswered architectural question, the correct response is to stop, document the question, and surface it to the human owner — not to make a judgment call and proceed.

The two roles communicate exclusively through artifacts that survive both contexts: ADRs in `docs/adr/`, CLAUDE.md files at the root and in each crate, skill files in `.claude/skills/`, and explicit prompts delivered out-of-band to the Implementer's context. There is no shared memory between the two contexts beyond what is committed to the repository.

The boundary is enforced primarily by convention, but the convention is reinforced by what each context is allowed to read and write. The Architect, in the Claude Project, has access to the conversation history and the project knowledge but cannot run `cargo build` or commit files. The Implementer, in Claude Code, has access to the file system and the build tools but cannot read the conversation history of the Claude Project. Each context is structurally limited to its role.

A corollary: **a prompt for the Implementer may not depend on an architectural commitment that is not already an Accepted ADR**. If the Architect finds itself wanting to write a prompt that would commit the project to a new technical choice, the correct sequence is to write the ADR first, have the human owner accept it, and only then write the prompt that implements it. The prompt translates ADRs into work; it does not introduce decisions of its own.

## Consequences

**Easier:**
- Architectural decisions have a single, durable, citable home: the ADR directory. The reasoning behind every significant choice is recoverable by reading the repository, without needing access to the conversation history of any specific LLM session.
- The Implementer's job is bounded and reviewable. When a pull request lands, the human owner can check it against the ADRs and the prompt it claims to implement, and assess "did this do what it was supposed to do" without re-deriving the architecture from scratch.
- Drift is visible. If the code stops matching an ADR, one of them is wrong. The discrepancy is forced into the open and must be resolved by either fixing the code or writing a superseding ADR. There is no comfortable middle ground where the code quietly diverges from intent.
- New contributors — human or LLM — can read `docs/adr/` in numerical order and reconstruct the project's reasoning. Onboarding does not depend on hallway conversations or chat histories.

**Harder:**
- Every architectural decision now incurs the cost of writing an ADR. This is intended; it raises the bar for what counts as a decision worth making, and it slows down the moment of commitment in exchange for durability.
- The Architect must resist the temptation to "speed things up" by embedding decisions into prompts rather than writing them as ADRs. This temptation is constant, because writing ADRs feels slower than writing prompts, and because the value of the ADR is invisible until much later when someone needs to understand why the project looks the way it does.
- Coordination between the two contexts is asynchronous and lossy. The Implementer cannot ask the Architect a clarifying question during a session — it can only stop and surface the question, then wait for the next prompt. This forces prompts to be more complete than they would be in a tightly-coupled setup.

**Accepted:**
- The role separation can be violated. There is no automated mechanism that prevents the Architect from writing a prompt that smuggles in undecided architecture, and no automated mechanism that prevents the Implementer from making a decision when the prompt is ambiguous. The discipline is held by the human owner's vigilance and by the explicit acknowledgement, in this ADR and in the Project's working agreements, that violations have happened and will happen again. The mechanism that catches violations is the post-hoc review of pull requests against the ADR set: if the code reflects an architecture that has no ADR, the response is to either write the missing ADR or roll the code back.
- The discipline costs time at the moment of decision and saves time at the moment of review. For a project that intends to last and that intends to be auditable by people other than its original author, this is the correct trade.

## Alternatives considered

**A single LLM context that both designs and implements.** Rejected because it concentrates all the failure modes — drift, optimization-for-easy-implementation, loss of architectural memory — into one place with no internal checks. The argument for separation is precisely that a context which can both decide and implement will, under pressure, decide in favor of what is easy to implement, and the bias is invisible from the inside.

**No separation, with the human owner as the only reviewer.** Rejected because the human owner cannot review every line of code at the rate the LLM produces it, and without a separate Architect context to draft the framing decisions, there is no anchor for the review to test against. The Architect's job is partly to prepare the questions that the human owner then answers; without that preparation, the review collapses into reading code and hoping to spot problems.

**Explicit hand-off documents for every change, like RFCs in larger open-source projects.** Rejected for Phase 1 as over-engineered. ADRs serve the same purpose at a smaller scale, and the project does not yet have multiple contributors to coordinate. If Ken ever grows to a state where multiple humans propose changes, the RFC pattern is a possible evolution, but it is not the right starting point.
