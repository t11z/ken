---
name: phase-closure-audit
description: Audits a Ken project phase for closure readiness. Use this skill when asked to audit a phase, check if a phase can be closed, or produce a phase closure report. Invoke with a phase number, e.g. "audit phase 2 for closure".
allowed-tools: Bash, Read, Glob, Grep, Agent
---

# Phase Closure Audit

Produces `docs/maintainer/phase-N-closure-audit.md` — a structured audit report that answers whether a Ken project phase meets its closure criteria.

## Inputs

`$ARGUMENTS` must contain a phase number (e.g. `2`, `3`). If absent, stop and ask.

## Step 1 — Read the milestone

```bash
PHASE_NUM=$ARGUMENTS
gh api repos/t11z/ken/milestones --jq ".[] | select(.title | test(\"Phase ${PHASE_NUM}\"))"
```

Extract from the response:
- Full milestone description (the "what this phase delivers" text)
- The "Closes when:" section — these are the closure criteria
- All ADR references (pattern: `ADR-NNNN`)
- All closed issues (`gh api repos/t11z/ken/milestones/{id}/issues?state=closed`)

If the milestone is not found, stop and report.

## Step 2 — Build the ADR list

From the milestone text, extract every ADR-NNNN reference. Verify each exists in `docs/adr/`:

```bash
ls docs/adr/ | grep -E "^[0-9]{4}-"
```

For any referenced ADR that does not exist as an Accepted ADR file, record it as a blocking gap.

## Step 3 — Conformance check per ADR

For each ADR in the list, delegate to the `adr-conformance-auditor` agent:
```
Agent(
  subagent_type="adr-conformance-auditor",
  prompt="Check ADR docs/adr/NNNN-[name].md against the codebase. Relevant source paths: crates/"
)
```
Run these sequentially. Collect the structured Markdown fragment from each.

## Step 4 — Closure criteria check

For each "Closes when:" condition from the milestone, make a binary determination:

- MET: evidence exists in the codebase or closed issues
- NOT MET: condition is unmet, with explanation
- UNVERIFIABLE: requires runtime or manual verification (note what test would confirm it)

Use Read, Grep, and Glob to check code-verifiable conditions. For runtime conditions (e.g. "a family IT chief can click X and Y happens"), mark UNVERIFIABLE and note what manual verification is required.

## Step 5 — Aggregate and write the report

Write `docs/maintainer/phase-N-closure-audit.md` with this structure:

```markdown
# Phase N Closure Audit

**Date**: [today]
**Auditor**: Claude Code (adr-conformance-auditor agent, claude-opus-4-6)
**Milestone**: [title]

## Verdict

READY TO CLOSE | BLOCKED | REQUIRES MANUAL VERIFICATION

[One paragraph summary of the overall finding.]

## Closure Criteria

| Criterion | Status | Notes |
|-----------|--------|-------|
| [from "Closes when:"] | MET/NOT MET/UNVERIFIABLE | [evidence or explanation] |

## ADR Conformance

[Paste the structured fragments from the adr-conformance-auditor agent for each ADR]

## Blocking Issues

[List any DEVIATED ADR findings or NOT MET closure criteria that block closure. Empty if none.]

## Items Requiring Manual Verification

[List UNVERIFIABLE items with the specific test that would confirm them.]

## Open Issues at Audit Time

[List any open issues in the milestone at the time of the audit, if any.]
```

After writing the file, report the verdict and the path to the human.

## Error handling

- Milestone not found → stop, report the exact `gh` output
- ADR referenced but missing from `docs/adr/` → record as blocking gap, continue with remaining ADRs
- Agent timeout or empty response → record ADR as UNVERIFIABLE, continue
- `gh` auth failure → stop, tell the user to run `gh auth login`
