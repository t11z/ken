# ADR-0006: Server-rendered HTML with axum, askama, htmx, and Tailwind

- **Status:** Accepted
- **Date:** 2026-04-09
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

The Ken server has an admin web UI for one user — the family IT chief — to monitor enrolled endpoints, queue commands, and view audit logs. The UI is small (about ten distinct pages), the interactivity is modest (forms, partial refreshes, occasional dialogs), and the deployment context is a Raspberry Pi running a single-binary Rust server. The audience is one technically capable person, not the general public, and the operational constraint is "should work for years without operator effort".

The realistic UI architecture choices fall on a spectrum from a single-page application (React, Vue, Svelte) talking to a JSON API, through a hybrid server-rendered approach with progressive enhancement (htmx, Hotwire, Unpoly), to a pure server-rendered classical application with full page loads on every action. Each point on this spectrum has different implications for the build pipeline, the codebase complexity, the binary size, and the network round-trip story.

The decision is forced now because the server's `admin.rs` already contains substantial UI code, and that code must commit to a rendering strategy. The current state of the repository has handlers that build HTML inline via `format!` strings rather than using a template engine, which is a deviation from the original `axum-askama-htmx` skill that requires this ADR to either ratify or correct.

## Decision

The Ken admin UI is a **server-rendered HTML application** built on:

- **`axum`** as the HTTP framework, with handlers returning `Html<String>` or `impl IntoResponse` producing HTML
- **`askama`** as the templating engine, with templates living in `crates/ken-server/templates/` and compiled into the binary at build time via `#[derive(Template)]`
- **`htmx`** as the client-side interactivity layer, loaded as a static asset from `/static/htmx.min.js`, used for partial page updates, form submissions, and live polling
- **Tailwind CSS** as the styling system, delivered as a pre-built static CSS file in `crates/ken-server/static/tailwind.css` — there is no Tailwind build pipeline, no PostCSS, no JavaScript build at all

There is no client-side application framework. There is no JSON API consumed by the browser. There is no bundler. The browser sees HTML, posts forms, and receives HTML fragments in response.

Templates use askama's compile-time checking: a template that references a field absent from its context struct is a build error. Partial templates for htmx swaps live in the same directory with a leading underscore (`_endpoint_row.html`). Forms are protected by a CSRF double-submit cookie pattern, with the token embedded as a hidden field.

The server's admin handlers do not produce JSON. Any data the UI needs is rendered to HTML on the server. The agent-facing API (a separate listener, see ADR-0004) serves JSON to the agent, not to the browser.

## Consequences

**Easier:**
- One language across the whole stack. The templates are checked by the same Rust compiler that builds the handlers. There is no JavaScript build to maintain, no `node_modules` to vendor, no transpiler version to pin.
- The binary is self-contained: askama compiles templates into the executable, htmx is a single static file under 50 KB, Tailwind is a static CSS file. Deployment is one binary plus a small static directory.
- Compile-time template checking catches a category of UI bugs at build time that would otherwise show up as runtime errors or, worse, as silently broken pages.
- htmx's interaction model — server returns HTML fragments, browser swaps them in — keeps the source of truth on the server. There is no client-side state to synchronize, no API contract to version, no two-environment debugging.
- The UI is keyboard-accessible and screen-reader-accessible by default, because it is real HTML produced by real form submissions and real anchor links. Modern SPA frameworks often have to work hard to recover what server-rendered HTML gets for free.

**Harder:**
- Complex client-side interactions (drag-and-drop, multi-step wizards with client-side validation, real-time visualizations) are awkward in this stack. Ken's admin UI does not need them, but if a future feature did, the current stack would be the wrong tool.
- htmx-specific patterns are not as widely known as React patterns. A future contributor unfamiliar with htmx will need to read the documentation. The skill `.claude/skills/axum-askama-htmx/SKILL.md` exists to shorten that ramp-up.
- The askama compile-time checking, while a win, also means that template changes trigger Rust recompilation. Iteration on visual design is slower than it would be with a runtime template engine. For Ken's small UI this is acceptable.

**Accepted:**
- The current code in `crates/ken-server/src/http/admin.rs` builds HTML inline via `format!` strings rather than via askama templates. This is a deviation from this ADR. The deviation is acknowledged as technical debt: a follow-up implementation prompt will move all admin HTML rendering into askama templates under `crates/ken-server/templates/`, and the existing inline rendering will be removed. The deviation exists because the original Phase 1 implementation prompt did not have an ADR to anchor the rendering strategy, and Claude Code chose the simpler path. This ADR resolves the ambiguity and authorizes the corrective work.
- We are foreclosing the option to evolve the admin UI into a richer client-side application without a stack change. If that need ever arises, it requires a new ADR that supersedes this one.

## Alternatives considered

**A React or Svelte single-page application talking to a JSON API.** Rejected because the operational complexity of a JavaScript build pipeline (Node.js version, package manager, bundler, transpiler, linter configuration) is disproportionate to a ten-page admin UI for a single user. Maintaining a JSON API alongside the agent's API doubles the wire-format surface and adds a versioning burden the project does not need. The benefits of an SPA — rich interactivity, offline support, smooth transitions — are not benefits Ken's admin UI needs.

**A pure server-rendered application with no client-side enhancement at all.** Rejected because some interactions in the admin UI (live-refreshing the endpoint list, submitting forms without full page reloads, showing modal dialogs) genuinely benefit from htmx's partial-update model. Going pure-classical would make the UI feel dated in ways that affect daily usability for the one person who uses it. htmx's incremental improvement over classical server-rendered HTML is exactly the right amount of client-side capability for Ken.

**Hotwire (Turbo + Stimulus) instead of htmx.** Rejected because Hotwire is more deeply tied to the Ruby on Rails ecosystem and assumes more about the server's response shapes. htmx is framework-agnostic, has a smaller surface area, and integrates more cleanly with axum handlers that produce raw HTML.

**A pre-built admin framework like Adminer or a Rust equivalent.** Rejected because no such framework matches Ken's specific data model (endpoints with heartbeats, command queues, audit events, mTLS-aware enrollment). Adapting a generic admin framework would be more work than writing the dozen handlers Ken actually needs.
