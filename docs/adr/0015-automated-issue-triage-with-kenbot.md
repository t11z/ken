# ADR-0015: Automated Issue Triage via KenBot

- **Status:** Accepted
- **Date:** 2026-04-10
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

Ken is currently maintained by a single architect. Triage of new issues — reading them, classifying them against the label taxonomy, recognizing which are trivial and which require architectural review, and either fixing the trivial ones or recording a useful first analysis — is repetitive work that consumes attention disproportionate to its value. The same architect is also the only person who can write ADRs and the only person who reviews implementation, so any minute saved on triage is a minute returned to load-bearing work.

At the same time, an automated agent operating on the repository is itself a trust subject. Ken's whole design rests on the idea that privileged software must be governed by explicit, documented boundaries rather than by good intentions or convention. A bot that opens branches, posts analyses, and changes labels on the project's own repository deserves the same discipline as the agent on a Windows endpoint: a stated purpose, an explicit scope, a hard list of what it will not do, and an identity in the audit log that distinguishes it from the human architect.

This ADR establishes such a bot — KenBot — and codifies its scope. The scope is deliberately narrow at the start. The bot exists to remove friction from triage and from a small set of mechanical fixes, not to participate in architecture or implementation of substantive changes.

## Decision

### Purpose

KenBot is an automated triage assistant for issues in the `t11z/ken` repository. Its job is to read newly filed issues, classify them against the existing label taxonomy (see `.github/labels.yml`), produce a structured analysis comment, set the appropriate next status label, and — for a small whitelisted subset of issue types — open a pull request with a proposed fix on a dedicated branch. KenBot does not participate in architectural decisions, does not write ADRs, and does not merge.

### Identity

KenBot operates as a GitHub App named "KenBot," installed on `t11z/ken`. The App's permission scopes are limited to the minimum required for its function: `contents:write`, `issues:write`, `pull_requests:write`, and `metadata:read`. No other permissions are granted. Commits, pull requests, and comments authored by the bot appear in the GitHub audit log under the identity `kenbot[bot]`, distinct from the human architect.

The choice of a GitHub App over the default `GITHUB_TOKEN` or a personal access token is deliberate: it makes the bot's identity visible in the audit log, makes its permission set explicit rather than inherited from a shared workflow token, and allows branch protection rules on `main` to exclude the bot from any bypass list without ambiguity.

### Trigger

KenBot reacts only to the event of the label `status/needs-triage` being newly added to an issue. It does not react to issue creation directly, to issue edits, to comments, or to other label changes. This ensures that the bot runs exactly once per triage cycle, that human-edited or already-triaged issues are not re-processed, and that the bot's own label changes do not retrigger it.

### Sphere of action: triage versus autonomous fix

KenBot has two modes of operation, and they are governed by two separate exclusion lists. The first determines whether KenBot acts on an issue at all. The second determines, for issues KenBot does act on, whether it is allowed to open a pull request in addition to its analysis comment.

#### Exclusions from any action

KenBot **refuses to act and exits silently** (no comment, no label change, `status/needs-triage` remains in place) if any of the following labels is present on the issue: `trust/tier-1`, `trust/tier-2`, `status/needs-adr`, `status/blocked`, `status/wontfix`, or `type/adr`. These are issues where a bot's analysis is unwelcome (the architect's call is needed first) or where the issue is already in a terminal or held state.

Note that `priority/critical` is not on this list. Critical issues benefit from a fast first-pass analysis, and KenBot's analysis is a useful data point that does not commit the project to anything. But `priority/critical` is on the second exclusion list below: KenBot will analyze a critical issue, but it will not open a fix PR for it.

#### Action for non-excluded issues

For all non-excluded issues, KenBot **produces an analysis comment** (following the schema below) and **changes the issue's status label**. The new status label is `status/in-progress` if KenBot also opens a pull request, otherwise `status/needs-discussion`. The `status/needs-triage` label is removed in either case. KenBot also applies its own idempotency label, `kenbot/analyzed`, to mark the issue as already processed by the bot.

#### Exclusions from the autonomous-fix path

KenBot **opens a pull request with a proposed fix** if and only if **all** of the following are true:

- The issue carries one of the following label combinations:
  - `type/docs` (any area)
  - `type/bug` together with `area/ci`
