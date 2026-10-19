import type { Pool, PoolClient } from "pg";
import { timingSafeEqual } from "node:crypto";
import { withTransaction } from "./db";
import type { ControlPlaneConfig } from "./config";
import { FixedWindowRateLimiter } from "./rate-limit";
import { hashOpaque, validDeviceName, validPlatform } from "./security";
import { requireNativeIdentity, type NativeIdentity } from "./native-identity";
import { clientAddress, enforceSameOrigin, exactString, HttpError, json, readJsonObject, type RequestContext } from "./http";

export const MAX_RELAY_BODY_BYTES = 384 * 1024;
const MAX_RELAY_ENVELOPE_CHARACTERS = 350_000;
const RELAY_TTL_SECONDS = 20;
const RELAY_RETAINED_SECONDS = 60;
const RELAY_MAX_ACTIVE_PER_DEVICE = 16;
const RELAY_MAX_ACTIVE_PER_ACCOUNT = 128;
const RELAY_MAX_RETAINED_PER_ACCOUNT = 512;
const DEVICE_ONLINE_SECONDS = 15;
const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

type NativeDeviceIdentity = NativeIdentity & {
  deviceId: string;
  publicKey: Buffer;
};
type RelayState = "pending" | "claimed" | "executing" | "completed" | "expired";

class FailedDeviceAuthLimiter {
  private readonly entries = new Map<string, { count: number; resetAt: number }>();

  constructor(private readonly limit = 60, private readonly windowMs = 60_000, private readonly maxKeys = 10_000) {}

  blocked(key: string, now = Date.now()): boolean {
    const entry = this.entries.get(key);
    if (!entry || entry.resetAt <= now) {
      this.entries.delete(key);
      return false;
    }
    return entry.count >= this.limit;
  }

  failure(key: string, now = Date.now()): void {
    const entry = this.entries.get(key);
    if (entry && entry.resetAt > now) {
      entry.count += 1;
      return;
    }
    if (this.entries.size >= this.maxKeys) {
      for (const [staleKey, stale] of this.entries) {
        if (stale.resetAt <= now) this.entries.delete(staleKey);
      }
      if (this.entries.size >= this.maxKeys) this.entries.delete(this.entries.keys().next().value ?? "");
    }
    this.entries.set(key, { count: 1, resetAt: now + this.windowMs });
  }

  success(key: string): void {
    this.entries.delete(key);
  }
}

function tooManyDeviceRequests(): HttpError {
  return new HttpError(429, "rate_limited", "too many requests for this device");
}

function canonicalBase64url(value: string, maxCharacters = MAX_RELAY_ENVELOPE_CHARACTERS): Buffer | null {
  if (!value.length || value.length > maxCharacters || !/^[A-Za-z0-9_-]+$/.test(value)) return null;
  const bytes = Buffer.from(value, "base64url");
  return bytes.toString("base64url") === value ? bytes : null;
}

function validUuid(value: string): boolean {
  return UUID_PATTERN.test(value);
}

function sameSecretHash(left: Buffer | null, right: Buffer): boolean {
  return left !== null && left.length === right.length && timingSafeEqual(left, right);
}

async function requireNativeDeviceIdentity(
  request: Request,
  pool: Pool,
  rateLimiter?: FixedWindowRateLimiter,
  rateKey?: string,
  failureLimiter?: FailedDeviceAuthLimiter,
  failureKey?: string,
): Promise<NativeDeviceIdentity> {
  if (failureLimiter && failureKey && failureLimiter.blocked(failureKey)) {
    throw new HttpError(429, "rate_limited", "too many authentication failures for this route");
  }
  try {
    const identity = await requireNativeIdentity(request, pool);
    const deviceId = request.headers.get("x-zmux-device-id") ?? "";
    const secret = request.headers.get("x-zmux-device-secret") ?? "";
    const secretBytes = canonicalBase64url(secret, 43);
    if (!validUuid(deviceId) || secret.length !== 43 || !secretBytes || secretBytes.length !== 32) {
      throw new HttpError(403, "invalid_device_proof", "a valid registered device proof is required");
    }
    const device = await pool.query<{ deviceId: string; userId: string; secretHash: Buffer | null; publicKey: Buffer | null }>(
      `SELECT id AS "deviceId", user_id AS "userId", device_secret_hash AS "secretHash", public_key AS "publicKey"
         FROM device
        WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL AND installation_id IS NOT NULL`,
      [deviceId, identity.userId],
    );
    const row = device.rows[0];
    if (!row || !sameSecretHash(row.secretHash, hashOpaque(secret)) || !row.publicKey || row.publicKey.length !== 32) {
      throw new HttpError(403, "invalid_device_proof", "a valid registered device proof is required");
    }
    if (failureLimiter && failureKey) failureLimiter.success(failureKey);
    if (rateLimiter && rateKey && !rateLimiter.allow(`${identity.userId}:${row.deviceId}:${rateKey}`)) {
      throw tooManyDeviceRequests();
    }
    return { ...identity, deviceId: row.deviceId, publicKey: row.publicKey };
  } catch (error) {
    if (error instanceof HttpError && (error.status === 401 || error.status === 403) && failureLimiter && failureKey) {
      failureLimiter.failure(failureKey);
    }
    throw error;
  }
}

