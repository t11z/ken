# .claude/skills

Skills are short, focused reference documents that Claude Code loads before working on a particular class of task. They encode conventions, patterns, and gotchas that would otherwise have to be rediscovered on every prompt. Each skill lives in its own subdirectory as a `SKILL.md` file.

## What a skill is

A skill is not an ADR and is not a prompt. An ADR records a decision that is binding on the project; a prompt instructs Claude Code to perform a specific piece of work. A skill is in between: it is a living reference that describes *how* to do something when you already know *what* to do and *why*.

Think of a skill as the answer to the question "if I were onboarding a Rust developer who had never seen this codebase, what would I tell them before they started on X?" — where X is a specific, recurring class of task.

Skills are allowed to evolve. Unlike ADRs, they are not immutable. When the working style of the project shifts, the corresponding skill is updated. Updates go through a pull request with an architect review, but they do not require an ADR.

## What a skill is not

Skills do not record decisions. If a `SKILL.md` file starts explaining *why* a particular approach was chosen, the "why" belongs in an ADR and the skill should reference the ADR instead.

Skills do not replace code review. A skill tells you how to write something; the reviewer tells you whether the result is actually good. A skill that tries to preempt every possible review comment will become unreadable long before it becomes complete.

Skills are not tutorials for Rust, axum, or any other external tool. They assume the reader knows the tool; they describe how this project uses it. For Rust basics, read the Rust book; for axum basics, read the axum docs.

## Structure

Each skill is one file: `SKILL.md` in its own subdirectory under `.claude/skills/`. The subdirectory name is kebab-case and matches the skill topic.

```
.claude/skills/
├── README.md                           this file
├── writing-adrs/
│   └── SKILL.md
├── rust-workspace-hygiene/
│   └── SKILL.md
├── windows-service-patterns/
│   └── SKILL.md
├── axum-askama-htmx/
│   └── SKILL.md
└── mtls-with-rustls/
    └── SKILL.md
```

A skill file has a short opening paragraph explaining what it covers and when to load it, followed by the actual content as prose and code examples. No strict structure beyond "be useful to the person reading it before they start a task."

## When Claude Code should load a skill

Claude Code should load a skill when the current task matches the skill's stated scope. The decision is made from the task description: if the task says "implement the enrollment endpoint," the relevant skills are `axum-askama-htmx`, `mtls-with-rustls`, and `rust-workspace-hygiene`. Load them, then proceed.

If no existing skill matches the task, Claude Code proceeds without one. It does not invent a skill mid-task. If the task reveals a pattern that would benefit from a skill, Claude Code surfaces this in the pull request description so the architect can decide whether to add one.

## When the architect should add a skill

- When the same gotcha has come up in two or three prompts and the architect is tired of repeating it
- When a convention is stable enough to codify but not important enough to enshrine in an ADR
- When a prompt would otherwise have to explain a large amount of context that applies to many future prompts

Skills are created by the architect. Claude Code does not author or modify skills on its own initiative; it reads them, applies them, and surfaces feedback in pull request descriptions.
