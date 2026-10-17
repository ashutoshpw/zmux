import type { Pool } from "pg";
import { withTransaction } from "./db";
import type { ZmuxAuth } from "./auth";
import type { ControlPlaneConfig } from "./config";
import { APP_CSS, APP_JS, appPage } from "./page";
import { FixedWindowRateLimiter } from "./rate-limit";
import {
  NATIVE_CLIENT_ID,
  NATIVE_REDIRECT_URI,
  bearerToken,
  hashOpaque,
  randomOpaque,
  validDeviceName,
  validPkceChallenge,
  validPlatform,
  validState,
  verifyPkce,
} from "./security";

const MAX_BODY_BYTES = 16 * 1024;
const AUTHORIZATION_CODE_TTL_SECONDS = 5 * 60;
const NATIVE_TOKEN_TTL_SECONDS = 30 * 24 * 60 * 60;
const REQUIRED_TABLES = [
  "user",
  "session",
  "account",
  "verification",
  "twoFactor",
  "organization",
  "member",
  "invitation",
  "native_authorization_code",
  "native_access_token",
  "device",
  "zmux_schema_migration",
];

class HttpError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

type RequestContext = { ip?: string | undefined };
type NativeIdentity = {
  tokenId: string;
  userId: string;
  name: string;
  email: string;
};

function json(data: unknown, status = 200, extraHeaders?: HeadersInit): Response {
  return Response.json(data, {
    status,
    headers: {
      "cache-control": "no-store",
      "x-content-type-options": "nosniff",
      ...extraHeaders,
    },
  });
}

function text(body: string, contentType: string): Response {
  return new Response(body, {
    headers: {
      "content-type": contentType,
      "cache-control": "no-store",
      "x-content-type-options": "nosniff",
      "content-security-policy": "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
      "referrer-policy": "no-referrer",
    },
  });
}

async function readJsonObject(request: Request): Promise<Record<string, unknown>> {
  if (!request.headers.get("content-type")?.toLowerCase().startsWith("application/json")) {
    throw new HttpError(415, "unsupported_media_type", "content-type must be application/json");
  }
  const declaredLength = Number(request.headers.get("content-length") ?? "0");
  if (Number.isFinite(declaredLength) && declaredLength > MAX_BODY_BYTES) {
    throw new HttpError(413, "payload_too_large", "request body exceeds 16 KiB");
  }
  const bytes = await request.arrayBuffer();
  if (bytes.byteLength > MAX_BODY_BYTES) throw new HttpError(413, "payload_too_large", "request body exceeds 16 KiB");
  let value: unknown;
  try {
    value = JSON.parse(new TextDecoder().decode(bytes));
  } catch {
    throw new HttpError(400, "invalid_json", "request body must be valid JSON");
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new HttpError(400, "invalid_request", "request body must be a JSON object");
  }
  return value as Record<string, unknown>;
}

function exactString(body: Record<string, unknown>, key: string): string {
  const value = body[key];
  if (typeof value !== "string") throw new HttpError(400, "invalid_request", `${key} must be a string`);
  return value;
}

function enforceSameOrigin(request: Request, config: ControlPlaneConfig): void {
  const origin = request.headers.get("origin");
  if (origin && origin !== config.origin) throw new HttpError(403, "origin_rejected", "request origin is not allowed");
}

function clientAddress(request: Request, context: RequestContext, config: ControlPlaneConfig): string {
  if (config.trustProxy) return request.headers.get("x-forwarded-for")?.split(",", 1)[0]?.trim() || context.ip || "unknown";
  if (context.ip) return context.ip;
  return "unknown";
}

async function requireNativeIdentity(request: Request, pool: Pool): Promise<NativeIdentity> {
  const token = bearerToken(request);
  if (!token) throw new HttpError(401, "invalid_token", "a valid bearer token is required");
  const result = await pool.query<NativeIdentity>(
    `SELECT token.id AS "tokenId", token.user_id AS "userId", account.name, account.email
       FROM native_access_token token
       JOIN "session" auth_session ON auth_session.id = token.auth_session_id
       JOIN "user" account ON account.id = token.user_id
      WHERE token.token_hash = $1
        AND token.revoked_at IS NULL
        AND token.expires_at > now()
        AND auth_session."expiresAt" > now()
        AND auth_session."userId" = token.user_id`,
    [hashOpaque(token)],
  );
  const identity = result.rows[0];
  if (!identity) throw new HttpError(401, "invalid_token", "a valid bearer token is required");
  return identity;
}

