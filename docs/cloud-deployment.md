# Cloud deployment

The owned production origin is **https://zmux.w3dev.app**. All production cloud
deployments must run through GitHub Actions. No hosting provider or server has been
selected, so the repository does not claim a deployed service at that origin.

The native app uses the validated origin in `config/deployment.json`. A running local
cloud service is useful for development and integration testing; it does not replace
production deployment.

## Available Actions workflows

- **Cloud checks** validates the service against disposable PostgreSQL, including native
  PKCE and authorization boundaries.
- **Cloud deployment package** is manually dispatched on `main`. It first calls the cloud
  checks, builds the container and uploads an immutable source-tagged image archive,
  checksum and intended-origin receipt. The receipt explicitly states `deployed: false`.

## Host-selection handoff

After the hosting discussion, extend the package workflow with a deployment job that:

1. Uses a protected `cloud-production` GitHub environment and provider-specific secrets.
2. Deploys the exact validated source/image, without mutable unverified image tags.
3. Injects PostgreSQL credentials and Better Auth secret from environment secrets.
4. Runs the service's migrations through the protected job before promotion.
5. Configures TLS and the owned origin without a fallback to another product's service.
6. Checks the live readiness endpoint and performs the authenticated native/browser journey.
7. Records image/source, origin, migration result and health evidence in the Actions receipt.

Host credentials, database provisioning, DNS/TLS setup and production mail configuration
remain decisions for that discussion. Do not run a deployment from a local shell or call
an external provider directly as a substitute for the Actions job.

Account/device APIs do not yet provide remote PTY transport, VMs or relay services. Their
availability must remain explicit in both the service readiness response and native UI.

## Remaining service operations

Before production rollout, add scheduled cleanup for expired authorization codes and
expired/revoked native tokens and device records, plus backups and monitoring. Native
registration currently updates `last_seen_at` once; periodic native heartbeats are a
separate device-presence feature. The account/device UI does not imply a live connection.
