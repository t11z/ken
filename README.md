[![ci](https://github.com/t11z/ken/actions/workflows/ci.yml/badge.svg)](https://github.com/t11z/ken/actions/workflows/ci.yml) [![Dependabot Updates](https://github.com/t11z/ken/actions/workflows/dependabot/dependabot-updates/badge.svg)](https://github.com/t11z/ken/actions/workflows/dependabot/dependabot-updates) [![release](https://github.com/t11z/ken/actions/workflows/workflow-release.yml/badge.svg)](https://github.com/t11z/ken/actions/workflows/workflow-release.yml)
# 🐕 Ken

*A quiet guardian for your family's PCs.*

Ken is a self-hosted, single-tenant observability and remote-access tool for families. One technically capable person — the *family IT chief* — runs a small server on a Raspberry Pi at home. The people they support install the Ken agent on their Windows PCs. The agent reports whether Windows Defender is running, whether updates are current, whether the firewall is up, and whether anything concerning has happened recently. When the family IT chief needs to actually help with something on a remote machine, the agent shows a single dialog on that machine asking *"may I let them in?"* — and only opens a remote-control session after an explicit yes.

That's it. That's the whole product.

## 🤔 Why Ken exists

If you are the person your family calls when their PC acts up, you know the problem. Commercial remote-management tools are built for IT departments, priced like it, and assume a trust model that does not fit a family. Consumer remote-support tools require you to talk your aunt through installing TeamViewer every time she needs help, and leave no trace of what was done. Nothing in the middle respects the actual shape of family IT: one knowledgeable person, a handful of relatives, a strong preference for self-hosting, and a non-negotiable commitment to not turning helpful software into surveillance.

Ken is the tool for that middle.

## 🎯 What Ken does

- 👀 **Observes passively.** Reads Windows Defender state, update status, firewall configuration, BitLocker state, and recent security events. Reports them to your server. Does not read files. Does not read browser history. Does not log keystrokes. Does not take screenshots unless you are in an active, consented remote session.
- 🙋 **Asks before it acts.** Every remote-control session starts with a dialog on the endpoint saying *"Thomas möchte deinen PC fernsteuern. Zugriff erlauben?"* — or whatever you, the family IT chief, are called. The user clicks yes or no. There is no bypass, no remembered consent, no silent mode.
- 📜 **Keeps an audit log the user can read.** Every action Ken takes on a PC is recorded in a local audit log that the user of that PC can open without asking anyone's permission. If Ken does something, the user can see it.
- 🔌 **Stays out of the way.** Runs as a small Windows service. Uses tiny amounts of memory and CPU. Does not pop up, does not nag, does not interrupt.
- 🏠 **Lives on your hardware.** No cloud service. No account. No telemetry to anyone. Your Raspberry Pi is the entire backend.

## 🛑 What Ken does not do

Ken is an observability and consent-gated remote-access tool. It is not an antivirus, not an endpoint detection and response product, not a mobile device manager, not a parental control suite. The complete list of things Ken refuses to do is recorded in [ADR-0001](docs/adr/0001-trust-boundaries-and-current-scope.md), which is binding on every version of Ken and every contribution. Read it if you are considering whether Ken fits your needs, and especially read it before opening a feature request.

The short version: Ken does not read user data, does not modify security settings, does not phone home to anyone, does not support multi-tenant hosting, and does not do anything on a remote PC without a per-session click on that PC.

## 🧱 Architecture at a glance

Ken has three components and they are all written in Rust:

- 🦀 **`ken-agent`** — the Windows binary that runs on each family PC. Includes a SYSTEM service (reads OS state, talks to the server), a user-mode Tray App (shows the consent dialog, offers the kill switch, opens the audit log), and an embedded remote-session subsystem built on the RustDesk crates for when you actually need to see a screen.
- 🦀 **`ken-server`** — the Linux binary that runs on your Raspberry Pi. Handles enrollment, status aggregation, command routing, and serves the web UI. Uses axum, sqlx with SQLite, rustls for mTLS, askama for server-rendered HTML, and htmx for interactivity. No build pipeline, no JavaScript framework, no cloud dependencies.
- 🦀 **`ken-protocol`** — the tiny crate that defines the wire format between the two. Both binaries depend on it; it depends on nothing except serde and a couple of timestamp helpers.

Everything else — MSI installer, GitHub Pages documentation, CI workflows, ADRs — is tooling around these three. The architectural drawings live in [`docs/architecture/`](docs/architecture/).

## 🚀 Quick start

> ⚠️ **Ken is in early development.** There are no releases yet. What follows describes the intended deployment model, not a working install path.

Once Ken has its first release, deployment will look roughly like this:

**On the Raspberry Pi:**

```bash
curl -fsSL https://t11z.github.io/ken/install.sh | sh
```

This pulls a Docker Compose file and the Ken server image, generates a root certificate authority for your deployment, and prints a one-time admin URL you use to log in for the first time.

**On each Windows PC:**

The family IT chief opens the admin web UI, adds a new endpoint, and gets a one-time enrollment link. They send that link to the family member. The family member clicks it, downloads the signed MSI, and runs it. The agent starts, enrolls itself against the server, and the endpoint appears in the admin dashboard.

Total hands-on time per endpoint: about two minutes.

## 📖 Documentation

- 🏛️ [Architecture Decision Records](docs/adr/) — the why behind every major choice
- 🗺️ [Repository Structure](docs/architecture/repository-structure.md) — where things live and why
- 🖼️ [Architecture Diagrams](docs/architecture/) — visual overview of how components fit together
- 👤 [User Documentation](docs/user/) — install guides, consent explainer, audit log reader

The full documentation is also published at [t11z.github.io/ken](https://t11z.github.io/ken).

## 💝 Built on the shoulders of giants

Ken would not exist without a long list of open-source projects it either depends on or borrows ideas from. Particular thanks go to:

- 🖥️ **[RustDesk](https://github.com/rustdesk/rustdesk)** — the remote-session subsystem in Ken uses RustDesk's protocol crates. RustDesk is an outstanding open-source remote desktop project, and Ken would have needed months of extra work without it. If you need a full-featured remote desktop tool in its own right, use RustDesk directly.
- 🔒 **[rustls](https://github.com/rustls/rustls)** — modern, safe TLS in pure Rust. The foundation of Ken's mTLS layer.
- 🕸️ **[axum](https://github.com/tokio-rs/axum)** and the [Tokio](https://github.com/tokio-rs/tokio) ecosystem — Ken's server is unthinkable without them.
- 🪟 **[windows-rs](https://github.com/microsoft/windows-rs)** — Microsoft's own Rust bindings for the Windows API. Made the agent possible.
- 📝 **[askama](https://github.com/djc/askama)** — compile-time checked templates in Rust. Exactly the right amount of magic.
- ⚡ **[htmx](https://htmx.org/)** — the library that made us realize we did not need a JavaScript framework.
- 🛡️ **[Wazuh](https://wazuh.com/)** and **[Velociraptor](https://docs.velociraptor.app/)** — not dependencies, but inspirations in how they treat observability. Ken is a much smaller tool for a much smaller audience, but it learned from looking at them.
- 🏡 **[Pi-hole](https://pi-hole.net/)** and **[Home Assistant](https://www.home-assistant.io/)** — the self-hosted-at-home ecosystem Ken wants to belong to. Their approach to documentation, installation, and community shaped how Ken is being built.

The full list of dependencies and their licenses is in the `Cargo.lock` file and in the generated attribution document published with each release.

## 🤝 Contributing

Ken welcomes contributions from people who share its trust model. If you have run into the family-IT problem yourself and Ken sounds like what you wished existed, you are exactly the kind of person this project is for.

Before opening an issue or a pull request, please read:

1. [ADR-0001](docs/adr/0001-trust-boundaries-and-current-scope.md) — what Ken will and will not do. A lot of reasonable-sounding feature requests are explicitly out of scope, and knowing why will save everyone time.
2. [CONTRIBUTING.md](CONTRIBUTING.md) — how to file issues, how to propose changes, and how the review process works.
3. [The repository structure document](docs/architecture/repository-structure.md) — where things live and why.

Good first contributions include: documentation improvements, additional ADR drafts for open questions, new tests, and bug fixes with clear reproductions. Larger changes should start as a discussion before any code is written.

## 📜 License

Ken is licensed under the **GNU Affero General Public License v3.0 or later**. See [LICENSE](LICENSE) for the full text.

AGPL-3.0 is a deliberate, load-bearing choice recorded in [ADR-0001 T1-7](docs/adr/0001-trust-boundaries-and-current-scope.md). Ken's trust story depends on every user being able to inspect every line of code running on their machines, and AGPL extends that guarantee to anyone who interacts with a Ken deployment over a network. Relicensing Ken under a more permissive license is forbidden by the ADR. If you want those capabilities in a different license, you will need to build them yourself under a different name.

## 🙋 Who is Ken named for?

Ken means *knowledge, perception, the range of what one can see* in English — *"beyond my ken"*. In Japanese, *見* (ken) means *to see*. The name captures what the tool is: a quiet observer who sees on your behalf, and who asks politely before doing anything more.

It is also short enough that your family will remember it.