async function exchangeAuthorizationCode(body: Record<string, unknown>, pool: Pool): Promise<Response> {
  if (exactString(body, "grant_type") !== "authorization_code") {
    throw new HttpError(400, "unsupported_grant_type", "grant_type must be authorization_code");
  }
  const clientId = exactString(body, "client_id");
  const redirectUri = exactString(body, "redirect_uri");
  const code = exactString(body, "code");
  const verifier = exactString(body, "code_verifier");
  if (clientId !== NATIVE_CLIENT_ID || redirectUri !== NATIVE_REDIRECT_URI || !/^[A-Za-z0-9_-]{43}$/.test(code)) {
    throw new HttpError(400, "invalid_grant", "authorization code is invalid");
  }

  const response = await withTransaction(pool, async (client) => {
    const result = await client.query<{
      userId: string;
      authSessionId: string;
      clientId: string;
      redirectUri: string;
      challenge: string;
      expired: boolean;
      sessionActive: boolean;
    }>(
      `SELECT code.user_id AS "userId", code.auth_session_id AS "authSessionId",
              code.client_id AS "clientId", code.redirect_uri AS "redirectUri",
              code.code_challenge AS challenge, code.expires_at <= now() AS expired,
              (auth_session.id IS NOT NULL AND auth_session."expiresAt" > now()) AS "sessionActive"
         FROM native_authorization_code code
         LEFT JOIN "session" auth_session ON auth_session.id = code.auth_session_id
        WHERE code.code_hash = $1
        FOR UPDATE OF code`,
      [hashOpaque(code)],
    );
    const row = result.rows[0];
    if (!row || row.expired || !row.sessionActive || row.clientId !== clientId || row.redirectUri !== redirectUri || !verifyPkce(verifier, row.challenge)) {
      if (row?.expired || (row && !row.sessionActive)) await client.query("DELETE FROM native_authorization_code WHERE code_hash = $1", [hashOpaque(code)]);
      return null;
    }

    const accessToken = randomOpaque();
    await client.query("DELETE FROM native_authorization_code WHERE code_hash = $1", [hashOpaque(code)]);
    await client.query(
      `INSERT INTO native_access_token (id, token_hash, user_id, auth_session_id, expires_at)
       VALUES ($1, $2, $3, $4, now() + make_interval(secs => $5))`,
      [crypto.randomUUID(), hashOpaque(accessToken), row.userId, row.authSessionId, NATIVE_TOKEN_TTL_SECONDS],
    );
    return json({ access_token: accessToken, token_type: "Bearer", expires_in: NATIVE_TOKEN_TTL_SECONDS });
  });
  if (!response) throw new HttpError(400, "invalid_grant", "authorization code is invalid");
  return response;
}

async function createAuthorizationCode(request: Request, url: URL, auth: ZmuxAuth, pool: Pool, config: ControlPlaneConfig): Promise<Response> {
  const clientId = url.searchParams.get("client_id") ?? "";
  const redirectUri = url.searchParams.get("redirect_uri") ?? "";
  const responseType = url.searchParams.get("response_type") ?? "";
  const challenge = url.searchParams.get("code_challenge") ?? "";
  const challengeMethod = url.searchParams.get("code_challenge_method") ?? "";
  const state = url.searchParams.get("state") ?? "";
  if (
    clientId !== NATIVE_CLIENT_ID ||
    redirectUri !== NATIVE_REDIRECT_URI ||
    responseType !== "code" ||
    challengeMethod !== "S256" ||
    !validPkceChallenge(challenge) ||
    !validState(state)
  ) {
    throw new HttpError(400, "invalid_authorization_request", "native authorization parameters are invalid");
  }

  const current = await auth.api.getSession({ headers: request.headers });
  if (!current) {
    const login = new URL("/", config.origin);
    login.searchParams.set("return_to", `${url.pathname}${url.search}`);
    return Response.redirect(login, 302);
  }

  const code = randomOpaque();
  await pool.query(
    `INSERT INTO native_authorization_code
       (code_hash, user_id, auth_session_id, client_id, redirect_uri, code_challenge, expires_at)
     VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(secs => $7))`,
    [hashOpaque(code), current.user.id, current.session.id, clientId, redirectUri, challenge, AUTHORIZATION_CODE_TTL_SECONDS],
  );
  const callback = new URL(NATIVE_REDIRECT_URI);
  callback.searchParams.set("code", code);
  callback.searchParams.set("state", state);
  return Response.redirect(callback, 302);
}

