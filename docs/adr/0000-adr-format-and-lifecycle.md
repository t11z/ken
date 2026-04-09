# ADR-0000: ADR Format and Lifecycle

- **Status:** Accepted
- **Date:** 2026-04-09
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

Ken is an architecture-driven project. Decisions are made by a human architect in collaboration with an LLM sparring partner, and implementation is delegated to Claude Code via prompt files. In this setup, the value of the project does not live in the source tree alone — it lives in the *reasoning behind* the source tree. Without a durable, structured record of why decisions were made, that reasoning evaporates the moment the conversation window closes.

Architecture Decision Records (ADRs) solve this by capturing each significant decision as a small, immutable document committed alongside the code it governs. They are the project's memory, its onboarding material, and its defense against drift.

This ADR defines how all subsequent ADRs in this repository are written, numbered, reviewed, and retired. It is itself an ADR — the first one — so that the format is governed by the same mechanism it describes.

## Decision

### Location and naming

All ADRs live in `docs/adr/`. Each ADR is a single Markdown file named `NNNN-kebab-case-title.md`, where `NNNN` is a zero-padded four-digit sequence number starting at `0000`. Numbers are assigned in strict creation order and never reused, even when an ADR is superseded or rejected.

Example: `0007-rust-toolchain-pinning.md`

### Required structure

Every ADR uses the following sections, in this order, with these exact headings:

1. **Title line** — `# ADR-NNNN: Short Imperative Title`
2. **Metadata block** — bullet list with `Status`, `Date`, `Deciders`, `Supersedes`, `Superseded by`
3. **Context** — what situation or pressure prompted the decision; what was true before
4. **Decision** — what we decided, in unambiguous declarative language
5. **Consequences** — what becomes easier, what becomes harder, what we explicitly accept
6. **Alternatives considered** — at least one rejected option with a brief reason

Optional sections allowed at the end: **Notes**, **References**, **Open questions**.

The title is imperative and concrete: `Use Rust for the Agent`, not `Language Choice for the Agent`. The reader should know the decision from the title alone.

### Status values

An ADR is always in exactly one of these states:

- **Proposed** — drafted, under discussion, not yet binding
- **Accepted** — decision is in force; implementation may proceed against it
- **Rejected** — explicitly decided against; kept in the repo as a record of the considered path
- **Superseded** — was Accepted, has been replaced by a newer ADR; the `Superseded by` field points to the replacement

ADRs never move backwards. An Accepted ADR cannot return to Proposed. If an Accepted ADR turns out to be wrong, the response is to write a new ADR that supersedes it, not to edit the old one.

### Immutability

Once an ADR is **Accepted**, its **Context**, **Decision**, **Consequences**, and **Alternatives considered** sections are frozen. Typo fixes and clarifications that do not change meaning are allowed and noted in the commit message. Substantive changes are forbidden — they must take the form of a new ADR that supersedes the old one.

This is the most important rule in this document. The point of an ADR is to be a stable reference. Editing an Accepted ADR after the fact destroys the trust that makes the system work.

### Lifecycle

1. **Drafting.** A new ADR is created with status `Proposed`. The next sequence number is taken from the highest existing number plus one.
2. **Discussion.** The architect and the sparring partner discuss the draft. Iteration on a Proposed ADR is free and expected.
3. **Acceptance.** When the architect is satisfied, the status is changed to `Accepted` and the date is updated to the acceptance date. From this moment the ADR is immutable.
4. **Implementation.** Claude Code is given prompts that reference the Accepted ADR by number. Code is built to comply with it.
5. **Supersession** (if needed). When an Accepted ADR no longer reflects the chosen direction, a new ADR is written. The new ADR's `Supersedes` field names the old one. The old ADR is updated *only* in its metadata block: status becomes `Superseded`, and `Superseded by` is filled in. Its body remains untouched.

### Authorship rules

ADRs are authored by the human architect in collaboration with the sparring partner (Claude in the Claude Project). They are **never** written or modified by Claude Code on its own initiative. Claude Code may *propose* an ADR by drafting one in a pull request, but acceptance is always a human decision.

This is part of the broader role separation defined in the root `CLAUDE.md`: architecture decisions belong to the architect, implementation belongs to Claude Code. ADRs are the boundary marker between the two domains.

### Length and tone

ADRs are short. A typical ADR fits on one screen. If a decision needs more than two pages of justification, the decision is probably not yet clear enough to be made. Tone is direct, declarative, and honest about trade-offs. ADRs do not sell the decision — they record it.

## Consequences

**Easier:**
- Every architectural commitment in Ken has a single, citable home.
- New contributors (human or LLM) can read `docs/adr/` in numerical order and reconstruct the project's reasoning.
- Drift between intent and implementation becomes visible: if the code stops matching an ADR, one of them is wrong, and the discrepancy must be resolved explicitly.
- Claude Code prompts can reference ADRs by number, giving the implementation an unambiguous source of truth.

**Harder:**
- Every significant decision now incurs the cost of writing an ADR. This is intended; it raises the bar for what counts as a decision worth making.
- Reversing a decision requires writing a new ADR, not editing an old one. This is also intended.

**Accepted:**
- The ADR directory will grow monotonically. Superseded ADRs remain in place as historical record. This is a feature, not a maintenance burden.
- Some decisions made early will look naive in hindsight. They stay in the record anyway. The honesty of the trail is more valuable than its polish.

## Alternatives considered

**Free-form architecture notes in a wiki or `docs/` directory.** Rejected because wikis drift, get edited silently, and lose causal links. ADRs in the repo are versioned alongside the code they govern and survive the same way the code does.

**MADR or another existing ADR template verbatim.** MADR is a fine standard and this format is heavily inspired by it, but Ken's ADRs add an explicit `Authorship rules` clause and a stricter immutability stance, both of which matter for the human-architect-plus-LLM-implementer model. Rather than adopt MADR and document deviations, the format is defined here directly.

**No ADRs at all, decisions captured only in commit messages and chat logs.** Rejected as the default failure mode of every project that ever wished it had documentation. Chat logs are not searchable by future contributors; commit messages are too narrow to carry the *why*.
