# ADR-0025: Self-hosted ARM64 runner for server container builds

- **Status:** Accepted
- **Date:** 2026-04-18
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

The Ken release pipeline (`.github/workflows/release.yml`) publishes two artifact
classes on every `v*.*.*` tag push: a signed MSI for the Windows agent, and a
multi-architecture container image for the server (amd64 and arm64). The MSI
build runs natively on a GitHub-hosted `windows-latest` runner. The server
container build runs on a GitHub-hosted `ubuntu-latest` runner and uses
`docker/setup-qemu-action` with `docker buildx` to produce both architectures
from one x86_64 host.

QEMU emulation for the `linux/arm64` container build is the slow leg of the
release pipeline. For a container that embeds a Rust workspace with heavy
dependencies (axum, sqlx, rustls, and associated build scripts), emulated builds
take noticeably longer than native builds and are a recurring operational
annoyance, not a correctness problem.

Oracle Cloud Infrastructure's Ampere A1 Free Tier offers a persistent ARM64
Linux VM at no cost. Using such a host as a GitHub Actions self-hosted runner
is a well-trodden path in the open-source Rust ecosystem and addresses the
QEMU slowness directly by replacing emulation with native ARM64 execution.

However, introducing a self-hosted runner is not a neutral operational change.
It shifts part of the build pipeline off the ephemeral, GitHub-administered
runner fleet and onto infrastructure under the sole control of the project
maintainer. This raises questions that a purely GitHub-hosted pipeline does
not have to answer:

- Which jobs are allowed to run on the self-hosted runner, and which are
  explicitly not?
- What happens when pull requests from forks are involved? A self-hosted
  runner executing untrusted fork code is a well-documented attack class.
- How is the runner's scope kept narrow over time, so that a later workflow
  edit does not silently migrate sensitive work (notably MSI signing) to it?

Ken's threat model already takes an explicit position on trust surfaces
(ADR-0001), and the Architect/Implementer separation (ADR-0007) requires that
architectural commitments be recorded, not embedded in workflow YAML. The
introduction of a self-hosted runner is an architectural commitment in that
sense, and this ADR records it.

This ADR does not address MSI signing infrastructure. The current MSI signing
is self-signed per build and ephemeral (the certificate is generated inside
the `windows-latest` runner and discarded with it), and ADR-0011 flags
durable OV or EV code-signing as future work. When durable signing material
is introduced, the question of which runner may see that material is a
distinct architectural decision and belongs in its own ADR.

## Decision

Ken permits the use of a self-hosted GitHub Actions runner for the
`build-server-image` job of the release workflow, and **only** for that job,
under the following constraints.

### Runner scope (allowlist)

The self-hosted runner is permitted to execute exactly one kind of job: the
native `linux/arm64` build of the `ken-server` container image, as part of the
multi-architecture server image release. All other workflow jobs continue to
run on GitHub-hosted runners. In particular:

- `build-agent-msi` remains on `windows-latest`.
- MSI signing, in its current or any future form, does not execute on a
  self-hosted runner.
- `github-release` (the release-publishing job) remains on a GitHub-hosted
  runner.
- The CI workflow (clippy, fmt, test, workspace build on pull requests and
  pushes to internal branches) does not use the self-hosted runner. It remains
  entirely on GitHub-hosted runners.

Any job outside this allowlist that wishes to run on a self-hosted runner
requires a new or superseding ADR.

### Trigger policy

The release workflow is triggered only by pushes of tags matching `v*.*.*`.
Tag pushes are restricted to repository collaborators with push access; they
cannot originate from fork pull requests. The self-hosted runner therefore
never sees code from a fork-authored pull request under the current workflow
topology.

As a defense-in-depth measure, workflows that run on the self-hosted runner
must not be configured with triggers that can be initiated by fork pull
requests. Specifically, `pull_request_target` is prohibited for any job
targeting the self-hosted runner, and `pull_request` is prohibited unless the
job is conditionally pinned to a GitHub-hosted runner when
`github.event.pull_request.head.repo.fork` is true.

### Runner labels

The self-hosted runner is registered with the label set
`[self-hosted, Linux, ARM64, ken-oci-ampere]`. The `ken-oci-ampere` label is
Ken-specific and allows workflows to target this runner unambiguously without
accidentally matching any other self-hosted runner that may exist in the
future under the same GitHub account. Workflow `runs-on` directives targeting
the self-hosted runner use the full label set, not just `self-hosted`.

### Secrets and credentials

The self-hosted runner is granted access to exactly the credentials required
for its one job:

- `GITHUB_TOKEN`, scoped per job by GitHub Actions, for authenticating to GHCR
  during the container push step.

The self-hosted runner does **not** receive:

- `CLAUDE_CODE_OAUTH_TOKEN`.
- `KENBOT_APP_PRIVATE_KEY` or `KENBOT_APP_ID`.
- Any current or future MSI code-signing material.
- Any credential unrelated to the allowlisted job.

This is enforced operationally by keeping the self-hosted runner out of any
workflow that would reference such secrets, not by runtime isolation on the
runner host.

### Runner host and operator

