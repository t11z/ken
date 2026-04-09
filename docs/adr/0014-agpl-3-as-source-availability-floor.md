# ADR-0014: AGPL-3.0 as the source-availability floor

- **Status:** Accepted
- **Date:** 2026-04-09
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

ADR-0001 in its first form contained a clause T1-7 that committed Ken to AGPL-3.0 licensing as a Tier 1 invariant. On reflection, that placement was a category error: AGPL is a *source-availability* commitment, not a *trust-boundary* commitment. The two are related — Ken's trust story depends on every user being able to inspect every line of code running on their machines, and AGPL is the mechanism that delivers that — but they belong in different ADRs. The trust boundaries in ADR-0001 are about what Ken does and refuses to do at runtime. The license commitment is about how Ken's source code is distributed and what obligations attach to distribution.

Mixing the two in a single ADR made ADR-0001 harder to read and harder to maintain. It also obscured the fact that the licensing decision has its own context, its own consequences, and its own rejected alternatives that are worth recording in their own right. This ADR extracts the licensing commitment into a dedicated record.

## Decision

Ken is licensed under the **GNU Affero General Public License v3.0 or later** (AGPL-3.0-or-later). This applies to every crate in the workspace, every contribution accepted into the project, and every binary distributed by the project.

The license commitment is one-directional: Ken may be relicensed in the future toward a *stricter* copyleft (for example, a hypothetical AGPL-4.0, or a license with even stronger source-availability requirements), but it may **not** be relicensed toward a more permissive license. MIT, Apache-2.0, BSD, MPL, LGPL, and any other license that would allow a downstream user to distribute Ken without making the source available to network users are all permanently off the table.

The "or later" clause in AGPL-3.0-or-later is deliberate. It grants downstream users the option to comply with a future, stricter version of the AGPL when one exists, without requiring Ken itself to relicense. This preserves flexibility on the strict-copyleft side while making the permissive side structurally impossible.

Every source file in the workspace that needs a license header (Rust source files, where the convention applies) carries an SPDX identifier `SPDX-License-Identifier: AGPL-3.0-or-later`. The `LICENSE` file at the repository root contains the full AGPL-3.0 text as published by the Free Software Foundation, unmodified.

Contributions are accepted under the same license. There is no Contributor License Agreement that would allow the project to relicense contributions later. Anyone contributing to Ken is contributing AGPL code, and the absence of a CLA is what makes the relicensing direction structurally one-way: even if a future maintainer wanted to relicense, they would need consent from every contributor whose code is in the tree, and the contributors are under no obligation to give it.

## Consequences

**Easier:**
- Users of Ken — including the people whose machines run the agent — have the legal right to inspect every line of code that runs on their machines, and the legal right to obtain the source code from anyone who runs a Ken server they interact with. This is not a project policy that could be changed; it is a license obligation that travels with the code.
- The project's trust story has a legal floor. Even if the original maintainers stopped caring, even if a future maintainer tried to close the source, the AGPL terms would prevent it. The user's right to audit is not contingent on the goodwill of any particular person.
- The AGPL is well-known in the self-hosted-tool ecosystem (Mastodon, Nextcloud, Plausible, MinIO before its relicensing, and many others use it). Family IT chiefs evaluating Ken alongside other self-hosted tools will recognize the license and what it means.
- Contributions are unambiguously AGPL. There is no licensing ambiguity for contributors to navigate, no CLA to sign, no question about whether their code might end up in a proprietary fork later.

**Harder:**
- AGPL is incompatible with several other license families. Ken cannot incorporate code under the Server Side Public License (SSPL), Common Clause license, or BUSL-licensed code, and Ken cannot be incorporated into projects with conflicting licenses. The dependency review (`cargo deny check`) is configured to enforce this.
- Some companies have internal policies against using AGPL software. This may limit Ken's adoption in corporate IT contexts. The project's stated audience — family IT chiefs — does not overlap meaningfully with corporate IT, so this is acceptable in practice, but it is worth knowing.
- Future relicensing options are gone. If years from now the project's needs change in a way that would benefit from a different license, the only path is "fork under a different name and rebuild". The current decision is deliberately irrevocable.

**Accepted:**
- We trade flexibility for durability. The project gives up the option to evolve its licensing in exchange for the guarantee that the licensing cannot be evolved against the user's interest. For a project whose value proposition depends on users trusting that it will not turn against them, this trade is the entire point.
- We accept that the AGPL's network-use clause (the AGPL section that distinguishes it from the GPL) places obligations on anyone who runs a modified Ken server and exposes it over a network. This is intentional. A family IT chief who modifies Ken for their own use and runs it on their own Pi for their own family is in a private context and the obligation is satisfied trivially. A hypothetical operator who runs a modified Ken as a service for other people is required to publish their modifications, which is exactly what the trust story demands.

## Alternatives considered

**A more permissive license like MIT or Apache-2.0.** Rejected because it would allow downstream users to distribute Ken without making the source available, which would undermine the trust story for any user of a downstream-distributed version. The whole point of the source-availability commitment is that it travels with the code. A permissive license breaks that travel.

**GPL-3.0 instead of AGPL-3.0.** Rejected because GPL's source-availability requirement triggers on *distribution*, not on *network use*. A hypothetical operator who runs a modified Ken server and exposes it over a network without distributing the binary would not trigger GPL's obligation. Ken's threat model includes exactly that scenario — a centralized Ken-as-a-service operator who closes their modifications — so the AGPL's network clause is essential, not optional.

**A custom source-available license (BUSL-style, with eventual conversion to a more permissive license).** Rejected because custom licenses have unpredictable legal interpretation and create friction with downstream users who already understand AGPL. The project gains no benefit from inventing its own license terms.

**Dual licensing (AGPL plus a commercial license sold by the maintainers).** Rejected because Ken does not have a commercial offering and does not intend to develop one. Dual licensing is a business model, and ADR-0001 already commits to "no central service operated by maintainers". The dual-license model assumes there is someone to negotiate with on the commercial side, and there is not.

**No license at all (public domain, CC0, or unlicensed).** Rejected because unlicensed code provides no legal clarity for users and no source-availability obligation for downstream redistributors. Public domain dedication varies in legal validity across jurisdictions. AGPL is the right tool for the commitment Ken is making.

## Notes

This ADR extracts a commitment that previously appeared as item T1-7 in ADR-0001. ADR-0001 will be amended in a small follow-up edit to remove T1-7 from its Tier 1 list and replace it with a cross-reference to this ADR. The amendment is an exception to the general rule that Accepted ADRs are immutable, because it removes content that is now recorded elsewhere rather than changing the substance of any decision. The substantive commitment — AGPL-3.0-or-later, no permissive relicensing — is unchanged; only its location in the ADR set has moved.

See the bundled `README.md` in this ADR delivery for the exact instructions on how to apply the ADR-0001 amendment.
