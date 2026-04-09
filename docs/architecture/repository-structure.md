# Repository Structure

This document is a snapshot of Ken's intended directory layout. It is a reference for human contributors and LLMs alike. The tree shows where things go; the `CLAUDE.md` files at each level explain *why* things go there and what conventions apply.

If the actual repo deviates from this tree, one of them is wrong. Open an issue.

```
ken/
│
├── Cargo.toml                          Cargo workspace root
├── Cargo.lock                          committed (binary project, not a library)
├── rust-toolchain.toml                 pinned Rust version
├── .gitignore
├── .editorconfig
│
├── CLAUDE.md                           root rules of engagement (immutable to Claude Code)
├── README.md                           user-facing project description with emoji anchors
├── CONTRIBUTING.md                     how to contribute, with the upstream-thanks section
├── CODE_OF_CONDUCT.md                  standard community code of conduct
├── LICENSE                             AGPL-3.0
├── SECURITY.md                         responsible disclosure policy
│
├── crates/
│   ├── ken-protocol/
│   │   ├── CLAUDE.md
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── messages.rs
│   │   │   ├── state.rs
│   │   │   └── version.rs
│   │   └── tests/
│   │       ├── roundtrip.rs
│   │       └── snapshots/
│   │
│   ├── ken-agent/
│   │   ├── CLAUDE.md
│   │   ├── Cargo.toml
│   │   ├── build.rs                    embeds version info, manifest
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── bin/
│   │   │   │   ├── svc.rs
│   │   │   │   └── tray.rs
│   │   │   ├── service/
│   │   │   ├── tray/
│   │   │   ├── ipc/
│   │   │   ├── windows_state/
│   │   │   ├── session/
│   │   │   ├── audit/
│   │   │   └── updater/
│   │   ├── tests/
│   │   │   ├── consent_enforcement.rs
│   │   │   └── ipc_roundtrip.rs
│   │   └── installer/
│   │       └── ken.wxs                 WiX MSI definition (Phase 1.5+)
│   │
│   └── ken-server/
│       ├── CLAUDE.md
│       ├── Cargo.toml
│       ├── src/
│       │   ├── main.rs
│       │   ├── lib.rs
│       │   ├── config.rs
│       │   ├── api/
│       │   ├── web/
│       │   ├── relay/
│       │   ├── storage/
│       │   ├── tls/
│       │   └── templates/              .html files compiled by askama
│       ├── static/
│       │   ├── tailwind.css            precompiled, committed
│       │   ├── htmx.min.js             vendored
│       │   └── img/
│       ├── tests/
│       │   ├── api_integration.rs
│       │   ├── web_routes.rs
│       │   └── enrollment_flow.rs
│       └── docker/
│           ├── Dockerfile
│           └── compose.yml             reference deployment
│
├── docs/
│   ├── adr/                            Architecture Decision Records
│   │   ├── README.md                   index of ADRs
│   │   ├── 0000-adr-format-and-lifecycle.md
│   │   ├── 0001-what-ken-will-never-do.md
│   │   └── ...                         future ADRs
│   │
│   ├── architecture/
│   │   ├── overview.md                 high-level architecture narrative
│   │   ├── repository-structure.md     this file
│   │   ├── diagrams/                   excalidraw exports, mermaid sources
│   │   └── threat-model.md             documented threat model
│   │
│   └── user/
│       ├── install.md                  family IT chief install guide
│       ├── enrollment.md               adding endpoints
│       ├── consent.md                  what the consent dialog means
│       ├── audit-log.md                how to read your local audit log
│       └── uninstall.md                how to remove Ken cleanly
│
├── prompts/                            Claude Code prompt files
│   ├── README.md                       how prompts are organized
│   ├── phase-1/
│   │   ├── 001-bootstrap-workspace.md
│   │   ├── 002-protocol-skeleton.md
│   │   ├── 003-server-hello-world.md
│   │   └── ...
│   └── archive/                        completed prompts kept for reference
│
├── skills/                             in-repo Claude Code skills
│   ├── README.md                       skill index
│   ├── adr-writing/
│   │   └── SKILL.md
│   ├── rust-windows-service/
│   │   └── SKILL.md
│   ├── htmx-askama-patterns/
│   │   └── SKILL.md
│   ├── cargo-workspace-hygiene/
│   │   └── SKILL.md
│   ├── mtls-setup/
│   │   └── SKILL.md
│   └── rustdesk-crate-integration/
│       └── SKILL.md
│
└── .github/
    ├── workflows/
    │   ├── ci.yml                      build, test, lint on every PR
    │   ├── release.yml                 cuts releases on tag push
    │   ├── pages.yml                   builds and publishes the GitHub Pages site
    │   └── labels.yml                  syncs labels from labels config
    ├── ISSUE_TEMPLATE/
    │   ├── bug_report.md
    │   ├── feature_request.md          (filtered through ADR-0001)
    │   ├── security_report.md          points at SECURITY.md
    │   └── config.yml
    ├── PULL_REQUEST_TEMPLATE.md
    ├── labels.yml                      label definitions
    ├── CODEOWNERS
    └── dependabot.yml                  Cargo dependency updates
```

## Notes on placement decisions

A few choices in this tree are worth flagging because they were not obvious.

**`ken-server/static/` is committed, including the Tailwind CSS file.** This is deliberate. Committing the precompiled CSS means the build does not need Node.js or any JavaScript tooling. The Tailwind regeneration is documented as a developer task in the server crate's `CLAUDE.md`, run on demand when templates change in ways that introduce new utility classes. This trades a bit of friction during template edits for a much simpler build pipeline and a cleaner contribution story.

**`ken-server/src/templates/` lives inside `src/` rather than alongside it.** This is so that askama can find them via the standard `template = "..."` derive without path acrobatics. The templates are compiled into the binary, so they are source code in every meaningful sense.

**`docs/architecture/diagrams/` holds Excalidraw exports.** The architecture diagrams developed in the Claude Project sparring sessions are exported as `.excalidraw` JSON and as PNG, both committed. The PNG is what GitHub renders inline; the JSON is what someone re-edits.

**`prompts/archive/`.** Completed prompt files are not deleted. They are moved to the archive subdirectory and remain in git history. This preserves the trail of "what Claude Code was actually asked to do" alongside the trail of "what was committed."

**No top-level `tests/` directory.** Testing lives inside each crate's own `tests/` directory. There is no workspace-level integration test suite right now. If one becomes necessary, it gets its own crate (`ken-e2e` or similar), authorized by ADR.