The runner host is an Oracle Cloud Infrastructure Ampere A1 Linux VM under the
sole control of the project maintainer. It is not a shared resource. SSH
access, operating system patching, and the lifecycle of the `actions/runner`
installation are the maintainer's responsibility. The runner runs under a
dedicated unprivileged system user without sudo rights, as a `systemd` service.

The operational details (user creation, runner installation, systemd unit,
token rotation, verification) live in `docs/maintainer/oci-runner-setup.md`
and are outside the scope of this ADR.

### Reversibility

If the self-hosted runner becomes unavailable, is suspected of compromise, or
is otherwise undesirable, the response is to revert the `build-server-image`
job's `runs-on` directive to `ubuntu-latest` with `setup-qemu-action`, which
is the pre-ADR state of the workflow. No other job in the release workflow
depends on the self-hosted runner, so this reversal is local and immediate.

## Consequences

**Easier:**
- Native ARM64 container builds replace QEMU emulation, reducing release
  pipeline wall-clock time.
- The runner's scope is recorded in an ADR rather than inferred from workflow
  YAML, making scope drift visible: any future change to `release.yml` that
  routes additional jobs to the self-hosted runner is an ADR-level change and
  must be reviewed as such.
- The separation between "what the self-hosted runner does" (build a container
  image) and "what remains on GitHub-hosted infrastructure" (MSI build, MSI
  signing, release publishing) is explicit. When MSI signing evolves toward a
  durable certificate, the question of where signing runs is a distinct ADR
  and cannot be silently answered by a workflow edit.
- Fork pull requests never reach the self-hosted runner under the current
  workflow topology, and the ADR's trigger policy prevents that from changing
  accidentally.

**Harder:**
- The maintainer now operates a small piece of infrastructure (one Linux VM)
  as part of the Ken release pipeline. Host security, OS updates, runner
  software updates, and monitoring are the maintainer's responsibility. If
  the runner is down, tagged releases will fail at the `build-server-image`
  step until the runner is restored or the workflow is reverted per the
  reversibility clause.
- The `ken-oci-ampere` label creates a naming convention that must be kept
  consistent between the workflow file and the runner registration. A typo
  in either place silently routes the job to a non-existent runner.
- A self-hosted runner with persistent disk retains build caches between
  jobs. This is intentional for speed but means any corruption or leakage in
  the Docker buildx cache persists across runs. The mitigation is the
  per-build isolation that `docker buildx` provides inside its own layer
  cache, not runner-level ephemerality.

**Accepted:**
- The project takes on direct operational responsibility for a build host.
  This is a meaningful step beyond "everything is GitHub-hosted and disposable"
  and must be documented in maintainer-facing docs so that a future
  co-maintainer can understand what exists, why, and how to operate it.
- The ADR intentionally does not generalize to other self-hosted runner use
  cases. If native ARM CI on pull requests, or a Linux test runner, or any
  other use of self-hosted infrastructure becomes desirable, it requires a
  new ADR (or a superseder of this one), not a workflow edit.
- The runner is under the personal control of one maintainer. In a future
  multi-maintainer scenario, governance questions (who may SSH to the host,
  who may reconfigure the runner, how is the runner's trust inherited when
  maintainers change) will need a separate governance ADR. This ADR does
  not pre-answer those questions.

## Alternatives considered

**Keep QEMU-based cross-architecture builds on GitHub-hosted runners.** This
is the status quo and the reversibility target. Rejected as the primary path
because the wall-clock cost of emulated ARM64 builds is real and recurring,
and a native ARM64 runner on free-tier OCI infrastructure eliminates it at
low operational cost. This option remains the fallback if the self-hosted
runner becomes unavailable.

**Use GitHub-hosted ARM runners.** GitHub now offers ARM-based hosted
runners, which would provide native ARM64 builds without self-hosting.
Rejected for the current phase because these runners are not part of the
free tier for this project's billing configuration, and because the free
OCI Ampere option is sufficient for Ken's release cadence. This option may
become preferable in the future if billing changes or if the operational
cost of the self-hosted runner grows; revisiting this decision would be a
superseding ADR.

**Use self-hosted runners broadly, including for CI on pull requests.**
Rejected for this ADR. Running self-hosted runners on `pull_request` events
(including from forks) is a well-known attack surface and would force the
ADR to take a position on fork-PR isolation, secret exposure during PR
runs, and build-cache poisoning between PRs. None of these questions arise
in the tag-only release-workflow scope, and none of them are needed to
solve the current problem. If native ARM CI on pull requests becomes
desirable, it is a separate decision and a separate ADR.

**Use self-hosted runners for MSI build or MSI signing.** Rejected
explicitly. The MSI build currently runs natively on `windows-latest` and
does not suffer from emulation overhead; there is no operational reason to
move it. The MSI signing path will, when Phase 2 signing infrastructure is
introduced, involve durable code-signing material whose exposure to a
self-hosted runner requires its own architectural decision. Bundling that
question into this ADR would conflate a straightforward performance
optimization with a significant trust-surface change.

**Run the runner host on the same hardware as the Ken server Raspberry
Pi.** Rejected because it co-locates build infrastructure with a
production-serving host, which violates the separation between "where Ken
is built" and "where Ken runs" that the current deployment story maintains
by default. It also introduces resource contention on a host that is
intentionally modest.
