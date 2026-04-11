# ADR-0023: Clarify the verifier's call pattern and the dual-path certificate expiry check

- **Status:** Accepted
- **Date:** —
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

ADR-0008 specifies the custom `ClientCertVerifier` that backs the Ken agent listener. Two details of the verifier's implementation are correct in the code but absent from ADR-0008, creating an unexplained gap between the written architecture and the implementation:

**Gap 1 — `block_in_place` wrapper.** ADR-0008 says the verifier wraps its async `Storage` lookup in `tokio::runtime::Handle::current().block_on(...)`. The code adds an outer `tokio::task::block_in_place(...)` around that call. Without it, calling `block_on` from a thread that is already running inside a current-thread tokio runtime panics at runtime. `block_in_place` temporarily removes the current thread from the runtime's scheduler, making `block_on` safe to call from that thread. The wrapper is not stylistic; it is required for the code to function correctly in both the test environment (which uses `#[tokio::test]`, a current-thread runtime by default) and any production runtime configuration that uses current-thread scheduling.

**Gap 2 — dual-path expiry check.** ADR-0008 step 5 says the verifier "checks the endpoint's certificate `expires_at` field." This describes only one of two independent mechanisms that reject expired certificates. The custom verifier performs an explicit DB-side check: it reads the `expires_at` value stored in the endpoint's enrollment record and rejects the handshake if the current time is past it. Separately, the underlying `WebPkiClientVerifier` — which the custom verifier wraps, per ADR-0008 — independently rejects any certificate whose X.509 `notAfter` field is in the past. Both rejections happen before `Ok(ClientCertVerified::assertion())` is returned. The two mechanisms are not redundant in a trivial sense: the DB field and the certificate's `notAfter` can in principle diverge (e.g., if the DB record is updated independently of certificate re-issuance), and each path covers failure modes the other does not.

Neither gap requires a code change. The code is correct. This ADR exists to make the written architecture match what the code does.

## Decision

**`block_in_place` wrapper:** The verifier's async `Storage` lookup is wrapped in `tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(...))`. The outer `block_in_place` is a required part of the call pattern, not optional scaffolding. It exists because `block_on` panics if called from a thread that is currently scheduled inside a tokio runtime. `block_in_place` yields the thread back to the runtime temporarily, making `block_on` safe. Any future refactoring of the verifier must preserve this structure or substitute a functionally equivalent mechanism.

**Dual-path expiry check:** The verifier enforces certificate expiry through two independent mechanisms:

1. **DB-side check (custom verifier, step 5 of ADR-0008):** The verifier reads the `expires_at` field from the endpoint's enrollment record in the database and rejects the handshake if the current time is at or past that value. This is the *primary business-logic check*: it uses the expiry date Ken itself recorded when the certificate was enrolled, which is the authoritative value in Ken's trust model.

2. **Cryptographic `notAfter` check (`WebPkiClientVerifier`):** The underlying `WebPkiClientVerifier` independently rejects any certificate whose X.509 `notAfter` field is in the past. This runs as part of the chain verification pass that the custom verifier delegates to `WebPkiClientVerifier` before executing its own logic. This is a *defense-in-depth fallback*: it ensures that a certificate that is cryptographically expired is rejected even if the DB-side check were somehow bypassed or if the DB record's `expires_at` had drifted from the certificate's actual validity period.

A handshake succeeds only if both checks pass. A certificate is rejected by whichever path fires first; the two are not in sequence — `WebPkiClientVerifier` runs during chain validation, the DB check runs explicitly afterward.

## Consequences

**Easier:**
- A future reader comparing ADR-0008 and the code in `crates/ken-server/src/http/tls.rs` finds no unexplained divergence.
- The dual-path expiry design is explicitly on record as intentional defense-in-depth, not as an accidental overlap. This prevents a well-meaning future refactor from removing the `WebPkiClientVerifier` leg on the grounds that "the DB check already covers expiry."
- The `block_in_place` requirement is documented where future implementers will look for it: in the architecture, not only in a code comment.

**Harder:**
- Nothing becomes harder. This ADR adds no new requirements; it records existing ones.

**Accepted:**
- This ADR does not supersede ADR-0008. ADR-0008 remains the governing document for the verifier design. This ADR adds precision to two points ADR-0008 left implicit.

## Alternatives considered

**Treat both gaps as typo-class fixes and apply them directly to ADR-0008's body.** Rejected. ADR-0000 permits direct edits only for changes that "do not change meaning." Both gaps here change meaning: they make implicit behavior explicit and add a design rationale that is absent from ADR-0008. That is a substantive clarification, not a typo fix. Editing ADR-0008 in place would violate ADR-0000's immutability rule.

**Supersede ADR-0008 with a revised version that incorporates both clarifications.** Rejected. Supersession is the right mechanism when a prior ADR is *wrong* or *no longer reflects the chosen direction*. ADR-0008 is neither: it is correct as far as it goes, and the direction it describes is still in force. Superseding it to add two paragraphs would inflate the ADR count and obscure what actually changed. A targeted clarification ADR is the more honest form.

**Leave both gaps as code comments only.** Rejected. Code comments are not visible to someone reading the ADR set to reconstruct the architecture. The dual-path expiry design in particular has security relevance: the `WebPkiClientVerifier` leg is defense-in-depth, and that framing belongs in the architecture record, not buried in an inline comment.

## Notes

This ADR was written as part of the Phase 1 closure audit. It addresses W1.4 from the closure action plan.

The corresponding GitHub issue describes the deliverable as "one ADR document or an errata note, depending on what ADR-0000 permits." ADR-0000 does not provide an errata mechanism distinct from the ADR lifecycle itself; its only provision for substantive clarifications is a new ADR. This ADR is that mechanism.

See also: ADR-0008 (governing document), ADR-0017 (precedent for concretizing a single sentence from ADR-0008 without superseding it).