export async function lockRelayAccount(client: PoolClient, userId: string): Promise<void> {
  await client.query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))", [`zmux-terminal-relay:${userId}`]);
}

async function registerDevice(body: Record<string, unknown>, identity: NativeIdentity, pool: Pool): Promise<Response> {
  const name = exactString(body, "name");
  const platform = exactString(body, "platform");
  const installationId = exactString(body, "installation_id");
  const deviceSecret = exactString(body, "device_secret");
  const publicKeyText = exactString(body, "public_key");
  const secretBytes = canonicalBase64url(deviceSecret, 43);
  const publicKey = canonicalBase64url(publicKeyText, 43);
  if (
    !validDeviceName(name) || !validPlatform(platform) || !validUuid(installationId) ||
    deviceSecret.length !== 43 || !secretBytes || secretBytes.length !== 32 ||
    publicKeyText.length !== 43 || !publicKey || publicKey.length !== 32
  ) {
    throw new HttpError(400, "invalid_device", "device identity, name, or platform is invalid");
  }

  const secretHash = hashOpaque(deviceSecret);
  const result = await withTransaction(pool, async (client) => {
    const inserted = await client.query<{
      id: string;
      installationId: string;
      lastSeenAt: Date;
      online: boolean;
      sharingEnabled: boolean;
      sharingEpoch: string | null;
    }>(
      `INSERT INTO device (id, user_id, installation_id, device_secret_hash, public_key, name, platform, last_seen_at)
       VALUES ($1, $2, $3, $4, $5, $6, $7, now())
       ON CONFLICT (user_id, installation_id) DO NOTHING
       RETURNING id, installation_id AS "installationId", last_seen_at AS "lastSeenAt",
                 last_seen_at > now() - make_interval(secs => $8) AS online,
                 sharing_enabled AS "sharingEnabled", sharing_epoch AS "sharingEpoch"`,
      [crypto.randomUUID(), identity.userId, installationId, secretHash, publicKey, name, platform, DEVICE_ONLINE_SECONDS],
    );
    const created = inserted.rows[0];
    if (created) return { ...created, created: true };

    const existing = await client.query<{
      id: string;
      installationId: string;
      secretHash: Buffer | null;
      publicKey: Buffer | null;
      revokedAt: Date | null;
    }>(
      `SELECT id, installation_id AS "installationId", device_secret_hash AS "secretHash",
              public_key AS "publicKey", revoked_at AS "revokedAt"
         FROM device WHERE user_id = $1 AND installation_id = $2 FOR UPDATE`,
      [identity.userId, installationId],
    );
    const row = existing.rows[0];
    if (!row) throw new Error("device registration conflict disappeared");
    if (row.revokedAt) throw new HttpError(409, "device_revoked", "this device registration has been revoked");
    if (!sameSecretHash(row.secretHash, secretHash) || !row.publicKey || !row.publicKey.equals(publicKey)) {
      throw new HttpError(409, "device_identity_conflict", "device identity proof or public key does not match");
    }
    const updated = await client.query<{
      lastSeenAt: Date;
      online: boolean;
      sharingEnabled: boolean;
      sharingEpoch: string | null;
    }>(
      `UPDATE device SET name = $3, platform = $4
        WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL
      RETURNING last_seen_at AS "lastSeenAt",
                last_seen_at > now() - make_interval(secs => $5) AS online,
                sharing_enabled AS "sharingEnabled", sharing_epoch AS "sharingEpoch"`,
      [row.id, identity.userId, name, platform, DEVICE_ONLINE_SECONDS],
    );
    if (!updated.rows[0]) throw new HttpError(409, "device_revoked", "this device registration has been revoked");
    return { id: row.id, installationId: row.installationId, ...updated.rows[0], created: false };
  });

  return json({
    id: result.id,
    installation_id: result.installationId,
    name,
    platform,
    public_key: publicKeyText,
    last_seen_at: result.lastSeenAt.toISOString(),
    online: result.online,
    sharing: result.online && result.sharingEnabled,
    sharing_epoch: result.sharingEnabled ? result.sharingEpoch : null,
  }, result.created ? 201 : 200);
}

