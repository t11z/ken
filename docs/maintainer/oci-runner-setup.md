# OCI Ampere runner setup

## Purpose

Ken's release pipeline builds a multi-architecture container image for
`ken-server` (amd64, arm64) on every `v*.*.*` tag push. The arm64 leg of this
build runs natively on a self-hosted GitHub Actions runner hosted on an Oracle
Cloud Infrastructure Ampere A1 Linux VM, instead of being emulated with QEMU
on a GitHub-hosted amd64 runner.

The architectural decision to use a self-hosted runner, the scope it is
permitted to cover, and the constraints on its use are recorded in
[ADR-0025](../adr/0025-self-hosted-arm64-runner-for-server-container-builds.md).
This document covers the operational setup of the runner host and assumes
ADR-0025 has been read.

## Prerequisites

- An Oracle Cloud Infrastructure account with a free-tier Ampere A1 Linux VM
  running Ubuntu 22.04 or later, reachable via SSH.
- GitHub admin rights on `t11z/ken` (needed to generate the runner
  registration token).
- The VM's initial user (typically `ubuntu`) with sudo rights for the
  one-time setup steps.

## Step 1 — Prepare the host

Connect to the VM as the default user and install Docker and the build
dependencies the runner will need:

```bash
sudo apt-get update
sudo apt-get upgrade -y
sudo apt-get install -y ca-certificates curl gnupg lsb-release
```

