<!--
Thank you for contributing to Ken. Please fill out this template so
that maintainers can review the change efficiently.

If this is a draft PR for work in progress, mark it as such and
disregard the sections you have not yet completed.
-->

## What this PR changes

<!-- A short, concrete description. One paragraph is usually enough. -->

## Why

<!--
What problem is being solved? Link to the issue this PR resolves.
If the change is driven by an ADR, link to it.
-->

- Resolves #
- ADR(s) this implements or references:

## How the change was validated

<!--
Describe what you ran locally. The four commands below are the minimum
CI will check; mention any additional testing you did.
-->

- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] `cargo build --workspace --release` succeeds
- [ ] New functionality is covered by tests
- [ ] Documentation is updated where relevant

## Trust boundary review

<!--
Ken is governed by ADR-0001. Confirm that this PR does not touch any
Tier 1 invariant, and name any Tier 2 boundary it interacts with.
-->

- [ ] This PR does not modify, weaken, or bypass any Tier 1 invariant in ADR-0001.
- [ ] If this PR touches a Tier 2 scope boundary, the change is backed by an accepted ADR (linked above).
- [ ] No new network endpoints, no new third-party data exports, no new telemetry.

## Notes for reviewers

<!--
Anything the reviewer should pay extra attention to. Design choices
you are uncertain about. Alternatives you considered and rejected.
Follow-up work that will come in a later PR.
-->
