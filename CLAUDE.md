# CLAUDE.md — Ken Repository Root

This file is the entry point for any LLM working in this repository. Read it before reading anything else. It defines the rules of engagement, the role separation between architect and implementer, and the conventions every contributor — human or otherwise — is expected to follow.

This file is **immutable from Claude Code's perspective**. Claude Code may read it, must comply with it, and may never modify it without an explicit, narrow instruction from the human architect that names this file by path. The same rule applies to every other `CLAUDE.md` in the tree, to every file under `docs/adr/`, and to every file under `prompts/`.

## What Ken is

Ken is a single-tenant, self-hosted observability and remote-access agent for Windows endpoints in a family-IT context. One technically capable person (the *family IT chief*) runs a small server on a Raspberry Pi at home. The people they support — relatives, partners, close friends — install the Ken agent on their Windows PCs. The agent reports passive status (Defender state, update state, firewall state, BitLocker state, recent security events) to the server. The server presents this state to the family IT chief through a web UI. When the family IT chief needs to actually touch a remote machine, the agent shows a single dialog on the endpoint asking for explicit consent, and only then opens a remote-control session using an embedded RustDesk protocol stack.

Everything Ken does and refuses to do is governed by the ADRs in `docs/adr/`. Read them in numerical order before forming any opinion about what Ken should look like.

## Role separation

Ken has two LLM roles, and they must never blur:

**The Architect** (Claude in the Claude Project, working alongside the human owner). The Architect reasons about design, drafts ADRs, writes prompts for Claude Code, edits documentation, and answers strategic questions. The Architect does **not** write production code, does not run tests, does not commit to the repository directly.

**The Implementer** (Claude Code, invoked through prompts). The Implementer reads ADRs, reads prompt files, writes Rust code, writes tests, runs builds, opens pull requests. The Implementer does **not** make architecture decisions, does **not** modify ADRs, does **not** modify any `CLAUDE.md`, and does **not** modify any file under `prompts/` unless a prompt file explicitly instructs it to.

The boundary exists because architectural drift is invisible until it is irreversible. A model that is allowed to both decide and implement will, under pressure, decide in favor of what is easy to implement. Separating the two roles forces every decision to survive a written round-trip, which is the only mechanism that reliably catches the drift.

If Claude Code encounters a situation where it believes an ADR is wrong, missing, or unclear, the correct response is to **stop, document the question, and surface it to the architect** — not to make a judgment call and proceed.

## Files Claude Code may not modify

Without an explicit per-file instruction from the architect, Claude Code may not create, modify, or delete:

- Any file in `docs/adr/`
- Any file named `CLAUDE.md` at any depth in the tree
- Any file in `prompts/`
- Any file in `.github/` (workflows, issue templates, labels, configuration)
- `LICENSE`, `README.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`
- This file

Claude Code may freely create, modify, and delete:

- Source files under `crates/*/src/`
- Test files under `crates/*/tests/`
- `Cargo.toml` files within individual crates (but not the workspace root `Cargo.toml` without instruction)
- Generated files, build artifacts, lockfiles
- Files explicitly named in a prompt as the target of the work

When in doubt, read the prompt file again. If the prompt does not name a file, do not modify it.

## Repository structure

```
ken/
├── Cargo.toml              workspace root, defines members
├── crates/
│   ├── ken-protocol/       shared wire types between agent and server
│   ├── ken-agent/          Windows binary; SYSTEM service + Tray app + embedded session
│   └── ken-server/         Linux/ARM64 binary; HTTP server, SQLite, web UI
├── docs/
│   ├── adr/                Architecture Decision Records, immutable once accepted
│   ├── architecture/       diagrams, longer-form design docs
│   └── user/               end-user documentation, install guides, consent explainer
├── prompts/                Claude Code prompt files, organized by phase and area
├── skills/                 in-repo Claude Code skills (SKILL.md format)
├── .github/
│   ├── workflows/          CI, release, page build
│   ├── ISSUE_TEMPLATE/
│   └── labels.yml
├── CLAUDE.md               this file
├── README.md
├── CONTRIBUTING.md
└── LICENSE                 AGPL-3.0
```

