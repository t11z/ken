# ADR-0009: egui as the technology choice for the agent tray app

- **Status:** Accepted
- **Date:** 2026-04-09
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

The Ken agent has two execution contexts on a Windows endpoint: the SYSTEM service that does the privileged work (reading OS state, mTLS-talking to the server, hosting the remote-session subsystem) and a user-mode tray application that handles everything the user sees and interacts with. The tray app's responsibilities are small but security-critical: display the consent dialog when the SYSTEM service receives a remote-session request, expose a status view, open the local audit log in the user's editor, and provide the kill-switch UI.

The tray app is a separate binary mode of `ken-agent.exe` (`ken-agent.exe tray`), launched by the service when it detects an interactive user session. It runs with the user's normal privileges, not as SYSTEM. It must work on every Windows version the agent supports, must not require additional runtime installation, and must produce a binary that fits inside the same MSI as the rest of the agent without significantly inflating it.

The realistic UI technology choices for a small Rust desktop app on Windows are: `egui`/`eframe` (immediate-mode pure-Rust UI), `tao` + `wry` (Rust wrappers around platform native windowing plus a webview), `windows-rs` directly with native Win32/WinUI APIs, or a Tauri-style framework that bundles a webview with a Rust backend.

## Decision

The Ken agent's tray application is built on **`egui`** with **`eframe`** as the framework wrapper. The system tray icon is provided by the `tray-icon` crate (or `egui`'s built-in tray support, whichever is more reliable on Windows at implementation time — this is an implementation detail, not an architectural commitment).

The tray app is a separate binary target inside the `ken-agent` crate. The `Cargo.toml` declares `[[bin]]` entries for both `ken-agent` (the service and CLI) and `ken-tray` (the tray UI), or a single binary that dispatches based on argv (`ken-agent.exe tray`). The dispatch approach is preferred because it produces one executable rather than two and simplifies the MSI.

The consent dialog is an `egui` modal: always-on-top, blocks interaction with other tray-app windows, has two clearly-labeled buttons (Allow / Deny in the user's language), and times out automatically after 60 seconds with a default of Deny. The result is communicated back to the SYSTEM service via the Named Pipe IPC defined in ADR-0010.

Other tray-app surfaces (status window, audit log opener, kill-switch confirmation) are also rendered with `egui`. The tray app does not embed a web view, does not load HTML, does not run JavaScript, and does not connect to any network endpoint of its own. Its entire interaction surface is the local user, the local audit log file, and the local Named Pipe to the service.

## Consequences

**Easier:**
- Pure Rust, no C++ dependencies, no webview engine to ship. The tray app's compile-time and binary-size cost is bounded and predictable. `egui` adds a few megabytes to the binary, not tens.
- The same Rust codebase as the rest of the agent. There is no FFI boundary, no separate UI language, no marshaling layer between the consent dialog and the IPC client.
- `egui` is actively maintained, well-documented, and widely used in production Rust desktop tools. The likelihood that the tray app's UI framework will need replacing in the next several years is low.
- Immediate-mode UI is conceptually simple: the tray app's main loop runs every frame, queries the current state, and renders the UI based on it. There is no retained widget tree to keep in sync with state, no event-handler graph to debug.
- Cross-platform potential exists. Although Ken's agent is Windows-only by ADR-0002, `egui` runs on macOS and Linux too. If a future ADR ever adds non-Windows agents, the tray-app code is the part that ports most easily.

**Harder:**
- `egui` is not a native Windows UI framework. Windows has its own design language (WinUI, Fluent), and an `egui` window does not blend in with native applications. For a tray app that the user sees rarely and briefly, this is acceptable. For an application the user lived in all day, it would matter more.
- System tray icons are a sore spot in every cross-platform UI framework, and `egui` is no exception. The implementation relies on third-party crates (`tray-icon` or similar) that have their own version-skew and Windows-version compatibility quirks. Expect some initial friction in tray-icon-specific code.
- Accessibility (screen readers, keyboard navigation) is weaker in `egui` than in native Win32 controls. The consent dialog and audit-log opener will get the basics (keyboard navigation, focus, button activation by Enter), but a user who relies on a screen reader will have a degraded experience. This is acknowledged and is a follow-up improvement, not a blocker for Phase 1.
- `egui`'s rendering uses GPU acceleration via wgpu by default. On a small fraction of Windows machines with broken or missing GPU drivers, the tray app may fall back to software rendering or fail to start. The fallback path needs to be tested.

**Accepted:**
- The tray app does not look "Windows-native". It looks like an `egui` application. For the consent dialog and status view, this is acceptable — what matters is clarity and correctness, not visual integration with the rest of the operating system. If a future user-research finding shows that family members reject the tray app because it looks alien, this ADR can be revisited.
- We commit to `egui` as a long-running dependency. Migrating to a different UI framework would touch the tray app's main loop, every dialog, and the rendering setup. This is a bounded migration but not a small one.

## Alternatives considered

**`tao` + `wry` with an HTML/CSS UI rendered in a webview.** Rejected because it ships a full webview engine (WebView2 on Windows, which is itself a runtime dependency) and inflates the binary substantially. It also reintroduces HTML and CSS as a second technology stack inside the agent, which contradicts the simplicity goal of a small native binary. The flexibility of an HTML UI is real but is not flexibility Ken needs.

**Native Win32 / WinUI via `windows-rs` directly.** Rejected because the verbosity of native Windows UI code in Rust is high and the maintenance cost is disproportionate to the size of Ken's UI. A consent dialog plus a status window plus a kill-switch confirmation does not justify the boilerplate of a hand-rolled WinUI application. `windows-rs` is the right choice for the agent's *non-UI* Windows interactions (services, WMI, Event Log, Named Pipes), and is used for those — but for the UI itself, `egui` is dramatically less code for the same outcome.

**A Tauri-based tray app with a Rust backend and an HTML/JS frontend.** Rejected for the same reasons as `tao` + `wry`: webview dependency, second technology stack, larger binary, and a build pipeline that wants to be a JavaScript project. Tauri is the right choice for desktop applications that want to feel like SaaS apps. Ken's tray app is the opposite of that.

**Slint, an alternative pure-Rust UI framework.** Rejected because Slint is younger, has a smaller community, and uses a custom DSL for layout that adds a learning step `egui` does not require. Slint may become the right answer in the future, but for Phase 1 the maturity gap matters more than the architectural elegance.
