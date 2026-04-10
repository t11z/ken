# KenBot setup

## Purpose

KenBot is an automated triage assistant for issues in `t11z/ken`.
Its behavior, scope, and constraints are defined in [ADR-0015](../adr/0015-automated-issue-triage-with-kenbot.md).
This document covers only the one-time setup steps — not the design decisions, which live in the ADR.

## Prerequisites

- A Claude Max subscription (provides the OAuth token for Claude Code).
- GitHub admin rights on `t11z/ken`.
- An Anthropic OAuth token (generated in Step 4).

## Step 1 — Create the GitHub App

1. Go to **GitHub** → **Settings** → **Developer settings** → **GitHub Apps** → **New GitHub App**.
2. Fill in the following fields:
   - **Name:** `KenBot`
   - **Homepage URL:** `https://github.com/t11z/ken`
3. Under **Webhook**, **uncheck** "Active". KenBot reacts exclusively to workflow triggers in the same repo; no webhook is needed.
4. Set **Repository permissions**:
   - Contents: **Read and write**
   - Issues: **Read and write**
   - Pull requests: **Read and write**
   - Metadata: **Read-only**
5. No organization permissions are needed.
6. **Subscribe to events:** none.
7. Under **Where can this GitHub App be installed**, select **Only on this account**.
8. Click **Create GitHub App**.

## Step 2 — Generate and store the App's private key

1. On the App configuration page, scroll to **Private keys** and click **Generate a private key**.
2. Save the downloaded `.pem` file locally.
3. Go to `t11z/ken` → **Settings** → **Secrets and variables** → **Actions** → **New repository secret**.
   - **Name:** `KENBOT_APP_PRIVATE_KEY`
   - **Value:** the full contents of the `.pem` file, including the `-----BEGIN RSA PRIVATE KEY-----` and `-----END RSA PRIVATE KEY-----` lines.
4. Note the **App ID** displayed at the top of the App's configuration page (under "About").
5. Create another repository secret:
   - **Name:** `KENBOT_APP_ID`
   - **Value:** the numeric App ID.

## Step 3 — Install the App on the repository

1. On the App's page, click **Install App** in the left sidebar.
2. Select the `t11z` account.
3. Choose **Only select repositories** → select `ken`.
4. Click **Install**.

## Step 4 — Generate and store the Anthropic OAuth token

1. Generate a Claude Code OAuth token from the Anthropic console. [TODO: link to Anthropic docs]
2. Go to `t11z/ken` → **Settings** → **Secrets and variables** → **Actions** → **New repository secret**.
   - **Name:** `CLAUDE_CODE_OAUTH_TOKEN`
   - **Value:** the OAuth token string.

## Step 5 — Verify branch protection on main

1. Go to `t11z/ken` → **Settings** → **Branches** → **Branch protection rules** → `main`.
2. Confirm that **Restrict who can push to matching branches** is active and that the KenBot App is **not** in any bypass list.
3. Confirm that **Require a pull request before merging** is active.

## Step 6 — Verify the workflow runs

1. Create a test issue using any issue template (they all apply `status/needs-triage` automatically).
2. Go to the **Actions** tab and confirm the `kenbot` workflow starts.
3. Expected result: a structured analysis comment from `kenbot[bot]`, a status label transition (to `status/needs-discussion` or `status/in-progress`), and the `kenbot/analyzed` label applied.
4. If the workflow did not trigger, see Troubleshooting below.

## Troubleshooting

1. **Workflow does not trigger** — Confirm the App is installed on `t11z/ken` (Step 3). Verify the issue actually has `status/needs-triage`.
2. **"Resource not accessible by integration" error** — The App's permissions are too narrow. Revisit Step 1 and confirm Contents, Issues, Pull requests are set to Read and write.
3. **Branch protection blocks the App from pushing** — The App should push to `kenbot/*` branches only, never to `main`. If `main` branch protection blocks all pushes including to other branches, check that the branch protection rule targets only `main`, not `**`.
4. **Token expired or invalid** — The `CLAUDE_CODE_OAUTH_TOKEN` may have expired. Regenerate it (Step 4) and update the repository secret.
5. **Workflow runs but no comment appears** — Check the workflow logs. Most likely the `CLAUDE_CODE_OAUTH_TOKEN` secret is missing or empty, or the Claude Code action failed to authenticate.
6. **App token generation step fails** — Verify that `KENBOT_APP_ID` and `KENBOT_APP_PRIVATE_KEY` secrets are set correctly. The private key must include the full PEM content with header/footer lines.

## Retraction workflow

To retract a KenBot analysis (per ADR-0015):

1. Edit the original KenBot analysis comment to prepend a one-line retraction note (e.g., `**Retracted:** [reason]`).
2. Remove the `kenbot/analyzed` label from the issue.

Removing the label re-enables KenBot to re-analyze the issue on the next `status/needs-triage` event. The retraction note is preserved in the edited comment as part of the audit trail.
