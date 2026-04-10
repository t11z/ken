# Writing ADRs

Load this skill when drafting or reviewing a new Architecture Decision Record. It covers the mechanical and stylistic conventions for ADRs in this repository. The decision-governance rules themselves (format, lifecycle, immutability, supersession) are in ADR-0000; this skill assumes you have read ADR-0000 and tells you how to actually write a good ADR in practice.

## Before you start

Every ADR is written by the architect, in the Claude Project, in collaboration with the human owner. Claude Code does not author ADRs on its own initiative. If you are Claude Code and you find yourself wanting to write one, stop and surface the question — either the task description you are working from should be updated to include the ADR draft, or the architect should draft the ADR first.

Before writing, confirm that the decision is actually ripe. An ADR that says "we will do X or Y, TBD" is not an ADR, it is a discussion note. The question the ADR answers should already have an answer; the ADR is where that answer is recorded.

## The title is the decision

Read the title in isolation. If a reader cannot tell from the title alone what the decision is, the title is too abstract. Good: *Use Rust for the Agent*. Bad: *Language Choice for the Agent*. The first commits; the second describes a discussion.

Titles are imperative. *Use*, *Adopt*, *Require*, *Forbid*, *Split*, *Replace*. Not *Consider*, *Evaluate*, *Discuss*, *Think about*.

## Context sets up the pressure, not the history

The Context section answers the question "why was this decision forced?" not "what happened in the project before?". A good Context paragraph makes the reader understand why ignoring the problem is no longer an option. Three to six sentences is usually enough. If you find yourself writing a page of context, you are probably trying to justify the decision — that belongs in the body, not here.

Concretely: a Context that says "We need to choose a database" is weak. A Context that says "The server persists endpoint state across restarts and must survive crashes; we have deferred this decision until now but the enrollment work requires a stable schema" is strong. The first describes a topic; the second describes a pressure.

## Decision is short, declarative, and unambiguous

Write the Decision section in present tense and active voice. *The server uses SQLite with `sqlx` for compile-time query checking.* Not *We have decided to use SQLite*. Not *SQLite will be used*. The ADR is the decision, not a record of having made one.

If the decision has multiple parts, number them or break them into subsections. Do not cram multiple decisions into one ADR. If you find yourself writing "and also," consider whether the "also" is its own ADR.

Avoid weasel words: *probably*, *usually*, *typically*, *where appropriate*, *as needed*. These words hide indecision. If the rule has exceptions, name them explicitly. If it does not, state it flatly.

## Consequences are honest

The Consequences section has three parts: what becomes easier, what becomes harder, and what we explicitly accept. All three are required, even when the decision is obvious. If you cannot name something that becomes harder, you are not thinking hard enough — every decision has a cost, and naming the cost is part of the commitment.

Do not write consequences that sell the decision. *The database will scale beautifully* is marketing. *Writes are serialized through a single SQLite file, which is acceptable at our deployment scale but would be a bottleneck at 10,000 endpoints* is a consequence.

## Alternatives considered prove you thought about it

At least one rejected alternative is mandatory, and the rejection reason must be substantive. *Considered Postgres, rejected because SQLite was simpler* is weak. *Considered Postgres for its concurrent write performance; rejected because the server's write volume is dominated by heartbeat updates at a rate (~10/minute per endpoint) where SQLite's single-writer model is not a bottleneck, and Postgres would add operational complexity (a second process to deploy, a second thing to back up) that is not justified by the deployment profile* is strong.

The point of this section is not to list every possibility — it is to prove that the decision survived contact with at least one serious alternative. One or two well-reasoned rejections are better than five superficial ones.

## Length and rhythm

A typical ADR fits on one screen. A long ADR is a sign that either the decision is not yet clear, or it is actually two decisions hiding in one document. When editing, cut ruthlessly. Every sentence that does not advance the decision, the context, the consequences, or the alternatives should be deleted.

Prose, not bullet lists, for the Context and Consequences sections. Bullets are acceptable in the Decision section when the decision has distinct enumerable components, but they are not a substitute for thinking.

## After writing

Before marking an ADR as Accepted:

1. Read it end to end in one pass. Can a reader who has never seen the project understand the decision?
2. Check the title against the Decision section. Do they agree?
3. Check the Consequences against the Decision. Are the consequences actually caused by this decision, or are they general project properties?
4. Check the Alternatives. Is the rejection reason specific enough that someone could not read it and think "well that doesn't seem that bad"?
5. Check the file name. Does it match the title in kebab-case?

If all five checks pass, flip the status to Accepted and commit. From that moment the ADR is immutable.
