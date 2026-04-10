# Repository Structure

This document describes how the Ken repository is laid out and what belongs where. It exists because a tree alone does not tell you *why* a directory exists or what the rules for it are, and because conventions spread across many README files rot faster than conventions in one place.

This file is the single source of truth for repository layout. If you are about to add a top-level directory, a new crate, or a new class of document, it is recorded here first. If you are unsure where something belongs, the answer is here or it does not yet have a place.

## The tree

```
ken/
├── .claude/
│   ├── skills/
│   └── commands/
├── .github/
│   ├── workflows/
│   ├── ISSUE_TEMPLATE/
│   └── labels.yml
├── crates/
│   ├── ken-protocol/
│   ├── ken-agent/
│   └── ken-server/
├── docs/
│   ├── adr/
│   ├── architecture/
│   └── user/
├── CLAUDE.md
├── CONTRIBUTING.md
├── Cargo.toml
├── LICENSE
├── README.md
└── rust-toolchain.toml
```

## Top-level directories

### `.claude/`

Tooling configuration for Claude Code. This directory follows the dotfile convention for tool-specific metadata — the same category as `.github/`, `.vscode/`, `.devcontainer/`. Nothing here is part of the Ken product; everything here exists to configure how Claude Code operates on the repository.

`.claude/skills/` contains project-specific SKILL.md files. Skills are lightweight, living reference documents that Claude Code loads before working on a particular class of task. They encode conventions and patterns that would otherwise have to be rediscovered on every prompt. Unlike ADRs, skills are not immutable — they evolve as the project's working style evolves.

`.claude/commands/` contains slash commands for recurring workflows (drafting a new ADR, generating a phase-status report, scaffolding a new crate). This directory may be empty early in the project and fill up as patterns emerge.

Claude Code may read anything under `.claude/`. Claude Code may not modify anything under `.claude/` without an explicit, per-file instruction from the architect.

### `.github/`

GitHub-specific configuration. Workflows for CI, release, and GitHub Pages builds live in `.github/workflows/`. Issue templates live in `.github/ISSUE_TEMPLATE/`. Repository labels are defined declaratively in `.github/labels.yml` and applied by a workflow, so that label drift on the GitHub side can always be reconciled against the file in the repo.

This directory is off-limits to Claude Code without explicit instruction. Changes to CI or release machinery are architectural decisions, not implementation details.

### `crates/`

The Cargo workspace members. Each subdirectory is one crate with its own `Cargo.toml`, `src/`, and typically `tests/`.

`ken-protocol` is the shared crate that defines the wire types flowing between agent and server. It is depended on by both `ken-agent` and `ken-server` and has no dependencies on either. It is intentionally small and stable — changes here ripple across every component, so they happen through ADRs, not ad-hoc edits.

`ken-agent` is the Windows-only binary that runs on endpoint machines. It contains the SYSTEM service, the user-mode Tray App, the Named Pipe IPC between them, the embedded remote-session subsystem (built on RustDesk crates), and the local audit log. It is the only component of Ken that runs on user machines and the only one with elevated privileges.

`ken-server` is the Linux binary that runs on the family IT chief's Raspberry Pi. It contains the HTTP server, the SQLite persistence layer, the mTLS termination, the askama-rendered web UI, and the signaling relay for remote sessions. It builds for both ARM64 (the target) and x86_64 (for development on a regular machine).

Each crate has its own `CLAUDE.md` at its root with crate-specific conventions that refine or extend the root `CLAUDE.md`.

### `docs/`

All project documentation that is not code and not tooling configuration. Split into three subdirectories with distinct lifecycles and audiences.

`docs/adr/` holds the Architecture Decision Records. Each ADR is immutable once accepted, named `NNNN-kebab-case-title.md`, and governed by ADR-0000. These are the project's memory and its primary defense against drift. They are the first thing any contributor — human or LLM — should read.

`docs/architecture/` holds longer-form design documents that do not fit the ADR format: diagrams, protocol specifications, threat models, sequence flows. Unlike ADRs, these documents can be updated in place as the project evolves. They describe *what is*, not *what was decided*.

`docs/user/` holds end-user documentation: installation guides, the consent model explainer, the audit log reader's guide, troubleshooting notes. This is the material that gets rendered to GitHub Pages for the project website, and it is written for family IT chiefs and their users, not for developers.

## Top-level files

`CLAUDE.md` is the root entry point for any LLM working in this repository. It defines the Architect/Implementer role separation, the list of files Claude Code may not modify, and the high-level conventions that apply to the whole workspace. Sub-`CLAUDE.md` files in individual crates refine but do not override the root.

`Cargo.toml` is the workspace root manifest. It lists members, defines shared dependencies via `[workspace.dependencies]`, and pins shared lints via `[workspace.lints]`. Claude Code may not modify the workspace root `Cargo.toml` without explicit instruction, because adding or removing a workspace member is an architectural change.

`rust-toolchain.toml` pins the Rust toolchain version for the entire workspace. Updates to this file require an ADR, because toolchain bumps can introduce behavior changes across the codebase.

`README.md` is the project's public face on GitHub and the source of the GitHub Pages landing. It is written for people encountering Ken for the first time and is deliberately short: what Ken is, what it is not, how to try it, where to read more.

`CONTRIBUTING.md` describes how external contributors engage with the project: how to file issues, how to propose ADR changes, what the PR review process looks like, and what the project will and will not accept. It references ADR-0001 for the scope boundaries.

`LICENSE` is AGPL-3.0 and is non-negotiable per ADR-0001.

## Conventions that apply to the whole tree

**Filenames are kebab-case.** `trust-boundaries-and-current-scope.md`, not `TrustBoundariesAndCurrentScope.md` or `trust_boundaries.md`. This applies to documentation, prompts, and assets. Rust source files follow Rust conventions (`snake_case.rs`), which is the only exception.

**Markdown files do not have YAML frontmatter** unless required by a specific tool (e.g., Jekyll for GitHub Pages). ADRs use the metadata block format defined in ADR-0000, which is plain markdown, not frontmatter.

**Diagrams live next to the document that references them**, under `docs/architecture/diagrams/` if they stand alone, or inline as a sibling file if they belong to a specific ADR. Diagram source files (Excalidraw JSON, Mermaid source) are committed alongside any rendered exports.

**Generated files are not committed.** Build artifacts, target directories, and rendered documentation go through `.gitignore`. The only exception is rendered diagrams where the rendering step is not part of CI.

**No file in this repository is allowed to claim it is the index of something else.** There are no `INDEX.md` files, no `TABLE_OF_CONTENTS.md` files, no `FILES.md` files. Directory listings are authoritative; any file that attempts to duplicate a directory listing will drift from reality and mislead future readers. This document is the exception that proves the rule — it describes the tree's *meaning*, not its contents, and it is small enough to be audited at a glance.

## Where to put new things

- A new architecture decision → a new ADR under `docs/adr/`
- A new design doc that is not a decision → `docs/architecture/`
- A new end-user-facing document → `docs/user/`
- A new skill for Claude Code → `.claude/skills/`
- A new slash command for Claude Code → `.claude/commands/`
- A new crate → `crates/`, and update the workspace `Cargo.toml` through an ADR-authorized prompt
- A new CI workflow → `.github/workflows/`, via an explicit architect instruction
- A new issue template → `.github/ISSUE_TEMPLATE/`, via an explicit architect instruction
- Something that does not fit any of the above → stop and ask; the answer is either "add it to this document first" or "you are trying to add something that does not belong in this repository"