- The issue does **not** carry any of: `priority/critical`, `trust/tier-1`, `trust/tier-2`, `status/needs-adr`.
- KenBot's own analysis does not conclude that the fix would require an architectural decision not yet recorded as an Accepted ADR. If it does, KenBot omits the PR, sets `status/needs-adr` instead of `status/needs-discussion`, and explains in the comment which decision is missing.

For all issues that pass the first exclusion list but fail any of the conditions in this second list, KenBot's contribution is the analysis comment alone. The whitelist is intentionally narrow at the outset and may be widened by a future ADR after the bot's behavior in production has been observed.

### Pull request discipline

Pull requests opened by KenBot follow these rules without exception:

1. The PR is opened from a branch named `kenbot/issue-<number>-<short-slug>`. The branch is created from the current `main`.
2. KenBot never pushes directly to `main`. Branch protection on `main` enforces this on the GitHub side; this ADR enforces it on the design side.
3. KenBot never merges any pull request, including its own. Merging is reserved for the architect.
4. KenBot never force-pushes to a branch that is not its own. On its own branches, force-push is permitted only as part of the same workflow run that created the branch (i.e. for amending its own initial commit before the PR is opened); after a PR exists, the branch is append-only from the bot's perspective.
5. The PR description references the originating issue with `Refs #<number>` (not `Closes`, since KenBot does not close issues; the architect does so on merge).
6. The PR is opened in draft state.

### Comment schema

Every analysis comment KenBot posts has a fixed structure: a short restatement of the issue in the bot's own words; the bot's classification against the label taxonomy with any suggested missing labels noted as proposals (not applied); an enumeration of solution options with explicit pro/con for each; and a recommended option with reasoning. If KenBot is also opening a PR, the comment links to it.

If KenBot must retract an analysis — because it was wrong, or because its PR was withdrawn — the retraction is a two-step act: edit the original comment to prepend a one-line note explaining the retraction, and remove the `kenbot/analyzed` label from the issue. Removing the label re-enables KenBot to analyze the issue again on the next `status/needs-triage` event, so the architect has an explicit "retract and rerun" workflow without needing to manually delete history. The retraction note is preserved in the edited comment as part of the audit trail.

### Idempotency

The `kenbot/analyzed` label is the source of truth for whether KenBot has already processed an issue. KenBot reads this label at the start of every run and exits silently if it is present. The label is owned by KenBot: the bot is the only entity that should set or remove it, and manual manipulation of the label by humans is discouraged except as part of the retraction workflow described above.

### Authentication against Anthropic

KenBot uses the `claude-code` toolchain to perform its analysis and code generation. It authenticates against Anthropic's API using a `CLAUDE_CODE_OAUTH_TOKEN` stored as a GitHub repository secret. The token is bound to the architect's Claude Max subscription. This is an explicit acceptance that the architect personally bears the cost and rate limits of the bot's operation.

Because the token is sensitive, the workflow that uses it never runs on `pull_request` events from forks (where secrets would leak to untrusted code), never echoes the token in logs, and never passes it to any process other than the official Claude Code toolchain.

### Rate and reentrancy limits

KenBot's workflow declares a concurrency group of `kenbot-issue-${{ github.event.issue.number }}` with `cancel-in-progress: false`, ensuring that two simultaneous triage runs on the same issue cannot interleave. The `kenbot/analyzed` label provides the per-issue idempotency guarantee: KenBot never processes an issue twice unless the architect has explicitly retracted the previous analysis.

### Out of scope

KenBot does not, and under this ADR will not:

- Write or modify ADRs.
- Merge pull requests.
- Push to `main`.
- Modify `.github/labels.yml`, `.github/workflows/`, `docs/adr/`, `CLAUDE.md`, or any file under `.claude/skills/`.
- React to issues outside the trigger condition above (in particular: not to comments, not to PR events, not to issue creation directly).
- Apply labels other than (a) the status transitions defined in the action matrix and (b) its own `kenbot/analyzed` idempotency label. Suggested missing labels are mentioned in comments, never applied.
- Perform any action on issues that lack `status/needs-triage` (or that already carry `kenbot/analyzed`).

A change to any of these scope limits requires a superseding ADR.

## Consequences

