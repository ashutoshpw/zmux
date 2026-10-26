# H01 authenticator and recovery flow

## HTTPS development tunnel cookie repair (2026-10-15)

The real authentication handler issued a session cookie without `Secure` when the
configured origin was HTTPS and `production` was false. A new table case in the
existing PostgreSQL integration suite failed on that exact response attribute
(expected true, received false), before the owner was changed. Existing tests
used HTTP loopback and could not detect the tunnel case; no production test seam
or replacement auth implementation was added.

Cookie policy now uses the validated public origin as well as production mode.
The same test passes HTTPS development, HTTPS production, and HTTP loopback,
checking the session cookie's Secure attribute/prefix, HttpOnly and SameSite.
`scripts/check-cloud` passes all 18 real PostgreSQL tests, TypeScript and the
production Docker image build. Evidence is retained in
`target/agent-validation/cloud-cookie-check-20261015.log`. The test database is a
separate disposable container; the serving database was not touched. Public
Cloudflare/browser acceptance and local service promotion remain separate gates.

Local promotion of `1507939` completed with the owned Docker workflow, including
the pre-migration backup. Health and readiness both returned200 at
`http://127.0.0.1:3530`; `bunx path-to-port` independently returned3530 after
startup. `https://zmux.w3dev.app/health` still failed DNS resolution from this
host. No public tunnel or browser acceptance is claimed.

Implement TOTP enrollment, confirmation, challenge and recovery using the pinned
Better Auth owner, including the web UI. Native authorization must remain denied
for a password-only challenge until a valid second factor creates a session.

Verification uses actual HTTP and disposable PostgreSQL, independently calculated
RFC 6238 codes and one-use recovery codes. Existing identity tests cover only
password sessions and cannot catch a challenge bypass. No auth/store substitute
or test-only production seam is needed. Prove the missing enrollment endpoint on
baseline, then validate enrollment, incorrect-code denial, native authorization
before/after challenge, and recovery consumption. Public tunnel and native browser
acceptance remain separate; this slice does not complete H01.

## API evidence

The original server returned 404 at the enrollment endpoint. After installing the
existing pinned plugin, the actual HTTP/PostgreSQL journey passes enrollment
password checks, unconfirmed setup, incorrect valid-length TOTP rejection,
confirmation, password-login challenge, native denial before verification, native
acceptance after TOTP, one-use recovery and password-gated disable. TOTP values are
computed independently using RFC6238 HMAC-SHA1 and the authenticator URI secret.
The test preserves real rate limits with two explicit 10.1s cooldowns and a 30s
budget for the resulting browser journey; auth guards are never disabled.

A separate regression reproduced readiness 200 with the MFA table missing. Readiness
now requires the new table and boolean user column; renaming either yields 503,
restoring both yields 200. Existing migration ownership adds the Better Auth schema
before server start; local restore verification also requires the MFA table.

All 17 service integration tests and strict TypeScript passed using a fresh private
PostgreSQL database that was dropped afterward. No production credentials or data
were used. The local running service is not yet promoted to this candidate. Web UI,
public tunnel and native external-browser acceptance remain separate gates.

## Browser and image evidence

The final page (`42f90ce744f3498ab7412bc0b4e84583ee447739b11292b0d4dcfb0ebd3d91d7`)
passes 15 actual Chrome journeys against Better Auth and disposable PostgreSQL 12.
They cover keyboard Enter signup, enrollment password/code errors, recovery-code
acknowledgement, confirmation and secret clearing, TOTP and recovery challenges,
return-path preservation and password-protected disable. Review found an unsubmitted
password survived sign-out until navigation. The exact prior page reproduced that
failure; a shared clear now empties password/challenge/enrollment fields before
navigation, observed using only a boolean from the actual beforeunload event.

The UI return-path check uses a deliberately incomplete native authorization query;
the API suite separately verifies real authorization parameters before and after
factor verification. Neither substitutes for the configured public callback journey.
No actual secret values or screenshots were retained in evidence. Direct delayed
response injection, broad keyboard traversal and VoiceOver are not established.
The final production Docker image builds and strict TypeScript checks pass.

Independent API receipt SHA256:
`e1262cc50cb92f0a82055bf7b2515f7bef32a60afb028d2895e10a2c5d7db10c`.
Combined final API/UI receipt SHA256:
`8ac4f45cd5cd20fc4c14d9b34747bb2b52b9abec4eb610f8dd1150e7fa2e6f5c`.
Parent isolated `scripts/check` also passes on the combined source snapshot;
native/public auth and full H01 remain open.

## Local runtime and hosted service acceptance

Cloud service checks run38821504830 passed on exact commit `0eb2198` using
PostgreSQL 18 and the production image build. The owned local Docker service was
backed up, migrated and rebuilt from the same commit. Parent verified all five
running auth/app/page/database/maintenance files against committed hashes.
Health and database readiness both returned HTTP200 at `http://127.0.0.1:3530`.

At 23:07UTC, a fresh 24,599-byte backup restored transactionally into an owned
new database: all 12 expected tables (including `twoFactor`), both versioned
migrations, and zero invalid constraints. The temporary database was removed;
the serving database was never replaced. Backup SHA256:
`c32feef89b859e2ad2089a65ec6adfe32909c1328bc654471dd2ab725949ed9f`.
The public hostname did not resolve from this workspace during verification;
Cloudflare Tunnel mapping and authenticated public/native callback remain open.