Install Docker Engine following the current upstream instructions
(<https://docs.docker.com/engine/install/ubuntu/>). After installation,
verify Docker works without sudo for members of the `docker` group:

```bash
sudo usermod -aG docker $USER
newgrp docker
docker run --rm hello-world
```

## Step 2 — Create the runner user

The runner must not execute as a user with sudo rights. Create a dedicated
unprivileged user whose sole purpose is to run the GitHub Actions runner and
invoke Docker:

```bash
sudo useradd --create-home --shell /bin/bash ken-runner
sudo usermod -aG docker ken-runner
```

Confirm the user has no sudo rights:

```bash
sudo -l -U ken-runner
```

The expected output is that the user is not allowed to run sudo commands.

## Step 3 — Download and configure the runner

Switch to the runner user and download the `actions/runner` software:

```bash
sudo -iu ken-runner
mkdir -p ~/actions-runner && cd ~/actions-runner

# Fetch the latest runner release URL from
# https://github.com/actions/runner/releases — pick the linux-arm64 asset.
# Example (replace VERSION with the current release):
VERSION=2.320.0
curl -o actions-runner-linux-arm64.tar.gz -L \
  https://github.com/actions/runner/releases/download/v${VERSION}/actions-runner-linux-arm64-${VERSION}.tar.gz
tar xzf actions-runner-linux-arm64.tar.gz
rm actions-runner-linux-arm64.tar.gz
```

Generate a runner registration token by navigating in a browser to
`t11z/ken` → **Settings** → **Actions** → **Runners** → **New self-hosted
runner**. The token is short-lived (one hour) and single-use.

Register the runner, pinning the labels ADR-0025 prescribes:

```bash
./config.sh \
  --url https://github.com/t11z/ken \
  --token <REGISTRATION_TOKEN> \
  --name ken-oci-ampere \
  --labels self-hosted,Linux,ARM64,ken-oci-ampere \
  --work _work \
  --unattended \
  --replace
```

The label set must match ADR-0025 §Runner labels exactly. A typo here
silently routes the `build-server-image` job to a non-existent runner, and
the release workflow will hang on `build-server-image` until the labels are
corrected or the workflow reverted per ADR-0025 §Reversibility.

Exit back to the sudo-capable user for the next step:

```bash
exit
```

## Step 4 — Install the runner as a systemd service

From the sudo-capable user, install the runner as a systemd service so it
starts automatically on boot and is restarted if it crashes:

```bash
cd /home/ken-runner/actions-runner
sudo ./svc.sh install ken-runner
sudo ./svc.sh start
sudo ./svc.sh status
```

`svc.sh` is provided by the `actions/runner` distribution. It creates a
systemd unit named `actions.runner.t11z-ken.ken-oci-ampere.service` and
starts it under the `ken-runner` user.

Confirm the service is active:

```bash
systemctl status actions.runner.t11z-ken.ken-oci-ampere.service
```

Confirm the runner appears as online in `t11z/ken` → **Settings** →
**Actions** → **Runners**. The runner should show the labels
`self-hosted`, `Linux`, `ARM64`, `ken-oci-ampere` and status **Idle**.

## Step 5 — Verify

With the runner online and idle, push a throwaway tag to trigger the
release workflow and confirm the `build-server-image` job lands on the
self-hosted runner:

1. Create a test tag locally: `git tag v0.0.0-runner-test && git push origin v0.0.0-runner-test`.
2. In the GitHub Actions UI, open the triggered `release` workflow run.
3. Confirm `build-server-image` is assigned to the `ken-oci-ampere` runner
   (visible in the job log as "Runner name: ken-oci-ampere").
4. Confirm the job completes successfully and pushes the multi-arch image
   to GHCR.
5. Delete the test tag and any resulting release/container tag.

If the test is successful, the runner is ready for real releases. If the
job fails to land on the self-hosted runner or fails during the build,
revert `release.yml` per ADR-0025 §Reversibility before diagnosing further.

## Operations

### Updating the runner software

GitHub releases new versions of `actions/runner` roughly monthly. The
runner auto-updates by default when a new version is available, so no
manual action is typically needed. If the auto-update is disabled or fails,
repeat Step 3's download and the `./config.sh --replace` registration with
a fresh token.

### Rotating the registration token

The token used in `./config.sh` is single-use and expires after an hour;
rotation is not applicable to the token itself. If the runner needs to be
re-registered (after an OS reinstall, for example), generate a fresh
registration token and re-run Step 3.

### Host updates and reboots

Apply OS updates monthly or when the VM reports security patches. A reboot
after updates is expected; the systemd service restarts the runner
automatically on boot. If a release is in flight during a reboot, the job
will fail and the release must be re-tagged.

### Decommissioning

If the runner is to be retired, stop and uninstall the service, remove the
runner from the repository, and delete the VM:

```bash
cd /home/ken-runner/actions-runner
sudo ./svc.sh stop
sudo ./svc.sh uninstall

# Generate a removal token in the GitHub UI at
# t11z/ken → Settings → Actions → Runners → Remove
sudo -iu ken-runner
cd ~/actions-runner
./config.sh remove --token <REMOVAL_TOKEN>
```

Then revert `release.yml` per ADR-0025 §Reversibility so that future
releases fall back to QEMU-based arm64 builds on GitHub-hosted runners.

## Troubleshooting

**The runner shows as offline in the GitHub UI.**
Check `systemctl status actions.runner.t11z-ken.ken-oci-ampere.service` on
the host. If the service is not running, start it. If it is running but
the runner still shows offline, check outbound connectivity from the host
to `github.com` (port 443) and restart the service.

**The `build-server-image` job hangs at "Waiting for a runner to pick up
this job".**
Most commonly caused by a label mismatch between the workflow file
(`runs-on:` directive) and the runner registration. Verify both match the
full label set from ADR-0025 §Runner labels.

**The job starts but fails during `docker buildx build`.**
Confirm the `ken-runner` user is in the `docker` group and can run `docker
run --rm hello-world` without sudo. Confirm disk space on the host is
adequate (`df -h` — buildx caches can grow).

**Docker buildx cache has grown large.**
Periodically prune the buildx cache to prevent disk exhaustion:

```bash
sudo -iu ken-runner
docker buildx prune --filter until=168h --force
```

Running this weekly via a cron job on the runner user is acceptable.