async function setDevicePresence(
  identity: NativeDeviceIdentity,
  requestedDeviceId: string,
  body: Record<string, unknown>,
  pool: Pool,
): Promise<Response> {
  if (requestedDeviceId !== identity.deviceId) throw new HttpError(404, "device_not_found", "device was not found");
  const sharingValue = body.sharing;
  const epoch = typeof body.epoch === "string" ? body.epoch : "";
  if (typeof sharingValue !== "boolean" || !validUuid(epoch)) {
    throw new HttpError(400, "invalid_presence", "sharing must be a boolean and epoch must be a UUID");
  }

  const result = await withTransaction(pool, async (client) => {
    await lockRelayAccount(client, identity.userId);
    const current = await client.query<{
      sharingEnabled: boolean;
      sharingEpoch: string | null;
      revokedAt: Date | null;
      leaseActive: boolean;
    }>(
      `SELECT sharing_enabled AS "sharingEnabled", sharing_epoch AS "sharingEpoch", revoked_at AS "revokedAt",
              last_seen_at > now() - make_interval(secs => $3) AS "leaseActive"
         FROM device WHERE id = $1 AND user_id = $2 FOR UPDATE`,
      [identity.deviceId, identity.userId, DEVICE_ONLINE_SECONDS],
    );
    const device = current.rows[0];
    if (!device || device.revokedAt) throw new HttpError(404, "device_not_found", "device was not found");

    if (device.sharingEnabled && device.sharingEpoch !== epoch && (device.leaseActive || !sharingValue)) {
      throw new HttpError(409, "sharing_epoch_conflict", "sharing epoch does not match the active grant");
    }
    if (!device.sharingEnabled && sharingValue && device.sharingEpoch === epoch) {
      throw new HttpError(409, "sharing_epoch_conflict", "a new sharing grant requires a fresh epoch");
    }

    const updated = await client.query<{
      lastSeenAt: Date;
      online: boolean;
      sharingEnabled: boolean;
      sharingEpoch: string | null;
    }>(
      `UPDATE device
          SET last_seen_at = now(), sharing_enabled = $3,
              sharing_epoch = CASE WHEN $3 THEN $4::uuid ELSE sharing_epoch END
        WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL
      RETURNING last_seen_at AS "lastSeenAt",
                last_seen_at > now() - make_interval(secs => $5) AS online,
                sharing_enabled AS "sharingEnabled", sharing_epoch AS "sharingEpoch"`,
      [identity.deviceId, identity.userId, sharingValue, epoch, DEVICE_ONLINE_SECONDS],
    );
    if (!updated.rows[0]) throw new HttpError(404, "device_not_found", "device was not found");
    if (!sharingValue || device.sharingEpoch !== epoch) {
      await client.query(
        `UPDATE terminal_relay_request SET state = 'expired'
          WHERE user_id = $1 AND target_device_id = $2 AND state IN ('pending', 'claimed')`,
        [identity.userId, identity.deviceId],
      );
    }
    return updated.rows[0];
  });

  return json({
    id: identity.deviceId,
    last_seen_at: result.lastSeenAt.toISOString(),
    online: result.online,
    sharing: result.online && result.sharingEnabled,
    sharing_epoch: result.sharingEnabled ? result.sharingEpoch : null,
  });
}

async function expireUnavailableRelayRequests(client: PoolClient, userId: string, targetDeviceId?: string): Promise<void> {
  await client.query(
    `UPDATE terminal_relay_request AS request SET state = 'expired'
      WHERE request.user_id = $1 AND ($3::uuid IS NULL OR request.target_device_id = $3)
        AND request.state IN ('pending', 'claimed', 'executing')
        AND (
          request.expires_at <= now()
          OR (request.state IN ('pending', 'claimed') AND NOT EXISTS (
            SELECT 1 FROM device sender
             WHERE sender.id = request.sender_device_id AND sender.user_id = request.user_id
               AND sender.revoked_at IS NULL
          ))
          OR (request.state IN ('pending', 'claimed') AND NOT EXISTS (
            SELECT 1 FROM native_access_token token
              JOIN "session" auth_session ON auth_session.id = token.auth_session_id
             WHERE token.id = request.sender_token_id AND token.user_id = request.user_id
               AND token.revoked_at IS NULL AND token.expires_at > now()
               AND auth_session."expiresAt" > now() AND auth_session."userId" = token.user_id
          ))
          OR (request.state IN ('pending', 'claimed') AND NOT EXISTS (
            SELECT 1 FROM device target
             WHERE target.id = request.target_device_id AND target.user_id = request.user_id
               AND target.revoked_at IS NULL AND target.sharing_enabled
               AND target.sharing_epoch = request.target_epoch
               AND target.last_seen_at > now() - make_interval(secs => $2)
          ))
        )`,
    [userId, DEVICE_ONLINE_SECONDS, targetDeviceId ?? null],
  );
}

async function pruneRetainedRelayRequests(client: PoolClient, userId: string): Promise<void> {
  await client.query(
    `WITH old_requests AS (
       SELECT id FROM terminal_relay_request
        WHERE user_id = $1 AND state IN ('completed', 'expired')
          AND expires_at <= now() - make_interval(secs => $2)
        ORDER BY expires_at, id LIMIT 1000 FOR UPDATE SKIP LOCKED
     )
     DELETE FROM terminal_relay_request AS request USING old_requests
      WHERE request.id = old_requests.id`,
    [userId, RELAY_RETAINED_SECONDS],
  );
}

