# axum, askama, htmx

Load this skill when working on the `ken-server` web layer. It covers how the server renders HTML, how htmx drives interactivity, and how to keep the UI small and coherent.

## The philosophy

The Ken web UI is a server-rendered HTML application with htmx for interactivity. There is no client-side framework, no bundler, no build pipeline outside `cargo build`. The UI is deliberately modest — it is an operator console for a single person managing a handful of endpoints, not a dashboard product.

The reason this stack works for Ken is that the number of distinct pages is small (on the order of ten), the interactivity is limited (forms, partial refreshes, a few modals), and the deployment context (Raspberry Pi, self-hosted) rewards simplicity over sophistication. Adding React or Svelte would multiply the build complexity for a UI that does not need their capabilities.

## Askama templates

Templates live in `crates/ken-server/templates/`. Each template is an `.html` file with askama's Jinja-like syntax. Templates are compiled into the binary at build time via the `#[derive(Template)]` macro on a Rust struct:

```rust
#[derive(Template)]
#[template(path = "endpoint_list.html")]
struct EndpointListTemplate<'a> {
    endpoints: &'a [EndpointSummary],
}
```

The struct fields are the template context. Askama enforces this at compile time — a template that references `{{ foo }}` where the struct has no `foo` field will not compile. This is the primary reason askama is preferred over runtime template engines: template errors are build errors.

## Template organization

Use a base template for the shared layout (header, nav, footer) and `{% extends "base.html" %}` in each page template. Keep the base template boring — no animations, no custom fonts beyond a single safe fallback, no third-party assets loaded from CDNs.

Use partial templates (stored with a leading underscore: `_endpoint_row.html`) for fragments that htmx will swap into the page. Partials extend nothing; they render exactly the fragment they represent.

Avoid template logic beyond simple iteration and conditionals. If a template wants to compute something, compute it in Rust first and pass the result in. Askama supports filters and control flow, but the cleaner pattern is to prepare the view model in the handler and let the template focus on structure.

## htmx conventions

htmx is loaded from a local static asset at `/static/htmx.min.js`. Do not load it from a CDN. The file is committed to `crates/ken-server/static/` and served via `tower-http`'s `ServeDir`.

Interactive elements use `hx-` attributes directly in the template:

```html
<button hx-post="/endpoints/{{ endpoint.id }}/session"
        hx-swap="outerHTML"
        hx-target="#session-button-{{ endpoint.id }}">
  Request Session
</button>
```

Key conventions:

- **Always set `hx-target` explicitly.** Default targeting is fragile; named targets are stable.
- **Use `hx-swap="outerHTML"`** for button state changes, `innerHTML` for content areas, `beforeend` for appending rows.
- **The handler returns an HTML fragment**, not a full page. The handler's return type is `Html<String>` or `impl IntoResponse` producing HTML.
- **Loading states use `hx-indicator`** pointing to a spinner element that is shown during the request. Include a spinner in the base template so every page has one available.
- **Error responses are HTML.** When a handler returns an error, it returns a rendered error fragment, not a JSON error object. htmx can swap error HTML into a target just as easily as success HTML.

## Handler patterns

A typical axum handler for an htmx-driven endpoint looks like this:

```rust
async fn endpoint_detail(
    State(state): State<AppState>,
    Path(id): Path<EndpointId>,
) -> Result<Html<String>, AppError> {
    let endpoint = state.db.get_endpoint(&id).await?;
    let template = EndpointDetailTemplate { endpoint: &endpoint };
    Ok(Html(template.render()?))
}
```

Patterns:

- **`State` extractor for shared app state.** The `AppState` is a struct with the database pool, config, and any other shared resources.
- **`Path` extractor for URL parameters.** Use strong types (`EndpointId`, not `String`) so parsing errors are caught at extraction time.
- **`Form` extractor for `<form>` submissions.** htmx posts forms the same way a regular browser does; the handler extracts via `Form<T>` where `T` is a serde struct.
- **Errors are a crate-local `AppError` enum** that implements `IntoResponse` to render an error fragment. Do not panic in handlers, do not `?` raw `anyhow::Error` without mapping it.
- **Handlers are small.** If a handler has more than twenty lines of logic, move the logic into a helper function or a dedicated module.

## Routing

Routes are defined in a single `routes.rs` module (or split into feature-area modules as the server grows). Keep routes flat and explicit:

```rust
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/endpoints", get(endpoint_list))
        .route("/endpoints/:id", get(endpoint_detail))
        .route("/endpoints/:id/session", post(request_session))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
}
```

Do not use the fancy route-builder macros. They save a few keystrokes and hide the route shape from grep.

## Static assets

All static assets live in `crates/ken-server/static/` and are served via `ServeDir`. This includes:

- `htmx.min.js` (committed, not fetched at build time)
- `tailwind.css` (pre-built, committed, not generated at build time)
- Any images, icons, or fonts the UI needs (minimize these)

Do not introduce a build step that generates CSS or JavaScript. The cost of the build pipeline is much higher than the benefit of incremental Tailwind generation for a UI of this size.

## Forms and CSRF

Form submissions go through standard HTML `<form>` elements or htmx `hx-post` attributes. CSRF protection uses a double-submit cookie pattern: the server sets a cookie with a random token on first page load, and every form includes a hidden field with the same token. The server verifies that the cookie and the form field match.

Every form is required to include the CSRF token. Missing tokens are a server-side 400, not a silent acceptance.

## What not to do

- **No single-page-application shell.** Every URL is a real URL that renders a real page.
- **No JSON API endpoints for the web UI.** The UI consumes HTML; JSON is for the agent-facing API only.
- **No client-side routing.** The browser's URL bar is the router.
- **No custom CSS framework.** Tailwind utility classes directly in templates.
- **No JavaScript beyond htmx.** If a feature feels like it needs custom JS, reconsider the feature.
- **No WebSockets for the UI.** Server-Sent Events are acceptable for simple live-update patterns (like "refresh this list when new data arrives"), but htmx polling (`hx-trigger="every 5s"`) is usually enough.