**Easier:**
- New issues receive a structured first pass within minutes, with classification and option analysis already in the comment thread when the architect arrives.
- Mechanical fixes — typos, broken links, CI configuration errors — can land as draft PRs without consuming architect attention until review time.
- The label taxonomy gains operational meaning. Labels are no longer purely descriptive; they drive bot behavior. This raises the cost of changing them carelessly, which is a feature, not a bug.
- The `kenbot/analyzed` label gives the architect a precise, machine-readable view of which issues the bot has already touched, and a one-click retraction workflow for cases where KenBot was wrong.

**Harder:**
- The label taxonomy in `.github/labels.yml` is now load-bearing for an automated agent. Renaming a label, removing one, or changing the semantics of a status transition can break or corrupt KenBot's behavior. Future label changes must be made with this in mind, and the relevant section of this ADR may need to be updated alongside any such change (via a superseding ADR if the change is substantive).
- The architect must monitor KenBot's first weeks of operation actively to confirm its analyses are useful and its proposed fixes are reasonable. A bot that produces low-quality analyses is worse than no bot, because it consumes review attention without saving triage attention.
- A new failure mode exists: a bug in KenBot's logic could produce a stream of incorrect comments or label changes across many issues before being noticed. The mitigations (single-trigger event, idempotency via the `kenbot/analyzed` label, narrow PR whitelist, two separate exclusion lists) make this unlikely but not impossible.

**Accepted:**
- KenBot's operating cost is borne by the architect's personal Claude Max subscription. There is no separate Ken project budget. If the bot's volume ever exceeds what the subscription comfortably absorbs, the response is to narrow the bot's scope, not to introduce a project-funded API key.
- KenBot's analyses will sometimes be wrong, and its proposed fixes will sometimes be rejected. This is acceptable: the bot is a triage assistant, not an authority. The retraction protocol exists precisely so that wrong analyses leave a visible trail rather than vanishing.
- The bot's autonomous-fix whitelist is conservative enough that it will frequently produce only an analysis comment when a more confident bot might have opened a PR. This is the correct trade-off at the outset. Widening the whitelist is a future ADR's job, after evidence has been gathered.
- KenBot will analyze `priority/critical` issues. This is deliberate: a fast structured first pass on a critical issue is more valuable than the marginal risk of a wrong analysis comment, and the autonomous-fix exclusion ensures KenBot cannot do anything irreversible to such issues.

## Alternatives considered

**No bot at all, all triage by hand.** Rejected because triage is the highest-volume, lowest-value activity in the architect's day. The cost of writing this ADR and the workflow is amortized across many future triage cycles.

**A bot that runs under the default `GITHUB_TOKEN` and the `github-actions[bot]` identity.** Rejected because it makes the bot's identity invisible in the audit log (any maintainer's automation looks the same), inherits a broad permission set rather than declaring an explicit one, and cannot be excluded from `main` branch protection cleanly. The marginal effort of setting up a GitHub App is small and the governance gain is large.

**A bot that classifies issues but never opens pull requests.** Rejected as too narrow. The cost of the autonomous-fix path is small if the whitelist is narrow, and the value of having `type/docs` fixes appear as reviewable PRs without architect involvement is real. The right answer is a narrow whitelist, not no whitelist.

**A bot that opens pull requests for any issue type it considers tractable.** Rejected as too broad. The Phase 1 incident in this project's history demonstrates what happens when an automated agent is allowed to make decisions implicitly rather than working from an explicit list. The whitelist is the explicit list.

**A single exclusion list governing both action and autonomous-fix.** Rejected because it conflates two different risks. "Should KenBot speak about this issue at all" and "should KenBot try to fix this issue without human review" are different questions with different correct answers, most clearly in the case of `priority/critical`: speaking is helpful, fixing is not. Two lists make this distinction visible in the ADR rather than buried in workflow logic.

**An HTML marker in the comment instead of a label for idempotency.** Rejected because a label is queryable in the GitHub API without fetching comment bodies, is visible to humans browsing the issue list, and creates a clean retraction workflow (remove the label to allow re-analysis). The marginal cost of adding one label to the taxonomy is small.

**Reacting to issue creation (`on: issues, types: [opened]`) instead of to label addition.** Rejected because the issue templates apply `status/needs-triage` automatically on creation, so the trigger condition is the same in practice — but the label-addition trigger also handles the case where an issue is filed without a template, triaged manually by the architect, and only later marked `needs-triage` for bot attention. The label trigger is strictly more flexible.