async function createRelayRequest(identity: NativeDeviceIdentity, body: Record<string, unknown>, pool: Pool): Promise<Response> {
  const requestId = exactString(body, "id");
  const targetDeviceId = exactString(body, "target_device_id");
  const payload = canonicalBase64url(exactString(body, "payload"));
  if (!validUuid(requestId) || !validUuid(targetDeviceId) || !payload || payload.length < 1 || identity.deviceId === targetDeviceId) {
    throw new HttpError(400, "invalid_relay_request", "relay request id, target, or encrypted payload is invalid");
  }

  const outcome = await withTransaction(pool, async (client) => {
    await lockRelayAccount(client, identity.userId);
    await expireUnavailableRelayRequests(client, identity.userId);
    const devices = await client.query<{
      id: string;
      userId: string;
      revokedAt: Date | null;
      sharingEnabled: boolean;
      sharingEpoch: string | null;
      online: boolean;
    }>(
      `SELECT id, user_id AS "userId", revoked_at AS "revokedAt", sharing_enabled AS "sharingEnabled",
              sharing_epoch AS "sharingEpoch", last_seen_at > now() - make_interval(secs => $2) AS online
         FROM device WHERE id = ANY($1::uuid[]) ORDER BY id FOR UPDATE`,
      [[identity.deviceId, targetDeviceId], DEVICE_ONLINE_SECONDS],
    );
    const sender = devices.rows.find((device) => device.id === identity.deviceId);
    const target = devices.rows.find((device) => device.id === targetDeviceId);
    if (!sender || sender.userId !== identity.userId || sender.revokedAt) return { kind: "sender_unavailable" as const };
    if (!target || target.userId !== identity.userId) return { kind: "target_not_found" as const };

    const existing = await client.query<{
      userId: string;
      senderDeviceId: string;
      senderTokenId: string | null;
      targetDeviceId: string;
      targetEpoch: string;
      payload: Buffer;
      state: RelayState;
      expiresAt: Date;
    }>(
      `SELECT user_id AS "userId", sender_device_id AS "senderDeviceId", sender_token_id AS "senderTokenId",
              target_device_id AS "targetDeviceId", target_epoch AS "targetEpoch", payload, state,
              expires_at AS "expiresAt"
         FROM terminal_relay_request WHERE id = $1 FOR UPDATE`,
      [requestId],
    );
    const previous = existing.rows[0];
    if (previous) {
      if (
        previous.userId !== identity.userId || previous.senderDeviceId !== identity.deviceId ||
        previous.senderTokenId !== identity.tokenId || previous.targetDeviceId !== targetDeviceId ||
        previous.targetEpoch !== target.sharingEpoch || !previous.payload.equals(payload)
      ) return { kind: "request_conflict" as const };
      return { kind: "existing" as const, id: requestId, state: previous.state, expiresAt: previous.expiresAt };
    }
    if (target.revokedAt || !target.sharingEnabled || !target.sharingEpoch || !target.online) {
      return { kind: "target_unavailable" as const };
    }

    await pruneRetainedRelayRequests(client, identity.userId);
    const retained = await client.query<{ count: number }>(
      "SELECT count(*)::int AS count FROM terminal_relay_request WHERE user_id = $1", [identity.userId],
    );
    if (retained.rows[0]!.count >= RELAY_MAX_RETAINED_PER_ACCOUNT) return { kind: "retained_capacity" as const };
    const active = await client.query<{ count: number }>(
      `SELECT count(*)::int AS count FROM terminal_relay_request
        WHERE user_id = $1 AND state IN ('pending', 'claimed', 'executing') AND expires_at > now()`,
      [identity.userId],
    );
    if (active.rows[0]!.count >= RELAY_MAX_ACTIVE_PER_ACCOUNT) return { kind: "active_capacity" as const };
    const perDevice = await client.query<{ deviceId: string; count: number }>(
      `SELECT reference.device_id AS "deviceId", count(*)::int AS count FROM (
           SELECT sender_device_id AS device_id FROM terminal_relay_request
            WHERE user_id = $1 AND state IN ('pending', 'claimed', 'executing') AND expires_at > now()
           UNION ALL
           SELECT target_device_id AS device_id FROM terminal_relay_request
            WHERE user_id = $1 AND state IN ('pending', 'claimed', 'executing') AND expires_at > now()
         ) AS reference
        WHERE reference.device_id = ANY($2::uuid[]) GROUP BY reference.device_id`,
      [identity.userId, [identity.deviceId, targetDeviceId]],
    );
    if (perDevice.rows.some((row) => row.count >= RELAY_MAX_ACTIVE_PER_DEVICE)) return { kind: "device_capacity" as const };

    const inserted = await client.query<{ expiresAt: Date }>(
      `INSERT INTO terminal_relay_request
         (id, user_id, sender_device_id, target_device_id, sender_token_id, target_epoch, payload, expires_at)
       VALUES ($1, $2, $3, $4, $5, $6, $7, now() + make_interval(secs => $8))
       RETURNING expires_at AS "expiresAt"`,
      [requestId, identity.userId, identity.deviceId, targetDeviceId, identity.tokenId, target.sharingEpoch, payload, RELAY_TTL_SECONDS],
    );
    return { kind: "created" as const, id: requestId, state: "pending" as const, expiresAt: inserted.rows[0]!.expiresAt };
  });

  if (outcome.kind === "sender_unavailable" || outcome.kind === "target_not_found") {
    throw new HttpError(404, "device_not_found", "device was not found");
  }
  if (outcome.kind === "target_unavailable") throw new HttpError(409, "target_unavailable", "target device is offline or not sharing");
  if (outcome.kind === "request_conflict") throw new HttpError(409, "request_id_conflict", "request id is already bound to another envelope");
  if (outcome.kind === "active_capacity" || outcome.kind === "device_capacity" || outcome.kind === "retained_capacity") {
    throw new HttpError(429, "relay_capacity", "terminal relay queue capacity is full");
  }
  if (outcome.kind === "created") return json({ id: outcome.id, state: outcome.state, expires_at: outcome.expiresAt.toISOString() }, 202);
  if (outcome.kind === "existing") return json({ id: outcome.id, state: outcome.state, expires_at: outcome.expiresAt.toISOString() }, 200);
  throw new Error("unhandled terminal relay outcome");
}