export function createApp(pool: Pool, auth: ZmuxAuth, config: ControlPlaneConfig) {
  const nativeLimiter = new FixedWindowRateLimiter(60, 60_000);

  return async function fetch(request: Request, context: RequestContext = {}): Promise<Response> {
    const url = new URL(request.url);
    const path = url.pathname;
    try {
      if (path.startsWith("/api/auth/")) {
        if (request.body && Number(request.headers.get("content-length") ?? "0") > MAX_BODY_BYTES) {
          throw new HttpError(413, "payload_too_large", "request body exceeds 16 KiB");
        }
        const headers = new Headers(request.headers);
        headers.delete("x-zmux-client-ip");
        const address = clientAddress(request, context, config);
        if (address !== "unknown") headers.set("x-zmux-client-ip", address);
        return auth.handler(new Request(request, { headers }));
      }
      if (request.method === "GET" && path === "/") return text(appPage(), "text/html; charset=utf-8");
      if (request.method === "GET" && path === "/app.css") return text(APP_CSS, "text/css; charset=utf-8");
      if (request.method === "GET" && path === "/app.js") return text(APP_JS, "text/javascript; charset=utf-8");
      if (request.method === "GET" && ["/logo.png", "/apple-touch-icon.png", "/favicon.ico"].includes(path)) {
        return new Response(Bun.file(new URL(`../public${path}`, import.meta.url)), {
          headers: {
            "content-type": path.endsWith(".ico") ? "image/x-icon" : "image/png",
            "cache-control": "public, max-age=3600",
            "x-content-type-options": "nosniff",
          },
        });
      }
      if (request.method === "GET" && path === "/health") return json({ status: "ok" });
      if (request.method === "GET" && path === "/ready") {
        try {
          const schema = await pool.query<{ tableCount: number; migrationApplied: boolean; mfaColumn: boolean }>(
            `SELECT
               (SELECT count(*)::int
                  FROM information_schema.tables
                 WHERE table_schema = current_schema()
                   AND table_name = ANY($1::text[])) AS "tableCount",
               EXISTS (
                 SELECT 1 FROM zmux_schema_migration WHERE name = '001_native_identity.sql'
               ) AS "migrationApplied",
               EXISTS (
                 SELECT 1 FROM information_schema.columns
                 WHERE table_schema = current_schema() AND table_name = 'user'
                   AND column_name = 'twoFactorEnabled' AND data_type = 'boolean'
               ) AS "mfaColumn"`,
            [REQUIRED_TABLES],
          );
          const state = schema.rows[0];
          if (!state || state.tableCount !== REQUIRED_TABLES.length || !state.migrationApplied || !state.mfaColumn) {
            return json({ status: "unavailable", capabilities: { identity: "unavailable", devices: "unavailable" } }, 503);
          }
          return json({
            status: "ready",
            capabilities: { identity: "ready", devices: "ready", organizations: "ready", vm: "unavailable", relay: "unavailable", email: "unavailable" },
          });
        } catch {
          return json({ status: "unavailable", capabilities: { identity: "unavailable", devices: "unavailable" } }, 503);
        }
      }
      if (request.method === "GET" && path === "/v1/config") {
        return json({
          name: "zmux",
          cloud_origin: config.origin,
          native_oauth: {
            client_id: NATIVE_CLIENT_ID,
            redirect_uri: NATIVE_REDIRECT_URI,
            authorization_endpoint: `${config.origin}/v1/native/authorize`,
            token_endpoint: `${config.origin}/v1/native/token`,
            code_challenge_methods_supported: ["S256"],
          },
          capabilities: { identity: true, devices: true, organizations: true, vm: false, relay: false, email: false },
        });
      }

      if (path.startsWith("/v1/") && !nativeLimiter.allow(`${clientAddress(request, context, config)}:${path}`)) {
        return json({ error: "rate_limited", message: "too many requests" }, 429, { "retry-after": "60" });
      }

      if (request.method === "GET" && path === "/v1/native/authorize") {
        return await createAuthorizationCode(request, url, auth, pool, config);
      }
      if (request.method === "POST" && path === "/v1/native/token") {
        enforceSameOrigin(request, config);
        return await exchangeAuthorizationCode(await readJsonObject(request), pool);
      }

      if (request.method === "GET" && path === "/v1/me") {
        const identity = await requireNativeIdentity(request, pool);
        const memberships = await pool.query<{ id: string; name: string; slug: string; role: string }>(
          `SELECT organization.id, organization.name, organization.slug, member.role
             FROM member
             JOIN organization ON organization.id = member."organizationId"
            WHERE member."userId" = $1
            ORDER BY organization.name, organization.id`,
          [identity.userId],
        );
        return json({ user: { id: identity.userId, name: identity.name, email: identity.email }, organizations: memberships.rows });
      }

      if (request.method === "GET" && path === "/v1/devices") {
        const identity = await requireNativeIdentity(request, pool);
        const devices = await pool.query<{ id: string; name: string; platform: string; last_seen_at: Date; revoked_at: Date | null }>(
          `SELECT id, name, platform, last_seen_at, revoked_at
             FROM device WHERE user_id = $1 ORDER BY created_at DESC, id`,
          [identity.userId],
        );
        return json({
          devices: devices.rows.map((device) => ({
            id: device.id,
            name: device.name,
            platform: device.platform,
            last_seen_at: device.last_seen_at.toISOString(),
            ...(device.revoked_at ? { revoked_at: device.revoked_at.toISOString() } : {}),
          })),
        });
      }

      if (request.method === "POST" && path === "/v1/devices") {
        enforceSameOrigin(request, config);
        const identity = await requireNativeIdentity(request, pool);
        const body = await readJsonObject(request);
        const name = exactString(body, "name");
        const platform = exactString(body, "platform");
        if (!validDeviceName(name) || !validPlatform(platform)) {
          throw new HttpError(400, "invalid_device", "device name or platform is invalid");
        }
        const id = crypto.randomUUID();
        const result = await pool.query<{ last_seen_at: Date }>(
          `INSERT INTO device (id, user_id, name, platform, last_seen_at)
           VALUES ($1, $2, $3, $4, now()) RETURNING last_seen_at`,
          [id, identity.userId, name, platform],
        );
        return json({ id, name, platform, last_seen_at: result.rows[0]!.last_seen_at.toISOString() }, 201);
      }

      const heartbeat = /^\/v1\/devices\/([0-9a-f-]{36})\/heartbeat$/.exec(path);
      if (request.method === "POST" && heartbeat) {
        enforceSameOrigin(request, config);
        const identity = await requireNativeIdentity(request, pool);
        const result = await pool.query<{ last_seen_at: Date }>(
          `UPDATE device SET last_seen_at = now()
            WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL
          RETURNING last_seen_at`,
          [heartbeat[1], identity.userId],
        );
        if (!result.rowCount) throw new HttpError(404, "device_not_found", "device was not found");
        return json({ id: heartbeat[1], last_seen_at: result.rows[0]!.last_seen_at.toISOString() });
      }

      const device = /^\/v1\/devices\/([0-9a-f-]{36})$/.exec(path);
      if (request.method === "DELETE" && device) {
        enforceSameOrigin(request, config);
        const identity = await requireNativeIdentity(request, pool);
        const result = await pool.query(
          `UPDATE device SET revoked_at = COALESCE(revoked_at, now()) WHERE id = $1 AND user_id = $2`,
          [device[1], identity.userId],
        );
        if (!result.rowCount) throw new HttpError(404, "device_not_found", "device was not found");
        return new Response(null, { status: 204 });
      }

      if (request.method === "POST" && path === "/v1/native/revoke") {
        enforceSameOrigin(request, config);
        const identity = await requireNativeIdentity(request, pool);
        await withTransaction(pool, async (client) => {
          await client.query("UPDATE native_access_token SET revoked_at = COALESCE(revoked_at, now()) WHERE id = $1", [identity.tokenId]);
        });
        return new Response(null, { status: 204 });
      }

      return json({ error: "not_found", message: "endpoint was not found" }, 404);
    } catch (error) {
      if (error instanceof HttpError) return json({ error: error.code, message: error.message }, error.status);
      console.error("control-plane request failed", { method: request.method, path, error });
      return json({ error: "internal_error", message: "request could not be completed" }, 500);
    }
  };
}
