# Contributing to Ken

Thank you for considering a contribution to Ken. This document explains how to engage with the project and what to expect.

Ken has strong opinions about what it is and what it is not, and the contribution process reflects that. Reading this document end to end before opening an issue or pull request will save everyone time.

## Before anything else: read ADR-0001

Ken is governed by a set of Architecture Decision Records in [`docs/adr/`](docs/adr/). The most important one for contributors is [ADR-0001: Trust Boundaries and Current Scope](docs/adr/0001-trust-boundaries-and-current-scope.md). It lists both the invariants that will never change and the current scope boundaries that could be changed through a defined process.

A substantial fraction of reasonable-sounding feature requests touch items in ADR-0001. Sometimes they touch a Tier 2 boundary, which means the request is a legitimate candidate for a new ADR. Sometimes they touch a Tier 1 invariant, which means the request will not be accepted and the correct path is a fork under a different name. Either way, knowing which tier a request touches makes the conversation shorter and more productive.

## How decisions are made

Ken uses a two-role model for all significant changes:

- **The Architect** decides what Ken should do and why. Architecture decisions are recorded as ADRs, and ADRs are immutable once accepted. The Architect is currently a single person (the project owner), working with the community through discussions and pull requests.
- **The Implementer** executes decisions. Implementation is delegated to Claude Code via prompts. Human contributors implement work the same way: against a decision that already exists, not by inventing one along the way.

This separation is not bureaucracy for its own sake. It exists because Ken's trust story depends on every significant commitment surviving a written round-trip, and because implementation under time pressure tends to prefer what is easy over what is correct. Separating the roles forces the "correct" question to be answered first.

## Filing an issue

Issues are the right place to report bugs, request features, ask questions about Ken's behavior, and propose ADR changes.

When you file an issue, please:

1. **Check the existing issues** to see if the topic is already being discussed.
2. **Use the appropriate issue template.** The templates prompt for the information maintainers need to evaluate the issue efficiently.
3. **Be specific.** A bug report that says "doesn't work on my machine" is much harder to act on than one that says "on Windows 11 24H2, the agent fails to enroll with error `X` visible in `audit.log` at this timestamp."
4. **Link to ADRs when relevant.** If you are proposing a Tier 2 boundary change, reference the specific T2-N item you want to change and explain what you think the new behavior should be.

Issue templates exist for: bug reports, feature requests, ADR proposals, and documentation improvements.

## Proposing a change

There are three kinds of changes you might want to make. Each has a different path.

### 🐛 Bug fixes and small improvements

Open an issue first if the bug is non-obvious, then a pull request. For truly trivial fixes (typos, broken links, clear documentation errors), you can skip the issue and open a pull request directly.

Pull requests should:

- Stay focused. One logical change per PR.
- Include tests where applicable.
- Pass CI before review.
- Reference the issue they resolve.
- Explain what changed and why in the description.

### ✨ New features or behavior changes

New features almost always require an ADR. If you are unsure whether your proposed feature needs an ADR, the answer is probably yes — open an issue with the "ADR proposal" template and start the discussion there.

The path for a new feature is:

1. **Discussion issue.** Explain the problem, the proposed solution, and which existing ADR items the change touches.
2. **ADR draft.** If the discussion reaches rough consensus that the change is worth pursuing, a maintainer drafts an ADR (or invites you to draft one, with guidance). The draft starts in status `Proposed`.
3. **ADR review.** The draft is reviewed, refined, and either accepted or rejected. Acceptance is a deliberate act; it is not automatic from the discussion.
4. **Implementation.** Either Claude Code or a human contributor implements the change against the accepted ADR. Pull requests reference the ADR.
5. **Merge.** A maintainer reviews and merges.

This process sounds heavy for small features. In practice the ADR for a small feature is short (half a page), the implementation is a single focused PR, and the overhead buys traceability and a durable record of why the project looks the way it does.

### 📝 Documentation improvements

Documentation contributions are welcome and handled lightly. Open a pull request with the changes, describe what you improved, and the review will usually be quick. Documentation about *current behavior* is always easier to merge than documentation about *intended behavior* — the former is a fact, the latter is a decision and may require a discussion first.

The exceptions are ADRs and the repository structure document: those are governed by the ADR process itself.

## What Ken will not accept

Pull requests that propose any of the following will be closed with a reference to ADR-0001:

- Telemetry to project maintainers, analytics, crash reporting to a central service, or any form of "phone home"
- A hosted Ken service or a cloud component of any kind
- Multi-tenant separation as a feature
- Removal of the per-session consent gate for remote-control operations
- Removal of the user-readable local audit log or the user-accessible kill switch
- Relicensing to MIT, Apache, BSD, or any other more permissive license

These are not things a maintainer will be talked out of. They are structural commitments that make Ken what it is, and changing them would produce a different product.

Pull requests that propose changes to current Tier 2 boundaries (e.g., "Ken should be able to trigger a Defender scan on user confirmation") are welcome as discussion and ADR proposals, but will not be accepted as direct PRs without the preceding ADR.

## Coding style

Ken is a Rust project. The coding conventions live in the in-repo skills documents under [`.claude/skills/`](.claude/skills/). The relevant ones for contributors are:

- [`rust-workspace-hygiene`](.claude/skills/rust-workspace-hygiene/SKILL.md) — formatting, lints, dependencies, testing
- [`windows-service-patterns`](.claude/skills/windows-service-patterns/SKILL.md) — for agent work
- [`axum-askama-htmx`](.claude/skills/axum-askama-htmx/SKILL.md) — for server and frontend work
- [`mtls-with-rustls`](.claude/skills/mtls-with-rustls/SKILL.md) — for anything touching the TLS layer

These documents are kept short and practical. They describe the conventions Ken follows and the reasons behind them. If you have read the skill relevant to your change, you already know what the maintainers will look for in review.

## Running the test suite

From the repository root:

```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

CI runs these four commands on every pull request. Passing them locally first saves a round trip.

Windows-specific tests require a Windows runner and are gated with `#[cfg(windows)]`. CI runs them on a Windows worker; if you are contributing agent code from a non-Windows development machine, open the PR and let CI handle the Windows-side validation.

## Communication

- **Issues** for bugs, features, and ADR proposals.
- **Discussions** for open-ended questions, design exploration, and community topics. Use the GitHub Discussions tab.
- **Pull requests** for concrete changes.

There is no chat channel, no mailing list, no Discord server. The reason is honest: Ken is a small project maintained by a small number of people in limited time, and every additional communication venue is another place where expectations accumulate and answers get lost. GitHub is enough.

## Code of conduct

Be kind. Assume good faith. Criticize ideas, not people. When you disagree, explain your reasoning. When you are wrong, say so and move on.

The project does not yet have a formal Code of Conduct document. If Ken grows to a point where a formal document is needed, one will be added through the same ADR-style process as every other significant commitment. Until then, the standard of behavior is what you would expect from a well-run open-source project run by adults who care about the work.

## One last thing

Ken exists because someone wanted it to exist for their own family. If you are thinking about contributing, you are probably one of those people too. The project is much more interesting to maintain when the contributors are the users, and the users have real households with real needs. Bring your context. Tell us about the problem you are trying to solve. That is often worth more than the code.

Welcome.
