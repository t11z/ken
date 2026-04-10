# ADR-0016: Agent identity derives from the client certificate only

- **Status:** Accepted
- **Date:** 2026-04-10
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

ADR-0004 and ADR-0008 together establish that every request on the agent listener arrives over a TLS connection whose client certificate has already been verified against the Ken root CA and the endpoint database. The verifier extracts the `EndpointId` from the certificate's Common Name as part of the handshake and propagates it into request handlers as a request extension. By the time a handler runs, the server already knows — cryptographically and authoritatively — which enrolled endpoint it is talking to.

The current `Heartbeat` message in `ken-protocol` nevertheless carries an `endpoint_id` field in its JSON body. This is a holdover from the pre-mTLS Phase 1 state, when the body was the only available identity signal. With the mTLS layer in place, the body field is at best redundant with the certificate-derived identity and at worst a second, weaker source of truth that invites confusion: a handler might compare the two, trust the wrong one, or silently accept a mismatch. Issue #8 surfaced exactly this confusion by framing the situation as a "cross-check" between two equal sources, which is the wrong mental model for a trust boundary that should have only one authoritative answer.

The decision is forced now because the agent listener's handlers are about to be wired up against the verifier's output, and the wire format of the heartbeat must be settled before that work begins. Leaving `endpoint_id` in the body and "ignoring" it in handlers is not a stable equilibrium — the next reader will either reintroduce a comparison or remove the field unilaterally.

## Decision

The `EndpointId` of an agent request is derived **exclusively from the verified client certificate**, never from the request body. The `endpoint_id` field is removed from the `Heartbeat` message and from any other agent-to-server message in `ken-protocol` that currently carries it. Handlers obtain the endpoint identity via `Extension<EndpointId>`, which is populated by the middleware described in ADR-0008. Any agent message that needs to refer to an endpoint refers to *the* endpoint — the one that owns the connection — implicitly.

This is an architectural rule, not just a code change. It means that no future agent message may reintroduce an `endpoint_id` field as a way of identifying the sender. If a future feature needs to refer to a *different* endpoint (for example, an admin-side message that names a target endpoint), that field is named to make the distinction obvious — `target_endpoint_id`, `peer_endpoint_id`, or similar — and it is never compared to the connection's own identity for authentication purposes.

The Ken root CA's CN-to-EndpointId mapping, the verifier's enrollment and revocation checks, and the middleware's request-extension propagation together form the single trust path. There is no second path.

## Consequences

**Easier:**
- The trust boundary becomes structurally simple: one source, one check, one place where the answer is computed. Handlers cannot accidentally trust the wrong field because there is no other field to trust.
- The `Heartbeat` message shrinks and its semantics become cleaner. The body describes the *state being reported*, not *who is reporting it*. The two concerns are separated at the wire level, which is where they belong.
- A whole class of bugs becomes unrepresentable: an agent cannot impersonate another agent by manipulating its body, because the body has nothing to manipulate. A compromised endpoint can still lie about its own state, but that is the unavoidable baseline of any reporting protocol and is the limit of what mTLS can promise.
- Tests of the heartbeat handler no longer need to construct matching cert/body pairs. Test fixtures construct a verified `EndpointId`, inject it as an extension, and exercise the handler against the body alone.

**Harder:**
- The `ken-protocol` wire format changes. Any agent built against the old shape will fail to compile against the new `Heartbeat` struct, and any in-flight serialized payloads (in tests, fixtures, recorded captures) become invalid. Phase 1 absorbs this without pain because there are no deployed agents and no committed format compatibility, but the change is real and must be made in one coordinated step across `ken-agent`, `ken-server`, and `ken-protocol`.
- A reader unfamiliar with the trust model may briefly wonder "where does the server learn which endpoint sent this?" when reading a heartbeat handler. The answer — the request extension, populated by the verifier — is one indirection away rather than visible in the message. A short comment at the top of each handler that extracts `Extension<EndpointId>` is the right place to surface this.

**Accepted:**
- We give up the defense-in-depth argument that a runtime body-vs-certificate comparison would catch a hypothetical middleware bug. The argument is real but the cost-benefit is wrong: the right place to verify that the middleware works is a test of the middleware, not a runtime check in every handler. If the middleware is broken, every agent request is broken, and a test catches that immediately. A runtime comparison would catch the same failure mode later, in production, at the cost of permanent protocol clutter.
- We commit to a stricter naming discipline for any future field that names an endpoint. The cost is small but real: reviewers must reject any new `endpoint_id` field on an agent-to-server message, and any field that names a *different* endpoint must be unambiguously named. This discipline lives in code review and in the protocol skill, not in tooling.

## Alternatives considered

**Keep `endpoint_id` in the body and verify on every request that it matches the certificate-derived identity.** Rejected because it institutionalizes the wrong mental model: that the body is a co-equal identity source which "happens to be checked." The check would catch nothing in normal operation — a correctly-functioning middleware makes the comparison vacuous, and a broken middleware makes every comparison wrong in the same way. The defense-in-depth framing is appealing but the defense is in the wrong layer. Tests of the verifier and the middleware are the correct place to ensure the trust path works; runtime comparisons in handlers are noise.

**Keep `endpoint_id` in the body and have handlers ignore it.** Rejected as the worst of the three options. It preserves the field as an attractive nuisance: future code may start trusting it, future readers will be confused about why it exists, and future protocol changes will inherit a field whose purpose is "do not use." A field that exists but must not be used is a documentation problem disguised as a data structure.

**Defer the decision and let handlers read whichever source they prefer.** Rejected because the resulting drift is exactly the failure mode this project's governance is designed to prevent. Two handlers making different choices about identity sourcing is not a tolerable steady state; it is a future ADR being written under duress.

## Notes

This ADR closes the architectural half of issue #8. The implementation half — wiring the verifier's output through the middleware into the handlers, and confirming that `axum-server` exposes peer-certificate information in the form ADR-0008 assumes — is a separate piece of work and may require a follow-up ADR if the assumed mechanism turns out to be unavailable in the current `axum-server` release.
