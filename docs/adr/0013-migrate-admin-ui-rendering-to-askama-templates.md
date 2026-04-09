# ADR-0013: Migrate admin UI rendering to askama templates

- **Status:** Accepted
- **Date:** 2026-04-09
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

ADR-0006 commits the Ken admin UI to a server-rendered HTML stack built on `axum`, `askama`, `htmx`, and Tailwind. The current code in `crates/ken-server/src/http/admin.rs` does not match that commitment: it builds HTML inline via `format!`-based helper functions (`render_login`, `render_page`, and per-handler `String::write!` calls), and the `crates/ken-server/templates/` directory does not exist. The deviation arose during the original Phase 1 implementation, when no ADR existed to anchor the rendering strategy and the implementer chose the simpler path.

The deviation is small in lines of code (one file, ~538 lines) but it has two real costs. First, the architectural story is internally inconsistent: ADR-0006 says askama, the code says `format!`, the skill `axum-askama-htmx` says askama, the sub-CLAUDE.md says askama. A new contributor reading any one of those would be misled by the others. Second, the inline approach does not provide HTML escaping by default. Every interpolation site in `admin.rs` that takes a value originating from the database (endpoint hostnames, display names, command reasons) is a potential XSS vector if the developer forgets to escape it. Askama's default-escape behavior eliminates that class of bugs at the language level.

The decision to resolve the inconsistency is forced now because it must be settled before the Phase 1 fix prompt is written. The fix prompt will either include a migration step or it will not, and it cannot do that until this ADR exists.

## Decision

The Ken admin UI uses **askama templates exclusively** for HTML rendering. All inline `format!`-based HTML construction in `crates/ken-server/src/http/admin.rs` is removed and replaced with template-based equivalents under `crates/ken-server/templates/`.

The templates directory is structured as follows:

- `base.html` — the shared layout, with header, navigation, content area, and footer. All full-page templates extend this via `{% extends "base.html" %}`.
- `login.html` — the login form, used by `GET /admin/login` and re-rendered with an error message on failed login attempts.
- `dashboard.html` — the endpoint list, the landing page after login.
- `endpoint_detail.html` — one endpoint's full status snapshot, recent heartbeats, pending commands, and recent audit events.
- `enroll.html` — the enrollment form (input) and the resulting one-time enrollment URL display (output).
- `audit.html` — the recent audit events table.
- `commands.html` — the command queue form for an endpoint.
- `_endpoint_row.html` — a partial template for one row in the dashboard's endpoint list. Used as the response body for htmx swaps when the dashboard polls for updates.
- `_status_badge.html` — a partial template for the colored status badge, reused across the dashboard and the endpoint detail view.

Each handler in `admin.rs` builds a context struct that derives `askama::Template` and points to the appropriate template file via the `#[template(path = "...")]` attribute. The handler returns `Html<String>` produced by `template.render()`, mapped through the crate's `AppError` for any rendering failure.

The integration crate is `askama` 0.12 (or current stable) with the `askama_axum` feature enabled, so that `IntoResponse` for templates works natively. Templates are compiled into the binary at build time; there is no template loading at runtime, no template directory deployed alongside the binary, no separate template watch mode in development.

HTML escaping is automatic. Askama escapes `{{ value }}` interpolations by default. The only places where raw HTML insertion is allowed are explicit `{{ value | safe }}` filter applications, and each such use is reviewed for correctness during the migration.

The base template includes the htmx script tag (`<script src="/static/htmx.min.js"></script>`) and the Tailwind stylesheet link (`<link rel="stylesheet" href="/static/tailwind.css">`). The static assets directory `crates/ken-server/static/` must contain `htmx.min.js` (currently missing — to be added during the Phase 1 fix work) and `tailwind.css` (currently present).

## Consequences

**Easier:**
- ADR-0006 stands as written. The architectural story across ADRs, skills, sub-CLAUDE.md, and code is internally consistent. A new contributor reading any one of those documents finds the same answer in the others.
- Compile-time template checking. A template that references a field absent from its context struct fails to build. The category of "I renamed the field but forgot to update the template" bugs disappears entirely. With inline `format!`, the same category exists as runtime errors at best, silent wrong output at worst.
- HTML escaping is the default. Every database-sourced value that flows into a template is escaped automatically. The XSS surface from a malicious display name, hostname, or command reason is closed at the language level rather than depending on developer vigilance at every interpolation site.
- Separation of concerns. Templates live in `.html` files, which means the structure of the markup is visible without scrolling through Rust handler code. Future iteration on the visual design touches the template files, not the handler logic.
- The skill `.claude/skills/axum-askama-htmx/SKILL.md` matches the code. The skill becomes a real reference document for future implementation work in the admin UI, not a description of an architecture that does not exist.
- htmx fragment handling is cleaner. Partial templates with leading underscores are a visible naming convention that says "this is an htmx swap target". With inline rendering, htmx fragments were just another `format!` call somewhere in a handler, indistinguishable from full-page rendering at a glance.

**Harder:**
- The migration is real refactoring work. The current `admin.rs` is 538 lines, all of which must change. Each handler grows a context struct, each piece of inline HTML moves to a template file, each template needs its own Tailwind class review to make sure the visual output is unchanged. The work is mechanical but bounded — estimated half a day for Claude Code with a clear prompt.
- One additional dependency. `askama` and `askama_axum` add a small amount to the build time and binary size. Marginal in absolute terms; worth naming.
- Template changes trigger Rust recompilation, because askama compiles templates at build time. Iteration on visual design is slower than it would be with a runtime template engine. For a ten-page admin UI used by one person, this is acceptable.

**Accepted:**
- We commit to askama as a long-running dependency. Migrating away in the future would touch every handler and every template, which is a bounded but non-trivial cost. ADR-0006's broader commitment to a server-rendered HTML stack with htmx is what makes askama the right choice; the two ADRs stand or fall together.
- The Phase 1 fix prompt will include a migration step that covers this work. The fix prompt's success criterion includes "no `format!`-based HTML strings remain in `crates/ken-server/src/http/admin.rs`" and "every admin handler returns a rendered askama template".

## Alternatives considered

**Option B: Accept inline rendering and supersede the askama part of ADR-0006.** Rejected because the HTML-escaping argument outweighs the cost savings. The inline approach requires the developer to remember to escape every interpolation that came from a database value, and the current code has multiple interpolation sites where the escaping would have to be added retroactively. Askama escapes by default, which is the right default for a security-positioned project even when the absolute XSS surface is small. The architectural-tidiness argument also pulls toward Option A: keeping ADR-0006 clean is better than partially superseding it, which would set a precedent for fragmenting ADRs that gets messy at scale.

**A third option: keep inline rendering but add an `escape_html` helper that every interpolation must use.** Rejected because it relies on developer discipline to call the helper at every site, with no compile-time enforcement. The first time someone forgets, the bug ships. Askama's default-escape removes the human reliability layer entirely, which is the correct shape for the protection.

**A fourth option: use `maud` (compile-time HTML macro) instead of askama.** Considered briefly. Rejected because `maud` puts the HTML structure inside Rust source files via a macro DSL, which keeps the "templates and handlers in the same file" friction that we are trying to escape. Askama's separation of `.html` files from handler code is part of the value, not just an implementation detail. `maud` is a defensible choice in its own right but it does not solve the file-organization problem we have.
