# New Issue

Creates a new GitHub Issue in t11z/ken following the Ken issue standard.

## Input

$ARGUMENTS contains the issue title or a short description.
If empty, ask for the title and type before proceeding.

## Step 1 — Determine issue type

Classify the issue based on $ARGUMENTS:

- **implementation-gap**: something that an ADR requires to exist but is not yet implemented
- **drift**: code deviates from an Accepted ADR
- **new-feature**: new capability; requires ADR work before implementation
- **bug**: behavior deviates from specification; no new ADR needed
- **hardening**: quality or robustness improvement without a new decision

If unclear, ask for the type.

## Step 2 — Identify ADR references

For implementation-gap, drift, new-feature, and hardening:

```bash
ls docs/adr/ | sort
grep -l "[relevant keywords]" docs/adr/*.md
```

Skim the relevant ADRs. Identify which ADRs this issue touches.

For bug: ADR reference is optional but useful when the bug violates an ADR requirement.

## Step 3 — Compose the issue body

Build the issue body using this template:

```
## Summary

[One sentence: what is the problem or task]

## Context

[2–4 sentences: why this issue exists, what triggered it]

## ADR References

[List of relevant ADRs with title, or "none" for pure bugs]
- ADR-NNNN — [Title]

## Acceptance Criteria

- [ ] [Concrete, verifiable criterion]
- [ ] [Further criterion]

## Notes

[Optional hints for the implementer: known constraints, related issues, risks]
```

## Step 4 — Create the issue

Show the composed body for confirmation first. Wait for an explicit "ok" or "yes"
before running the gh command.

After confirmation:

```bash
gh issue create \
  --title "[title from $ARGUMENTS]" \
  --body "[composed body]" \
  --label "[implementation-gap|drift|new-feature|bug|hardening]"
```

Output the issue URL.