async function claimRelayRequest(identity: NativeDeviceIdentity, pool: Pool): Promise<Response> {
  const claimed = await withTransaction(pool, async (client) => {
    await lockRelayAccount(client, identity.userId);
    const device = await client.query<{ sharingEnabled: boolean; sharingEpoch: string | null; revokedAt: Date | null; online: boolean }>(
      `SELECT sharing_enabled AS "sharingEnabled", sharing_epoch AS "sharingEpoch", revoked_at AS "revokedAt",
              last_seen_at > now() - make_interval(secs => $3) AS online
         FROM device WHERE id = $1 AND user_id = $2 FOR UPDATE`,
      [identity.deviceId, identity.userId, DEVICE_ONLINE_SECONDS],
    );
    const target = device.rows[0];
    if (!target || target.revokedAt || !target.sharingEnabled || !target.sharingEpoch || !target.online) return null;

    await expireUnavailableRelayRequests(client, identity.userId, identity.deviceId);
    await client.query(
      `UPDATE terminal_relay_request SET state = 'expired'
        WHERE user_id = $1 AND target_device_id = $2 AND state IN ('pending', 'claimed')
          AND (expires_at <= now() OR target_epoch <> $3)`,
      [identity.userId, identity.deviceId, target.sharingEpoch],
    );
    const result = await client.query<{
      id: string;
      senderDeviceId: string;
      senderPublicKey: Buffer;
      payload: Buffer;
      expiresAt: Date;
      targetEpoch: string;
    }>(
      `WITH candidate AS (
         SELECT request.id FROM terminal_relay_request request
          WHERE request.user_id = $1 AND request.target_device_id = $2
            AND request.target_epoch = $3 AND request.state = 'pending' AND request.expires_at > now()
          ORDER BY request.created_at, request.id LIMIT 1 FOR UPDATE SKIP LOCKED
       )
       UPDATE terminal_relay_request AS request SET state = 'claimed', claimed_at = now()
         FROM candidate, device sender
        WHERE request.id = candidate.id AND sender.id = request.sender_device_id AND sender.user_id = $1
       RETURNING request.id, request.sender_device_id AS "senderDeviceId", sender.public_key AS "senderPublicKey",
                 request.payload, request.expires_at AS "expiresAt", request.target_epoch AS "targetEpoch"`,
      [identity.userId, identity.deviceId, target.sharingEpoch],
    );
    return result.rows[0] ?? null;
  });
  return json({ requests: claimed ? [{
    id: claimed.id,
    sender_device_id: claimed.senderDeviceId,
    sender_public_key: claimed.senderPublicKey.toString("base64url"),
    payload: claimed.payload.toString("base64url"),
    expires_at: claimed.expiresAt.toISOString(),
    target_epoch: claimed.targetEpoch,
  }] : [] });
}