Each crate has its own `CLAUDE.md` with crate-specific conventions. Read the relevant one before working in that crate.

## Language and stack commitments

These are recorded in detail in their own ADRs; this section is a quick reference, not the source of truth.

- **Agent:** Rust, Windows-only, uses `windows-rs`, `windows-service`, embeds RustDesk crates for the remote-session subsystem.
- **Server:** Rust, Linux-only (target: ARM64 for Raspberry Pi, also x86_64 for development), uses `axum`, `sqlx` with SQLite, `rustls` for mTLS, `askama` for server-side templates.
- **Frontend:** Server-rendered HTML via askama templates, htmx for interactivity, Tailwind via static asset (no build pipeline).
- **Shared:** `ken-protocol` crate, depended on by both agent and server, defines all wire types.
- **License:** AGPL-3.0 across the entire workspace.
- **Toolchain:** pinned via `rust-toolchain.toml` at the workspace root.

## How Claude Code should work in this repository

When invoked with a prompt file:

1. **Read the prompt file completely** before doing anything else. Identify which ADRs it references and read those next.
2. **Read the relevant `CLAUDE.md`** for the crate or area being modified. Conventions in a sub-`CLAUDE.md` override or refine this root file for that subtree.
3. **Plan the change in writing** as a comment or scratch note before editing files. The plan should name every file that will be touched and reference the ADR or prompt section that justifies each change.
4. **Make the change**, then run the relevant build and test commands. Do not consider the work complete until the build passes and tests pass. If a test cannot pass for reasons outside the scope of the prompt, surface this in the pull request description rather than disabling the test.
5. **Open a pull request** with a description that names the prompt file and the ADRs the change is built against. The PR description is the contract between implementation and intent.
6. **Do not commit to `main` directly.** All work goes through pull requests.

When invoked without a prompt file, for an ad-hoc change:

1. **Stop and ask** whether this work should be captured as a prompt file first. Most ad-hoc requests should become prompt files for traceability. The exceptions are trivial fixes (typos, formatting, dependency bumps) and explicit emergency changes.
2. **For trivial fixes**, proceed and open a PR with a descriptive title.
3. **For anything that touches behavior**, refuse politely and ask the architect to draft a prompt or to explicitly authorize an ad-hoc change.

## Style, hygiene, and quality

- **Rust style:** standard `rustfmt` defaults, `clippy` clean at the `warn` level, no `#[allow]` without a comment explaining why.
- **Commit messages:** imperative present tense, first line under 72 characters, body wrapped at 72. Reference the prompt file and any ADRs in the body.
- **Tests:** every non-trivial function gets a unit test. Integration tests go in `crates/*/tests/`. No test is allowed to depend on network access, on a clock, or on a specific filesystem layout outside the test's own tempdir.
- **Documentation:** every public item in every crate gets a doc comment. The doc comment names the ADR that justifies its existence whenever applicable.
- **Logging:** structured logging via `tracing`. Never log user data, never log credentials, never log session content. Log Ken's own actions.

## What this project is not

Ken is not a competitor to Wazuh, Velociraptor, Tactical RMM, or any commercial endpoint protection product. Ken does not detect malware, does not enforce policy, does not quarantine files, does not block processes, does not exfiltrate forensic artifacts, does not score endpoints, does not produce compliance reports. The list of things Ken refuses to do is in ADR-0001 and is binding.

Ken is also not a hobby project that happens to have an ADR directory. The architectural discipline is the product. If the discipline slips, the trust story slips, and the trust story is the only thing that distinguishes Ken from the long list of well-intentioned tools that became surveillance ware.

Read the ADRs. Follow the prompts. When in doubt, stop and ask.
