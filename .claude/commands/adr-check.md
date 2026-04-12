# ADR Coverage Check

Before writing any implementation prompt: verify that every architectural commitment
implied by the planned work is covered by an Accepted ADR.

## Input

$ARGUMENTS describes the planned work in one sentence
(e.g. "mTLS verifier for ken-agent", "SQLite migration for heartbeat table").

If $ARGUMENTS is empty, ask for the description before proceeding.

## Step 1 — Enumerate commitments

Read `CLAUDE.md` and the relevant crate-level `CLAUDE.md` files. Then enumerate
every architectural commitment the planned work implies:

- Language and crate ownership
- External dependencies (new or existing)
- Protocol and interface boundaries
- Data model changes
- Security model touches
- Deployment model touches

Be exhaustive. Every commitment a competent reviewer would call "a decision"
must appear on the list.

## Step 2 — Map to ADRs

Read `docs/adr/` and map each commitment to the ADR(s) that cover it:

```bash
ls docs/adr/ | sort
```

For each commitment:
- COVERED: name the ADR number and title
- UNCOVERED: mark the commitment explicitly as missing

## Step 3 — Output

Produce a clear table:

| Commitment | Status | ADR |
|------------|--------|-----|
| [description] | COVERED / UNCOVERED | ADR-NNNN or — |

Follow with an overall verdict:

**READY FOR PROMPT** — all commitments covered; the implementation prompt may be written.

or

**NOT READY** — N commitments uncovered. Required ADRs before the prompt:
- [list of missing ADRs with a one-line description of the decision each must record]