async function beginRelayRequest(identity: NativeDeviceIdentity, requestId: string, pool: Pool): Promise<Response> {
  if (!validUuid(requestId)) throw new HttpError(404, "request_not_found", "terminal relay request was not found");
  const outcome = await withTransaction(pool, async (client) => {
    await lockRelayAccount(client, identity.userId);
    const request = await client.query<{
      state: RelayState;
      unexpired: boolean;
      targetEpoch: string;
      senderDeviceId: string;
      senderTokenId: string | null;
    }>(
      `SELECT state, expires_at > now() AS unexpired, target_epoch AS "targetEpoch",
              sender_device_id AS "senderDeviceId", sender_token_id AS "senderTokenId"
         FROM terminal_relay_request WHERE id = $1 AND user_id = $2 AND target_device_id = $3 FOR UPDATE`,
      [requestId, identity.userId, identity.deviceId],
    );
    const row = request.rows[0];
    if (!row) return "not_found" as const;
    if (row.state !== "claimed") return "invalid_state" as const;
    if (!row.unexpired) {
      await client.query("UPDATE terminal_relay_request SET state = 'expired' WHERE id = $1 AND state = 'claimed' AND expires_at <= now()", [requestId]);
      return "expired" as const;
    }

    const authorization = await client.query<{ valid: boolean }>(
      `SELECT (
         EXISTS (
           SELECT 1 FROM native_access_token token
             JOIN "session" auth_session ON auth_session.id = token.auth_session_id
            WHERE token.id = $1 AND token.user_id = $2 AND token.revoked_at IS NULL AND token.expires_at > now()
              AND auth_session."expiresAt" > now() AND auth_session."userId" = token.user_id
         )
         AND EXISTS (SELECT 1 FROM device sender WHERE sender.id = $3 AND sender.user_id = $2 AND sender.revoked_at IS NULL)
         AND EXISTS (
           SELECT 1 FROM device target
            WHERE target.id = $4 AND target.user_id = $2 AND target.revoked_at IS NULL
              AND target.sharing_enabled AND target.sharing_epoch = $5
              AND target.last_seen_at > now() - make_interval(secs => $6)
         )
       ) AS valid`,
      [row.senderTokenId, identity.userId, row.senderDeviceId, identity.deviceId, row.targetEpoch, DEVICE_ONLINE_SECONDS],
    );
    if (!authorization.rows[0]?.valid) {
      await client.query("UPDATE terminal_relay_request SET state = 'expired' WHERE id = $1 AND state = 'claimed'", [requestId]);
      return "authorization_denied" as const;
    }
    const started = await client.query(
      `UPDATE terminal_relay_request SET state = 'executing', started_at = now()
        WHERE id = $1 AND state = 'claimed' AND expires_at > now() RETURNING id`,
      [requestId],
    );
    return started.rowCount ? "executing" as const : "expired" as const;
  });

  if (outcome === "not_found") throw new HttpError(404, "request_not_found", "terminal relay request was not found");
  if (outcome === "expired" || outcome === "authorization_denied") throw new HttpError(409, "request_expired", "terminal relay request is no longer authorized");
  if (outcome === "invalid_state") throw new HttpError(409, "invalid_request_state", "terminal relay request is not awaiting execution");
  return json({ id: requestId, state: outcome });
}

async function completeRelayRequest(identity: NativeDeviceIdentity, requestId: string, body: Record<string, unknown>, pool: Pool): Promise<Response> {
  if (!validUuid(requestId)) throw new HttpError(404, "request_not_found", "terminal relay request was not found");
  const payload = canonicalBase64url(exactString(body, "payload"));
  if (!payload || payload.length < 1) throw new HttpError(400, "invalid_relay_response", "encrypted response payload is invalid");

  const outcome = await withTransaction(pool, async (client) => {
    await lockRelayAccount(client, identity.userId);
    const rowResult = await client.query<{
      state: RelayState;
      unexpired: boolean;
      targetEpoch: string;
      responsePayload: Buffer | null;
    }>(
      `SELECT state, expires_at > now() AS unexpired, target_epoch AS "targetEpoch", response_payload AS "responsePayload"
         FROM terminal_relay_request WHERE id = $1 AND user_id = $2 AND target_device_id = $3 FOR UPDATE`,
      [requestId, identity.userId, identity.deviceId],
    );
    const row = rowResult.rows[0];
    if (!row) return "not_found" as const;
    if (row.state === "completed") return row.responsePayload?.equals(payload) ? "already_completed" as const : "response_conflict" as const;
    if (row.state !== "executing") return "invalid_state" as const;
    if (!row.unexpired) {
      await client.query("UPDATE terminal_relay_request SET state = 'expired' WHERE id = $1 AND state = 'executing' AND expires_at <= now()", [requestId]);
      return "expired" as const;
    }
    const target = await client.query<{ valid: boolean }>(
      `SELECT EXISTS (
         SELECT 1 FROM device WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL AND sharing_enabled
           AND sharing_epoch = $3 AND last_seen_at > now() - make_interval(secs => $4)
       ) AS valid`,
      [identity.deviceId, identity.userId, row.targetEpoch, DEVICE_ONLINE_SECONDS],
    );
    if (!target.rows[0]?.valid) {
      await client.query("UPDATE terminal_relay_request SET state = 'expired' WHERE id = $1 AND state = 'executing'", [requestId]);
      return "grant_expired" as const;
    }
    const completed = await client.query(
      `UPDATE terminal_relay_request SET state = 'completed', response_payload = $2, completed_at = now()
        WHERE id = $1 AND state = 'executing' AND expires_at > now() RETURNING id`,
      [requestId, payload],
    );
    return completed.rowCount ? "completed" as const : "expired" as const;
  });

  if (outcome === "not_found") throw new HttpError(404, "request_not_found", "terminal relay request was not found");
  if (outcome === "expired" || outcome === "grant_expired") throw new HttpError(409, "request_expired", "terminal relay request is no longer authorized");
  if (outcome === "invalid_state") throw new HttpError(409, "invalid_request_state", "terminal relay request is not executing");
  if (outcome === "response_conflict") throw new HttpError(409, "response_already_set", "terminal relay response cannot be replaced");
  return new Response(null, { status: 204 });
}

async function readRelayRequest(identity: NativeDeviceIdentity, requestId: string, pool: Pool): Promise<Response> {
  if (!validUuid(requestId)) throw new HttpError(404, "request_not_found", "terminal relay request was not found");
  const result = await withTransaction(pool, async (client) => {
    const query = await client.query<{
      state: RelayState;
      expiresAt: Date;
      responsePayload: Buffer | null;
      senderTokenId: string | null;
      unexpired: boolean;
      senderTokenValid: boolean;
    }>(
      `SELECT state, expires_at AS "expiresAt", response_payload AS "responsePayload",
              sender_token_id AS "senderTokenId", expires_at > now() AS unexpired,
              EXISTS (
                SELECT 1 FROM native_access_token token
                  JOIN "session" auth_session ON auth_session.id = token.auth_session_id
                 WHERE token.id = terminal_relay_request.sender_token_id AND token.id = $4 AND token.user_id = $2
                   AND token.revoked_at IS NULL AND token.expires_at > now()
                   AND auth_session."expiresAt" > now() AND auth_session."userId" = token.user_id
              ) AS "senderTokenValid"
         FROM terminal_relay_request WHERE id = $1 AND user_id = $2 AND sender_device_id = $3 FOR UPDATE`,
      [requestId, identity.userId, identity.deviceId, identity.tokenId],
    );
    const row = query.rows[0];
    if (!row || row.senderTokenId !== identity.tokenId || !row.senderTokenValid) return null;
    if (row.state !== "completed" && row.state !== "expired" && !row.unexpired) {
      await client.query("UPDATE terminal_relay_request SET state = 'expired' WHERE id = $1 AND state = $2 AND expires_at <= now()", [requestId, row.state]);
      return { ...row, state: "expired" as const };
    }
    return row;
  });
  if (!result) throw new HttpError(404, "request_not_found", "terminal relay request was not found");
  return json({
    id: requestId,
    state: result.state,
    expires_at: result.expiresAt.toISOString(),
    ...(result.state === "completed" && result.responsePayload ? { payload: result.responsePayload.toString("base64url") } : {}),
  });
}

export async function terminalRelaySchemaReady(pool: Pool): Promise<boolean> {
  try {
    const result = await pool.query<{ ready: boolean }>(
      `SELECT EXISTS (SELECT 1 FROM zmux_schema_migration WHERE name = '003_device_terminal_relay.sql')
         AND EXISTS (
           SELECT 1 FROM information_schema.tables
            WHERE table_schema = current_schema() AND table_name = 'terminal_relay_request'
         )
         AND (SELECT count(*)::int = 5 FROM information_schema.columns
               WHERE table_schema = current_schema() AND table_name = 'device'
                 AND column_name = ANY($1::text[]))
         AND EXISTS (
           SELECT 1 FROM pg_indexes WHERE schemaname = current_schema() AND tablename = 'device'
             AND indexname = 'device_user_installation_unique_idx'
         ) AS ready`,
      [["installation_id", "device_secret_hash", "public_key", "sharing_enabled", "sharing_epoch"]],
    );
    return result.rows[0]?.ready === true;
  } catch {
    return false;
  }
}

export function isFrequentDeviceRoute(request: Request): boolean {
  const path = new URL(request.url).pathname;
  return (request.method === "POST" && /^\/v1\/devices\/([0-9a-f-]{36})\/presence$/.test(path)) ||
    (request.method === "GET" && path === "/v1/terminal-relay/inbox") ||
    (request.method === "POST" && path === "/v1/terminal-relay/requests") ||
    (request.method === "POST" && /^\/v1\/terminal-relay\/requests\/[^/]+\/(begin|response)$/.test(path)) ||
    (request.method === "GET" && /^\/v1\/terminal-relay\/requests\/[^/]+$/.test(path));
}

export function createDeviceTerminalApi(pool: Pool, config: ControlPlaneConfig) {
  const deviceRouteLimiter = new FixedWindowRateLimiter(2_000, 60_000);
  const failedDeviceAuthLimiter = new FailedDeviceAuthLimiter();

  async function authenticateDevice(request: Request, context: RequestContext, route: string): Promise<NativeDeviceIdentity> {
    const failureKey = `${clientAddress(request, context, config)}:${route}`;
    return requireNativeDeviceIdentity(request, pool, deviceRouteLimiter, route, failedDeviceAuthLimiter, failureKey);
  }

  async function handle(request: Request, context: RequestContext = {}): Promise<Response | null> {
    const path = new URL(request.url).pathname;
    const presencePath = /^\/v1\/devices\/([0-9a-f-]{36})\/presence$/.exec(path);
    const beginPath = /^\/v1\/terminal-relay\/requests\/([^/]+)\/begin$/.exec(path);
    const responsePath = /^\/v1\/terminal-relay\/requests\/([^/]+)\/response$/.exec(path);
    const statusPath = /^\/v1\/terminal-relay\/requests\/([^/]+)$/.exec(path);

    if (request.method === "GET" && path === "/v1/devices") {
      const identity = await requireNativeIdentity(request, pool);
      const devices = await pool.query<{
        id: string;
        name: string;
        platform: string;
        lastSeenAt: Date;
        revokedAt: Date | null;
        installationId: string | null;
        publicKey: Buffer | null;
        online: boolean;
        sharingEnabled: boolean;
        sharingEpoch: string | null;
      }>(
        `SELECT id, name, platform, last_seen_at AS "lastSeenAt", revoked_at AS "revokedAt",
                installation_id AS "installationId", public_key AS "publicKey",
                last_seen_at > now() - make_interval(secs => $2) AS online,
                sharing_enabled AS "sharingEnabled", sharing_epoch AS "sharingEpoch"
           FROM device WHERE user_id = $1 ORDER BY created_at DESC, id`,
        [identity.userId, DEVICE_ONLINE_SECONDS],
      );
      return json({
        devices: devices.rows.map((device) => ({
          id: device.id,
          name: device.name,
          platform: device.platform,
          last_seen_at: device.lastSeenAt.toISOString(),
          ...(device.installationId ? { installation_id: device.installationId } : {}),
          ...(device.publicKey ? { public_key: device.publicKey.toString("base64url") } : {}),
          online: device.online,
          sharing: device.online && device.sharingEnabled,
          sharing_epoch: device.sharingEnabled ? device.sharingEpoch : null,
          ...(device.revokedAt ? { revoked_at: device.revokedAt.toISOString() } : {}),
        })),
      });
    }

    if (request.method === "POST" && path === "/v1/devices") {
      enforceSameOrigin(request, config);
      const identity = await requireNativeIdentity(request, pool);
      return await registerDevice(await readJsonObject(request), identity, pool);
    }

    if (request.method === "POST" && presencePath) {
      enforceSameOrigin(request, config);
      const identity = await authenticateDevice(request, context, "presence");
      return await setDevicePresence(identity, presencePath[1]!, await readJsonObject(request), pool);
    }

    if (request.method === "GET" && path === "/v1/terminal-relay/inbox") {
      enforceSameOrigin(request, config);
      const identity = await authenticateDevice(request, context, "inbox");
      return await claimRelayRequest(identity, pool);
    }

    if (request.method === "POST" && path === "/v1/terminal-relay/requests") {
      enforceSameOrigin(request, config);
      const identity = await authenticateDevice(request, context, "request");
      return await createRelayRequest(identity, await readJsonObject(request, MAX_RELAY_BODY_BYTES), pool);
    }

    if (request.method === "POST" && beginPath) {
      enforceSameOrigin(request, config);
      const identity = await authenticateDevice(request, context, "begin");
      return await beginRelayRequest(identity, beginPath[1]!, pool);
    }

    if (request.method === "POST" && responsePath) {
      enforceSameOrigin(request, config);
      const identity = await authenticateDevice(request, context, "response");
      return await completeRelayRequest(identity, responsePath[1]!, await readJsonObject(request, MAX_RELAY_BODY_BYTES), pool);
    }

    if (request.method === "GET" && statusPath) {
      const identity = await authenticateDevice(request, context, "status");
      return await readRelayRequest(identity, statusPath[1]!, pool);
    }

    const heartbeat = /^\/v1\/devices\/([0-9a-f-]{36})\/heartbeat$/.exec(path);
    if (request.method === "POST" && heartbeat) {
      enforceSameOrigin(request, config);
      const identity = await requireNativeIdentity(request, pool);
      const result = await pool.query<{ last_seen_at: Date }>(
        `UPDATE device SET last_seen_at = now()
          WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL AND installation_id IS NULL
        RETURNING last_seen_at`,
        [heartbeat[1], identity.userId],
      );
      if (!result.rowCount) throw new HttpError(404, "device_not_found", "device was not found");
      return json({ id: heartbeat[1], last_seen_at: result.rows[0]!.last_seen_at.toISOString() });
    }

    const devicePath = /^\/v1\/devices\/([0-9a-f-]{36})$/.exec(path);
    if (request.method === "DELETE" && devicePath) {
      enforceSameOrigin(request, config);
      const identity = await requireNativeIdentity(request, pool);
      await withTransaction(pool, async (client) => {
        await lockRelayAccount(client, identity.userId);
        const result = await client.query(
          `UPDATE device
              SET revoked_at = COALESCE(revoked_at, now()), sharing_enabled = false
            WHERE id = $1 AND user_id = $2`,
          [devicePath[1], identity.userId],
        );
        if (!result.rowCount) throw new HttpError(404, "device_not_found", "device was not found");
        await client.query(
          `UPDATE terminal_relay_request
              SET state = 'expired'
            WHERE user_id = $1 AND state IN ('pending', 'claimed')
              AND (sender_device_id = $2 OR target_device_id = $2)`,
          [identity.userId, devicePath[1]],
        );
      });
      return new Response(null, { status: 204 });
    }

    return null;
  }

  return { handle, isFrequentRoute: isFrequentDeviceRoute };
}

export async function expirePendingRelayRequestsForToken(client: PoolClient, userId: string, tokenId: string): Promise<void> {
  await client.query(
    `UPDATE terminal_relay_request SET state = 'expired'
      WHERE user_id = $1 AND sender_token_id = $2 AND state IN ('pending', 'claimed')`,
    [userId, tokenId],
  );
}
