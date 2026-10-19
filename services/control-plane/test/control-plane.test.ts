import { afterAll, beforeAll, beforeEach, describe, expect, test } from "bun:test";
import { createHash, createHmac, randomBytes } from "node:crypto";
import { Pool } from "pg";
import { createApp } from "../src/app";
import { createAuth } from "../src/auth";
import { cleanupExpiredNativeCredentials } from "../src/maintenance";
import type { ControlPlaneConfig } from "../src/config";
import { MAX_RELAY_BODY_BYTES } from "../src/device-terminal";
import { MAX_BODY_BYTES } from "../src/http";

const origin = process.env.ZMUX_TEST_ORIGIN ?? "http://127.0.0.1:41992";
const databaseUrl = process.env.DATABASE_URL;
if (!databaseUrl) throw new Error("DATABASE_URL is required for control-plane integration tests");
const testApplicationName = "zmux-control-plane-integration-test";

const config: ControlPlaneConfig = {
  origin,
  databaseUrl,
  authSecret: process.env.BETTER_AUTH_SECRET ?? "test-only-secret-test-only-secret-1234",
  port: Number(new URL(origin).port),
  host: "127.0.0.1",
  production: false,
  trustProxy: false,
};

const pool = new Pool({ connectionString: databaseUrl, max: 10, application_name: testApplicationName });
let auth = createAuth(pool, config);
let app = createApp(pool, auth, config);
let server: ReturnType<typeof Bun.serve>;
let testClient = 1;
let appDispatchCount = 0;

type Session = { cookie: string; user: { id: string; email: string } };

function cookieFrom(response: Response): string {
  const value = response.headers.get("set-cookie");
  if (!value) throw new Error("response did not set a session cookie");
  return value.split(";", 1)[0]!;
}

function newDeviceRegistration(name = "Studio Mac") {
  return {
    name,
    platform: "macOS",
    installation_id: crypto.randomUUID(),
    device_secret: randomBytes(32).toString("base64url"),
    public_key: randomBytes(32).toString("base64url"),
  };
}

type DeviceRegistration = ReturnType<typeof newDeviceRegistration>;
type RegisteredDevice = { id: string; registration: DeviceRegistration };

function deviceHeaders(token: string, device: RegisteredDevice, jsonBody = false) {
  return {
    authorization: `Bearer ${token}`,
    "x-zmux-device-id": device.id,
    "x-zmux-device-secret": device.registration.device_secret,
    ...(jsonBody ? { "content-type": "application/json" } : {}),
  };
}

async function registerDevice(token: string, registration = newDeviceRegistration()): Promise<{ response: Response; device?: RegisteredDevice }> {
  const response = await fetch(`${origin}/v1/devices`, {
    method: "POST",
    headers: { authorization: `Bearer ${token}`, origin, "content-type": "application/json" },
    body: JSON.stringify(registration),
  });
  if (response.status !== 200 && response.status !== 201) return { response };
  const result = await response.clone().json() as { id: string };
  return { response, device: { id: result.id, registration } };
}

async function setPresence(token: string, device: RegisteredDevice, sharing: boolean, epoch: string): Promise<Response> {
  return fetch(`${origin}/v1/devices/${device.id}/presence`, {
    method: "POST",
    headers: { ...deviceHeaders(token, device, true), origin },
    body: JSON.stringify({ sharing, epoch }),
  });
}

async function waitForDatabaseLockWaiters(expected: number): Promise<void> {
  const deadline = Date.now() + 2_000;
  while (Date.now() < deadline) {
    const result = await pool.query<{ count: number }>(
      `SELECT count(*)::int AS count FROM pg_stat_activity
        WHERE application_name = $1 AND state = 'active' AND wait_event_type = 'Lock'
          AND pid <> pg_backend_pid()`,
      [testApplicationName],
    );
    if (result.rows[0]!.count >= expected) return;
    await Bun.sleep(10);
  }
  throw new Error(`control-plane database did not reach ${expected} concurrent lock waiters`);
}

async function signUp(email: string): Promise<Session> {
  const response = await fetch(`${origin}/api/auth/sign-up/email`, {
    method: "POST",
    headers: { "content-type": "application/json", origin, "x-test-client-ip": `192.0.2.${testClient++}` },
    body: JSON.stringify({ name: "Test User", email, password: "correct horse battery staple" }),
  });
  expect(response.status).toBe(200);
  const result = (await response.json()) as { user: { id: string; email: string } };
  return { cookie: cookieFrom(response), user: result.user };
}

async function authorize(cookie: string, verifier: string, state = "state-roundtrip"): Promise<{ code: string; callback: URL }> {
  const challenge = createHash("sha256").update(verifier).digest("base64url");
  const query = new URLSearchParams({
    client_id: "zmux-macos-dev",
    redirect_uri: "zmux-dev://oauth/callback",
    response_type: "code",
    code_challenge: challenge,
    code_challenge_method: "S256",
    state,
  });
  const response = await fetch(`${origin}/v1/native/authorize?${query}`, {
    headers: { cookie },
    redirect: "manual",
  });
  expect(response.status).toBe(302);
  const callback = new URL(response.headers.get("location")!);
  return { code: callback.searchParams.get("code")!, callback };
}

async function exchange(code: string, verifier: string): Promise<Response> {
  return fetch(`${origin}/v1/native/token`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      grant_type: "authorization_code",
      client_id: "zmux-macos-dev",
      redirect_uri: "zmux-dev://oauth/callback",
      code,
      code_verifier: verifier,
    }),
  });
}

async function nativeToken(cookie: string): Promise<string> {
  const verifier = "v".repeat(64);
  const { code } = await authorize(cookie, verifier);
  const response = await exchange(code, verifier);
  expect(response.status).toBe(200);
  const result = (await response.json()) as { access_token: string; token_type: string; expires_in: number };
  expect(result.token_type).toBe("Bearer");
  expect(result.expires_in).toBe(2_592_000);
  return result.access_token;
}

beforeAll(() => {
  server = Bun.serve({
    port: config.port,
    maxRequestBodySize: MAX_RELAY_BODY_BYTES,
    fetch: (request) => {
      appDispatchCount += 1;
      return app(request, { ip: request.headers.get("x-test-client-ip") ?? "127.0.0.1" });
    },
  });
});

beforeEach(async () => {
  await pool.query(
    `TRUNCATE device, native_access_token, native_authorization_code, invitation, member,
       organization, verification, "twoFactor", account, "session", "user" CASCADE`,
  );
  auth = createAuth(pool, config);
  app = createApp(pool, auth, config);
});

afterAll(async () => {
  await server.stop();
  await pool.end();
});

describe("native OAuth contract", () => {
  test("rejects streamed auth bodies over 16 KiB before calling Better Auth", async () => {
    let handlerCalls = 0;
    const authSpy = {
      handler: async () => {
        handlerCalls += 1;
        return new Response(null, { status: 204 });
      },
    } as unknown as typeof auth;
    const previousApp = app;
    app = createApp(pool, authSpy, config);

    try {
      const body = new TextEncoder().encode(JSON.stringify({ payload: "x".repeat(MAX_BODY_BYTES) }));
      expect(body.byteLength).toBeGreaterThan(MAX_BODY_BYTES);
      const bodyStream = new ReadableStream<Uint8Array>({
        start(controller) {
          for (let offset = 0; offset < body.byteLength; offset += 1_024) {
            controller.enqueue(body.slice(offset, offset + 1_024));
          }
          controller.close();
        },
      });
      const request = new Request(`${origin}/api/auth/sign-in/email`, {
        method: "POST",
        headers: { "content-type": "application/json", origin },
        body: bodyStream,
        duplex: "half",
      } as RequestInit);
      expect(request.headers.get("content-length")).toBeNull();

      const response = await fetch(request);
      expect(response.status).toBe(413);
      expect(await response.json()).toMatchObject({ error: "payload_too_large" });
      expect(handlerCalls).toBe(0);
    } finally {
      app = previousApp;
    }
  });

  test("session cookies follow the public HTTPS origin even behind a development tunnel", async () => {
    for (const [publicOrigin, production, secure] of [
      ["https://zmux.example.test", false, true],
      ["https://zmux.example.test", true, true],
      [origin, false, false],
    ] as const) {
      const publicConfig = { ...config, origin: publicOrigin, production };
      const publicApp = createApp(pool, createAuth(pool, publicConfig), publicConfig);
      const response = await publicApp(new Request(`${publicOrigin}/api/auth/sign-up/email`, {
        method: "POST",
        headers: { "content-type": "application/json", origin: publicOrigin },
        body: JSON.stringify({ name: "Cookie User", email: `cookie-${production}-${secure}@example.com`, password: "correct horse battery staple" }),
      }), { ip: "192.0.2.230" });
      expect(response.status).toBe(200);
      const cookies = response.headers.getSetCookie();
      const sessionCookie = cookies.find((cookie) => cookie.split("=", 1)[0]!.endsWith("better-auth.session_token"));
      expect(sessionCookie).toBeDefined();
      expect(/;\s*Secure(?:;|$)/i.test(sessionCookie!)).toBe(secure);
      expect(sessionCookie!.startsWith("__Secure-")).toBe(secure);
      expect(sessionCookie).toMatch(/;\s*HttpOnly(?:;|$)/i);
      expect(sessionCookie).toMatch(/;\s*SameSite=Lax(?:;|$)/i);
    }
  });

  test("requires browser auth and preserves state through an exact allowlisted callback", async () => {
    const verifier = "p".repeat(64);
    const challenge = createHash("sha256").update(verifier).digest("base64url");
    const query = new URLSearchParams({
      client_id: "zmux-macos-dev",
      redirect_uri: "zmux-dev://oauth/callback",
      response_type: "code",
      code_challenge: challenge,
      code_challenge_method: "S256",
      state: "opaque state / +",
    });
    const unauthenticated = await fetch(`${origin}/v1/native/authorize?${query}`, { redirect: "manual" });
    expect(unauthenticated.status).toBe(302);
    expect(new URL(unauthenticated.headers.get("location")!).pathname).toBe("/");

    const session = await signUp("oauth@example.com");
    const { callback } = await authorize(session.cookie, verifier, "opaque state / +");
    expect(callback.protocol).toBe("zmux-dev:");
    expect(callback.host).toBe("oauth");
    expect(callback.pathname).toBe("/callback");
    expect(callback.searchParams.get("state")).toBe("opaque state / +");
    expect(callback.searchParams.get("code")).toMatch(/^[A-Za-z0-9_-]{43}$/);
  });

  test("rejects unregistered callbacks, clients, and PKCE methods before redirecting", async () => {
    const session = await signUp("reject@example.com");
    const base = {
      client_id: "zmux-macos-dev",
      redirect_uri: "zmux-dev://oauth/callback",
      response_type: "code",
      code_challenge: createHash("sha256").update("a".repeat(64)).digest("base64url"),
      code_challenge_method: "S256",
      state: "state",
    };
    for (const change of [
      { redirect_uri: "zmux-dev://oauth/other" },
      { client_id: "other-client" },
      { code_challenge_method: "plain" },
    ]) {
      const response = await fetch(`${origin}/v1/native/authorize?${new URLSearchParams({ ...base, ...change })}`, {
        headers: { cookie: session.cookie },
        redirect: "manual",
      });
      expect(response.status).toBe(400);
      expect(response.headers.get("location")).toBeNull();
    }
  });

  test("uses S256, consumes each code once, and never returns the bearer token in a URL", async () => {
    const session = await signUp("pkce@example.com");
    const verifier = "c".repeat(64);
    const { code, callback } = await authorize(session.cookie, verifier);
    const wrong = await exchange(code, "x".repeat(64));
    expect(wrong.status).toBe(400);

    const accepted = await exchange(code, verifier);
    expect(accepted.status).toBe(200);
    const payload = (await accepted.json()) as { access_token: string };
    expect(callback.toString()).not.toContain(payload.access_token);

    const replay = await exchange(code, verifier);
    expect(replay.status).toBe(400);
  });

  test("binds native bearer validity to the Better Auth session", async () => {
    const session = await signUp("session@example.com");
    const token = await nativeToken(session.cookie);
    const before = await fetch(`${origin}/v1/me`, { headers: { authorization: `Bearer ${token}` } });
    expect(before.status).toBe(200);

    const signOut = await fetch(`${origin}/api/auth/sign-out`, {
      method: "POST",
      headers: { cookie: session.cookie, origin },
    });
    expect(signOut.status).toBe(200);

    const after = await fetch(`${origin}/v1/me`, { headers: { authorization: `Bearer ${token}` } });
    expect(after.status).toBe(401);
  });
});

describe("owner-scoped device registry", () => {
  test("repeated registration updates one stable installation row", async () => {
    const session = await signUp("stable-device@example.com");
    const token = await nativeToken(session.cookie);
    const registration = newDeviceRegistration();
    const headers = { authorization: `Bearer ${token}`, origin, "content-type": "application/json" };
    const register = (name: string) => fetch(`${origin}/v1/devices`, {
      method: "POST",
      headers,
      body: JSON.stringify({ ...registration, name }),
    });

    const first = await register("Old Name");
    expect(first.status).toBe(201);
    const firstDevice = await first.json() as { id: string; sharing: boolean };
    const registered: RegisteredDevice = { id: firstDevice.id, registration };
    const epoch = crypto.randomUUID();
    expect((await setPresence(token, registered, true, epoch)).status).toBe(200);
    const repeated = await register("Current Name");
    expect(repeated.status).toBe(200);
    expect(await repeated.json()).toMatchObject({ id: firstDevice.id, name: "Current Name", online: true, sharing: true, sharing_epoch: epoch });
    expect((await pool.query("SELECT id FROM device WHERE user_id = $1", [session.user.id])).rowCount).toBe(1);
  });

  test("requires modern identity fields instead of silently creating another legacy row", async () => {
    const session = await signUp("legacy-registration@example.com");
    const token = await nativeToken(session.cookie);
    const response = await fetch(`${origin}/v1/devices`, {
      method: "POST",
      headers: { authorization: `Bearer ${token}`, origin, "content-type": "application/json" },
      body: JSON.stringify({ name: "Legacy Client", platform: "macOS" }),
    });
    expect(response.status).toBe(400);
    expect(await pool.query("SELECT count(*)::int AS count FROM device WHERE user_id = $1", [session.user.id])).toMatchObject({ rows: [{ count: 0 }] });
  });

  test("concurrent retries converge on one stable installation", async () => {
    const session = await signUp("concurrent-device@example.com");
    const token = await nativeToken(session.cookie);
    const registration = newDeviceRegistration();
    const results = await Promise.all(Array.from({ length: 8 }, () => registerDevice(token, registration)));
    expect(results.every(({ response }) => response.status === 200 || response.status === 201)).toBe(true);
    const ids = results.map(({ device }) => device!.id);
    expect(new Set(ids).size).toBe(1);
    expect(ids[0]).toBeDefined();
    expect((await pool.query("SELECT count(*)::int AS count FROM device WHERE user_id = $1", [session.user.id])).rows[0]!.count).toBe(1);
  });

  test("rejects identity substitution, keeps same-name installations distinct, and never revives a revoked row", async () => {
    const session = await signUp("identity-substitution@example.com");
    const token = await nativeToken(session.cookie);
    const first = newDeviceRegistration("Shared Name");
    const created = await registerDevice(token, first);
    expect(created.response.status).toBe(201);
    const proofSubstitution = { ...first, device_secret: randomBytes(32).toString("base64url") };
    expect((await registerDevice(token, proofSubstitution)).response.status).toBe(409);
    const keySubstitution = { ...first, public_key: randomBytes(32).toString("base64url") };
    expect((await registerDevice(token, keySubstitution)).response.status).toBe(409);

    const second = newDeviceRegistration("Shared Name");
    const distinct = await registerDevice(token, second);
    expect(distinct.response.status).toBe(201);
    expect(distinct.device!.id).not.toBe(created.device!.id);

    expect((await fetch(`${origin}/v1/devices/${created.device!.id}`, {
      method: "DELETE", headers: { authorization: `Bearer ${token}`, origin },
    })).status).toBe(204);
    const reenrollment = await registerDevice(token, first);
    expect(reenrollment.response.status).toBe(409);
    expect(await reenrollment.response.json()).toMatchObject({ error: "device_revoked" });
    expect((await fetch(`${origin}/v1/terminal-relay/inbox`, { headers: deviceHeaders(token, created.device!) })).status).toBe(403);
  });

  test("lists legacy rows without merging them and limits legacy heartbeats to historical devices", async () => {
    const session = await signUp("legacy-device@example.com");
    const token = await nativeToken(session.cookie);
    const created = await registerDevice(token, newDeviceRegistration("Modern Mac"));
    expect(created.response.status).toBe(201);
    const legacyId = crypto.randomUUID();
    await pool.query(
      `INSERT INTO device (id, user_id, name, platform, last_seen_at) VALUES ($1, $2, $3, $4, now())`,
      [legacyId, session.user.id, "Historical Mac", "macOS"],
    );

    const listed = await fetch(`${origin}/v1/devices`, { headers: { authorization: `Bearer ${token}` } });
    const result = await listed.json() as { devices: Array<Record<string, unknown>> };
    expect(result.devices).toHaveLength(2);
    const legacy = result.devices.find((device) => device.id === legacyId)!;
    expect(legacy).toMatchObject({ name: "Historical Mac", online: true, sharing: false });
    expect(legacy).not.toHaveProperty("installation_id");
    expect(legacy).not.toHaveProperty("public_key");

    expect((await fetch(`${origin}/v1/devices/${legacyId}/heartbeat`, {
      method: "POST", headers: { authorization: `Bearer ${token}`, origin },
    })).status).toBe(200);
    expect((await fetch(`${origin}/v1/devices/${created.device!.id}/heartbeat`, {
      method: "POST", headers: { authorization: `Bearer ${token}`, origin },
    })).status).toBe(404);
  });

  test("starts a new sharing epoch only after the previous lease expires", async () => {
    const session = await signUp("presence-epoch@example.com");
    const token = await nativeToken(session.cookie);
    const created = await registerDevice(token);
    expect(created.response.status).toBe(201);
    const device = created.device!;
    const epochA = crypto.randomUUID();
    const epochB = crypto.randomUUID();
    const epochC = crypto.randomUUID();
    expect((await setPresence(token, device, true, epochA)).status).toBe(200);
    expect((await setPresence(token, device, true, epochB)).status).toBe(409);
    expect((await setPresence(token, device, false, epochA)).status).toBe(200);
    expect((await setPresence(token, device, true, epochA)).status).toBe(409);
    expect((await setPresence(token, device, true, epochB)).status).toBe(200);

    await pool.query("UPDATE device SET last_seen_at = now() - interval '16 seconds' WHERE id = $1", [device.id]);
    expect((await setPresence(token, device, true, epochC)).status).toBe(200);
    expect((await setPresence(token, device, false, epochB)).status).toBe(409);
    expect((await setPresence(token, device, true, epochB)).status).toBe(409);
  });

  test("does not reveal another user's device", async () => {
    const first = await signUp("first@example.com");
    const second = await signUp("second@example.com");
    const firstToken = await nativeToken(first.cookie);
    const secondToken = await nativeToken(second.cookie);
    const created = await registerDevice(firstToken, newDeviceRegistration("Owner Mac"));
    expect(created.response.status).toBe(201);
    const foreignHeaders = { ...deviceHeaders(secondToken, created.device!, true), origin };
    expect((await fetch(`${origin}/v1/devices/${created.device!.id}/presence`, { method: "POST", headers: foreignHeaders, body: "{}" })).status).toBe(403);
    expect((await fetch(`${origin}/v1/devices/${created.device!.id}`, { method: "DELETE", headers: { authorization: `Bearer ${secondToken}`, origin } })).status).toBe(404);
  });

  test("revokes the calling native token without affecting its browser session", async () => {
    const session = await signUp("revoke@example.com");
    const token = await nativeToken(session.cookie);
    const authorization = { authorization: `Bearer ${token}` };
    expect((await fetch(`${origin}/v1/native/revoke`, { method: "POST", headers: authorization })).status).toBe(204);
    expect((await fetch(`${origin}/v1/me`, { headers: authorization })).status).toBe(401);
    expect((await fetch(`${origin}/api/auth/get-session`, { headers: { cookie: session.cookie } })).status).toBe(200);
  });
});

describe("authenticated encrypted terminal relay", () => {
  async function sharedPair(email: string) {
    const session = await signUp(email);
    const senderToken = await nativeToken(session.cookie);
    const targetToken = await nativeToken(session.cookie);
    const sender = (await registerDevice(senderToken, newDeviceRegistration("Sender Mac"))).device!;
    const target = (await registerDevice(targetToken, newDeviceRegistration("Host Mac"))).device!;
    const epoch = crypto.randomUUID();
    expect((await setPresence(targetToken, target, true, epoch)).status).toBe(200);
    return { session, senderToken, targetToken, sender, target, epoch };
  }

  function relayBody(id: string, target: RegisteredDevice, payload = Buffer.from("encrypted request").toString("base64url")) {
    return { id, target_device_id: target.id, payload };
  }

  async function sendRelayRequest(token: string, sender: RegisteredDevice, body: unknown): Promise<Response> {
    return fetch(`${origin}/v1/terminal-relay/requests`, {
      method: "POST",
      headers: { ...deviceHeaders(token, sender, true), origin },
      body: JSON.stringify(body),
    });
  }

  async function claimInbox(token: string, target: RegisteredDevice): Promise<Response> {
    return fetch(`${origin}/v1/terminal-relay/inbox`, { headers: deviceHeaders(token, target) });
  }

  test("validates the encrypted envelope and rejects cross-account targets", async () => {
    const pair = await sharedPair("relay-validation@example.com");
    const invalid = await sendRelayRequest(pair.senderToken, pair.sender, {
      ...relayBody(crypto.randomUUID(), pair.target), payload: "***",
    });
    expect(invalid.status).toBe(400);

    const foreign = await signUp("relay-foreign@example.com");
    const foreignToken = await nativeToken(foreign.cookie);
    const foreignDevice = (await registerDevice(foreignToken)).device!;
    const crossing = await sendRelayRequest(pair.senderToken, pair.sender, relayBody(crypto.randomUUID(), foreignDevice));
    expect(crossing.status).toBe(404);
  });

  test("accepts encrypted response envelopes larger than the generic JSON body limit over HTTP", async () => {
    const pair = await sharedPair("relay-large-response@example.com");
    const requestId = crypto.randomUUID();
    expect((await sendRelayRequest(pair.senderToken, pair.sender, relayBody(requestId, pair.target))).status).toBe(202);
    expect((await (await claimInbox(pair.targetToken, pair.target)).json() as { requests: unknown[] }).requests).toHaveLength(1);
    const begin = await fetch(`${origin}/v1/terminal-relay/requests/${requestId}/begin`, {
      method: "POST", headers: { ...deviceHeaders(pair.targetToken, pair.target), origin },
    });
    expect(begin.status).toBe(200);

    const payload = randomBytes(20 * 1024).toString("base64url");
    const response = await fetch(`${origin}/v1/terminal-relay/requests/${requestId}/response`, {
      method: "POST",
      headers: { ...deviceHeaders(pair.targetToken, pair.target, true), origin },
      body: JSON.stringify({ payload }),
    });
    expect(response.status).toBe(204);

    const result = await fetch(`${origin}/v1/terminal-relay/requests/${requestId}`, {
      headers: deviceHeaders(pair.senderToken, pair.sender),
    });
    expect(result.status).toBe(200);
    expect(await result.json()).toMatchObject({ id: requestId, state: "completed", payload });
  });

  test("rejects request bodies over 384 KiB before dispatching them to the application", async () => {
    const dispatchedBefore = appDispatchCount;
    const response = await fetch(`${origin}/v1/terminal-relay/requests`, {
      method: "POST",
      headers: { "content-type": "application/json", origin },
      body: "x".repeat(MAX_RELAY_BODY_BYTES + 1),
    });
    expect(response.status).toBe(413);
    expect(appDispatchCount).toBe(dispatchedBefore);
  });

  test("makes retries idempotent only for the original sender token and identical envelope", async () => {
    const pair = await sharedPair("relay-idempotency@example.com");
    const requestId = crypto.randomUUID();
    const envelope = relayBody(requestId, pair.target);
    const results = await Promise.all(Array.from({ length: 8 }, () => sendRelayRequest(pair.senderToken, pair.sender, envelope)));
    expect(results.map(({ status }) => status).sort()).toEqual([200, 200, 200, 200, 200, 200, 200, 202]);
    expect((await pool.query("SELECT count(*)::int AS count FROM terminal_relay_request WHERE id = $1", [requestId])).rows[0]!.count).toBe(1);

    const secondToken = await nativeToken(pair.session.cookie);
    expect((await sendRelayRequest(secondToken, pair.sender, envelope)).status).toBe(409);
    expect((await sendRelayRequest(pair.senderToken, pair.sender, { ...envelope, payload: Buffer.from("different").toString("base64url") })).status).toBe(409);
  });

  test("serializes inbox claims with enqueue expiration cleanup", async () => {
    const pair = await sharedPair("relay-claim-enqueue-lock-order@example.com");
    const expiredRequestId = crypto.randomUUID();
    expect((await sendRelayRequest(pair.senderToken, pair.sender, relayBody(expiredRequestId, pair.target))).status).toBe(202);
    await pool.query(
      "UPDATE terminal_relay_request SET expires_at = now() - interval '1 second' WHERE id = $1",
      [expiredRequestId],
    );

    const blocker = await pool.connect();
    let claim: Promise<Response> | undefined;
    let enqueue: Promise<Response> | undefined;
    let discardBlocker = false;
    try {
      await blocker.query("BEGIN");
      await blocker.query("SELECT id FROM device WHERE id = $1 FOR UPDATE", [pair.target.id]);

      claim = claimInbox(pair.targetToken, pair.target);
      await waitForDatabaseLockWaiters(1);

      const nextRequestId = crypto.randomUUID();
      enqueue = sendRelayRequest(pair.senderToken, pair.sender, relayBody(nextRequestId, pair.target));
      await waitForDatabaseLockWaiters(2);
    } finally {
      try {
        await blocker.query("ROLLBACK");
      } catch (error) {
        discardBlocker = true;
        throw error;
      } finally {
        blocker.release(discardBlocker);
        await Promise.allSettled([claim, enqueue].filter((request): request is Promise<Response> => request !== undefined));
      }
    }

    const [claimResponse, enqueueResponse] = await Promise.all([claim!, enqueue!]);
    expect(claimResponse.status).toBe(200);
    expect(enqueueResponse.status).toBe(202);
    expect((await pool.query("SELECT state FROM terminal_relay_request WHERE id = $1", [expiredRequestId])).rows[0]!.state).toBe("expired");

    const nextInbox = await claimInbox(pair.targetToken, pair.target);
    expect(nextInbox.status).toBe(200);
    expect((await nextInbox.json() as { requests: Array<{ id: string }> }).requests.map(({ id }) => id)).toEqual([
      (await enqueueResponse.clone().json() as { id: string }).id,
    ]);
  });

  test("enforces per-device active queue limits", async () => {
    const pair = await sharedPair("relay-capacity@example.com");
    for (let index = 0; index < 16; index += 1) {
      const response = await sendRelayRequest(pair.senderToken, pair.sender, relayBody(crypto.randomUUID(), pair.target, Buffer.from(`payload-${index}`).toString("base64url")));
      expect(response.status).toBe(202);
    }
    const full = await sendRelayRequest(pair.senderToken, pair.sender, relayBody(crypto.randomUUID(), pair.target, Buffer.from("capacity").toString("base64url")));
    expect(full.status).toBe(429);
    expect(await full.json()).toMatchObject({ error: "relay_capacity" });
  });

  test("allows more than the former IP limit on authenticated inbox polling", async () => {
    const pair = await sharedPair("relay-rate-limit@example.com");
    const responses = await Promise.all(Array.from({ length: 65 }, () => claimInbox(pair.targetToken, pair.target)));
    expect(responses.every((response) => response.status === 200)).toBe(true);
  });

  test("atomically claims once, revalidates begin, and completes accepted work after sender revocation", async () => {
    const pair = await sharedPair("relay-execution@example.com");
    const requestId = crypto.randomUUID();
    expect((await sendRelayRequest(pair.senderToken, pair.sender, relayBody(requestId, pair.target))).status).toBe(202);

    const polls = await Promise.all(Array.from({ length: 8 }, () => claimInbox(pair.targetToken, pair.target)));
    const messages = await Promise.all(polls.map(async (response) => {
      expect(response.status).toBe(200);
      return (await response.json() as { requests: unknown[] }).requests;
    }));
    expect(messages.flat()).toHaveLength(1);
    expect(messages.flat()[0]).toMatchObject({ id: requestId, sender_device_id: pair.sender.id, target_epoch: pair.epoch });

    expect((await fetch(`${origin}/v1/terminal-relay/requests/${requestId}`, {
      headers: deviceHeaders(pair.targetToken, pair.target),
    })).status).toBe(404);
    expect((await fetch(`${origin}/v1/terminal-relay/requests/${requestId}/response`, {
      method: "POST",
      headers: { ...deviceHeaders(pair.senderToken, pair.sender, true), origin },
      body: JSON.stringify({ payload: Buffer.from("not target").toString("base64url") }),
    })).status).toBe(404);

    const begin = await fetch(`${origin}/v1/terminal-relay/requests/${requestId}/begin`, {
      method: "POST", headers: { ...deviceHeaders(pair.targetToken, pair.target), origin },
    });
    expect(begin.status).toBe(200);
    expect(await begin.json()).toEqual({ id: requestId, state: "executing" });

    expect((await fetch(`${origin}/v1/native/revoke`, {
      method: "POST", headers: { authorization: `Bearer ${pair.senderToken}`, origin },
    })).status).toBe(204);
    const responsePayload = Buffer.from("encrypted response").toString("base64url");
    expect((await fetch(`${origin}/v1/terminal-relay/requests/${requestId}/response`, {
      method: "POST",
      headers: { ...deviceHeaders(pair.targetToken, pair.target, true), origin },
      body: JSON.stringify({ payload: responsePayload }),
    })).status).toBe(204);

    expect((await fetch(`${origin}/v1/terminal-relay/requests/${requestId}`, {
      headers: deviceHeaders(pair.senderToken, pair.sender),
    })).status).toBe(401);
    const replacementToken = await nativeToken(pair.session.cookie);
    const mismatchedStatus = await fetch(`${origin}/v1/terminal-relay/requests/${requestId}`, {
      headers: deviceHeaders(replacementToken, pair.sender),
    });
    expect(mismatchedStatus.status).toBe(404);
    const saved = await pool.query<{ state: string; response_payload: Buffer }>(
      "SELECT state, response_payload FROM terminal_relay_request WHERE id = $1", [requestId],
    );
    expect(saved.rows[0]!.state).toBe("completed");
    expect(saved.rows[0]!.response_payload.equals(Buffer.from(responsePayload, "base64url"))).toBe(true);
  });

  test("expires claimed work when the original sender session is revoked before begin", async () => {
    const senderSession = await signUp("relay-session-revoke@example.com");
    const senderToken = await nativeToken(senderSession.cookie);
    const sender = (await registerDevice(senderToken)).device!;
    const signIn = await fetch(`${origin}/api/auth/sign-in/email`, {
      method: "POST",
      headers: { origin, "content-type": "application/json" },
      body: JSON.stringify({ email: senderSession.user.email, password: "correct horse battery staple" }),
    });
    expect(signIn.status).toBe(200);
    const targetCookie = cookieFrom(signIn);
    const targetToken = await nativeToken(targetCookie);
    const target = (await registerDevice(targetToken)).device!;
    const epoch = crypto.randomUUID();
    expect((await setPresence(targetToken, target, true, epoch)).status).toBe(200);
    const requestId = crypto.randomUUID();
    expect((await sendRelayRequest(senderToken, sender, relayBody(requestId, target))).status).toBe(202);
    const claimed = await claimInbox(targetToken, target);
    expect((await claimed.json() as { requests: unknown[] }).requests).toHaveLength(1);

    expect((await fetch(`${origin}/api/auth/sign-out`, {
      method: "POST", headers: { cookie: senderSession.cookie, origin },
    })).status).toBe(200);
    const begin = await fetch(`${origin}/v1/terminal-relay/requests/${requestId}/begin`, {
      method: "POST", headers: { ...deviceHeaders(targetToken, target), origin },
    });
    expect(begin.status).toBe(409);
    expect((await pool.query("SELECT state FROM terminal_relay_request WHERE id = $1", [requestId])).rows[0]!.state).toBe("expired");
  });

  test("expires pending work at PostgreSQL time and reports terminal status", async () => {
    const pair = await sharedPair("relay-expiry@example.com");
    const requestId = crypto.randomUUID();
    expect((await sendRelayRequest(pair.senderToken, pair.sender, relayBody(requestId, pair.target))).status).toBe(202);
    await pool.query("UPDATE terminal_relay_request SET expires_at = now() - interval '1 second' WHERE id = $1", [requestId]);

    const inbox = await claimInbox(pair.targetToken, pair.target);
    expect((await inbox.json() as { requests: unknown[] }).requests).toEqual([]);
    const status = await fetch(`${origin}/v1/terminal-relay/requests/${requestId}`, {
      headers: deviceHeaders(pair.senderToken, pair.sender),
    });
    expect(status.status).toBe(200);
    expect(await status.json()).toMatchObject({ id: requestId, state: "expired" });
  });

  test("sharing changes expire pending and claimed work while preserving executing work", async () => {
    const pair = await sharedPair("relay-sharing-revocation@example.com");
    const requests = [crypto.randomUUID(), crypto.randomUUID(), crypto.randomUUID()];
    for (const requestId of requests) {
      expect((await sendRelayRequest(pair.senderToken, pair.sender, relayBody(requestId, pair.target))).status).toBe(202);
    }
    await pool.query(
      `UPDATE terminal_relay_request
          SET created_at = now() + (array_position($1::uuid[], id) * interval '1 second')
        WHERE id = ANY($1::uuid[])`,
      [requests],
    );
    const claimedFirst = await claimInbox(pair.targetToken, pair.target);
    expect((await claimedFirst.json() as { requests: Array<{ id: string }> }).requests[0]!.id).toBe(requests[0]!);
    expect((await fetch(`${origin}/v1/terminal-relay/requests/${requests[0]!}/begin`, {
      method: "POST", headers: { ...deviceHeaders(pair.targetToken, pair.target), origin },
    })).status).toBe(200);

    const claimedSecond = await claimInbox(pair.targetToken, pair.target);
    expect((await claimedSecond.json() as { requests: Array<{ id: string }> }).requests[0]!.id).toBe(requests[1]!);
    expect((await setPresence(pair.targetToken, pair.target, false, pair.epoch)).status).toBe(200);
    const states = await pool.query<{ id: string; state: string }>(
      "SELECT id, state FROM terminal_relay_request WHERE id = ANY($1::uuid[]) ORDER BY created_at, id", [requests],
    );
    expect(states.rows.map(({ state }) => state)).toEqual(["executing", "expired", "expired"]);

    const response = await fetch(`${origin}/v1/terminal-relay/requests/${requests[0]}/response`, {
      method: "POST",
      headers: { ...deviceHeaders(pair.targetToken, pair.target, true), origin },
      body: JSON.stringify({ payload: Buffer.from("late response").toString("base64url") }),
    });
    expect(response.status).toBe(409);
    expect((await pool.query("SELECT state FROM terminal_relay_request WHERE id = $1", [requests[0]!])).rows[0]!.state).toBe("expired");
  });
});

test("returns Better Auth organization membership to the native client", async () => {
  const session = await signUp("organization@example.com");
  const created = await fetch(`${origin}/api/auth/organization/create`, {
    method: "POST",
    headers: { cookie: session.cookie, origin, "content-type": "application/json" },
    body: JSON.stringify({ name: "Terminal Lab", slug: "terminal-lab" }),
  });
  expect(created.status).toBe(200);

  const token = await nativeToken(session.cookie);
  const me = await fetch(`${origin}/v1/me`, { headers: { authorization: `Bearer ${token}` } });
  expect(me.status).toBe(200);
  expect(await me.json()).toEqual({
    user: { id: session.user.id, name: "Test User", email: "organization@example.com" },
    organizations: [{ id: expect.any(String), name: "Terminal Lab", slug: "terminal-lab", role: "owner" }],
  });
});

test("rejects cross-origin mutations and malformed bearer credentials", async () => {
  const crossOrigin = await fetch(`${origin}/v1/native/token`, {
    method: "POST",
    headers: { origin: "https://attacker.example", "content-type": "application/json" },
    body: "{}",
  });
  expect(crossOrigin.status).toBe(403);
  expect((await fetch(`${origin}/v1/me`, { headers: { authorization: "Bearer not-a-token" } })).status).toBe(401);
});

test("reports only implemented readiness capabilities", async () => {
  const ready = await fetch(`${origin}/ready`);
  expect(ready.status).toBe(200);
  expect(await ready.json()).toEqual({
    status: "ready",
    capabilities: {
      identity: "ready",
      devices: "ready",
      stable_device_identity: "ready",
      organizations: "ready",
      vm: "unavailable",
      terminal_relay: "ready",
      email: "unavailable",
    },
  });
});

test("reports unavailable when PostgreSQL is reachable but migrations are absent", async () => {
  const schema = `zmux_unmigrated_${crypto.randomUUID().replaceAll("-", "")}`;
  await pool.query(`CREATE SCHEMA ${schema}`);
  const unmigratedPool = new Pool({ connectionString: databaseUrl, options: `-c search_path=${schema}`, max: 1 });
  try {
    const unmigratedApp = createApp(unmigratedPool, auth, config);
    const response = await unmigratedApp(new Request(`${origin}/ready`));
    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({
      status: "unavailable",
      capabilities: {
        identity: "unavailable",
        devices: "unavailable",
        stable_device_identity: "unavailable",
        terminal_relay: "unavailable",
      },
    });
    const configResponse = await unmigratedApp(new Request(`${origin}/v1/config`));
    expect(configResponse.status).toBe(200);
    expect(await configResponse.json()).toMatchObject({
      capabilities: { stable_device_identity: false, terminal_relay: false },
    });
  } finally {
    await unmigratedPool.end();
    await pool.query(`DROP SCHEMA ${schema} CASCADE`);
  }
});

test("expired credential cleanup bounds batches, skips locked rows and retains usable tokens", async () => {
  const session = await signUp("cleanup@example.com");
  const liveToken = await nativeToken(session.cookie);
  const { code } = await authorize(session.cookie, "c".repeat(64));
  const liveCodeHash = createHash("sha256").update(code).digest();
  await pool.query(`INSERT INTO native_authorization_code
    (code_hash, user_id, auth_session_id, client_id, redirect_uri, code_challenge, expires_at)
    SELECT decode(md5('expired-code-' || n), 'hex'), user_id, auth_session_id,
      client_id, redirect_uri, code_challenge, now() - interval '1 hour'
    FROM native_authorization_code CROSS JOIN generate_series(1, 1002) n
    WHERE code_hash = $1`, [liveCodeHash]);
  await pool.query(`INSERT INTO native_access_token
    (id, token_hash, user_id, auth_session_id, expires_at)
    SELECT md5('expired-token-' || n)::uuid, decode(md5('expired-token-' || n), 'hex'),
      user_id, auth_session_id, now() - interval '1 hour'
    FROM native_access_token CROSS JOIN generate_series(1, 1002) n`);
  const locked = await pool.connect();
  try {
    await locked.query("BEGIN");
    await locked.query("SELECT code_hash FROM native_authorization_code WHERE expires_at <= now() ORDER BY code_hash LIMIT 1 FOR UPDATE");
    await locked.query("SELECT id FROM native_access_token WHERE expires_at <= now() ORDER BY id LIMIT 1 FOR UPDATE");
    expect(await cleanupExpiredNativeCredentials(pool)).toEqual({ codes: 1000, tokens: 1000 });
    expect(await cleanupExpiredNativeCredentials(pool)).toEqual({ codes: 1, tokens: 1 });
    expect(await cleanupExpiredNativeCredentials(pool)).toEqual({ codes: 0, tokens: 0 });
  } finally {
    await locked.query("ROLLBACK");
    locked.release();
  }
  expect(await cleanupExpiredNativeCredentials(pool)).toEqual({ codes: 1, tokens: 1 });
  expect(await cleanupExpiredNativeCredentials(pool)).toEqual({ codes: 0, tokens: 0 });
  expect((await pool.query("SELECT count(*)::int AS count FROM native_authorization_code")).rows[0].count).toBe(1);
  expect((await pool.query("SELECT count(*)::int AS count FROM native_access_token")).rows[0].count).toBe(1);
  const identity = await fetch(`${origin}/v1/me`, { headers: { authorization: `Bearer ${liveToken}` } });
  expect(identity.status).toBe(200);
  expect((await exchange(code, "c".repeat(64))).status).toBe(200);
});


describe("authenticator protection", () => {
  test("requires a confirmed second factor before native authorization and consumes recovery codes once", async () => {
    // A real browser cookie jar: challenge and session rotation must survive HTTP.
    const jar = new Map<string, string>();
    async function request(path: string, body?: unknown) {
      const response = await fetch(origin + path, {
        method: body === undefined ? "GET" : "POST",
        headers: { origin, "content-type": "application/json", cookie: [...jar].map(([k, v]) => `${k}=${v}`).join("; "), "x-test-client-ip": "192.0.2.220" },
        body: body === undefined ? null : JSON.stringify(body), redirect: "manual",
      });
      for (const value of response.headers.getSetCookie()) {
        const pair = value.split(";", 1)[0]!; const index = pair.indexOf("=");
        jar.set(pair.slice(0, index), pair.slice(index + 1));
      }
      return response;
    }
    const credentials = { email: "mfa@example.com", password: "correct horse battery staple" };
    expect((await request("/api/auth/sign-up/email", { ...credentials, name: "MFA User" })).status).toBe(200);
    expect((await request("/api/auth/two-factor/enable", { password: "incorrect password" })).ok).toBe(false);
    const enabled = await request("/api/auth/two-factor/enable", { password: credentials.password });
    expect(enabled.status).toBe(200);
    const enrollment = await enabled.json() as { totpURI: string; backupCodes: string[] };
    const uri = new URL(enrollment.totpURI);
    const secret = uri.searchParams.get("secret")!;
    // Decode base32 and compute RFC6238 independently of the auth implementation.
    const bits = [...secret.replace(/=+$/, "").toUpperCase()].map(c => "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567".indexOf(c).toString(2).padStart(5, "0")).join("");
    const key = Buffer.from(bits.match(/.{8}/g)!.map(b => Number.parseInt(b, 2)));
    function code(stepOffset = 0) {
      const counter = Buffer.alloc(8); counter.writeBigUInt64BE(BigInt(Math.floor(Date.now() / 30000) + stepOffset));
      const digest = createHmac("sha1", key).update(counter).digest();
      return ((digest.readUInt32BE(digest[19]! & 15) & 0x7fffffff) % 1000000).toString().padStart(6, "0");
    }
    async function isEnabled() {
      const response = await request("/api/auth/get-session");
      expect(response.status).toBe(200);
      return (await response.json() as { user: { twoFactorEnabled: boolean } }).user.twoFactorEnabled;
    }
    expect(await isEnabled()).toBe(false);
    const wrongCode = () => {
      const window = new Set([-2, -1, 0, 1, 2].map(code));
      for (let candidate = 0; ; candidate++) {
        const value = candidate.toString().padStart(6, "0");
        if (!window.has(value)) return value;
      }
    };
    expect((await request("/api/auth/two-factor/verify-totp", { code: wrongCode() })).ok).toBe(false);
    expect(await isEnabled()).toBe(false);
    expect((await request("/api/auth/two-factor/verify-totp", { code: code() })).status).toBe(200);
    expect(await isEnabled()).toBe(true);
    // Respect the real owner's three-verifications-per-ten-seconds limit.
    await Bun.sleep(10_100);
    expect((await request("/api/auth/sign-out", {})).status).toBe(200);
    jar.clear();
    const signIn = await request("/api/auth/sign-in/email", credentials);
    expect(signIn.status).toBe(200);
    expect((await signIn.json() as { twoFactorRedirect: boolean }).twoFactorRedirect).toBe(true);
    const verifier = "m".repeat(64);
    const query = new URLSearchParams({ client_id: "zmux-macos-dev", redirect_uri: "zmux-dev://oauth/callback", response_type: "code", code_challenge: createHash("sha256").update(verifier).digest("base64url"), code_challenge_method: "S256", state: "mfa-state" });
    async function nativeLocation() {
      const response = await request("/v1/native/authorize?" + query);
      expect(response.status).toBe(302);
      return new URL(response.headers.get("location")!);
    }
    expect((await nativeLocation()).pathname).toBe("/");
    expect((await request("/api/auth/two-factor/verify-totp", { code: wrongCode() })).ok).toBe(false);
    expect((await nativeLocation()).pathname).toBe("/");
    expect((await request("/api/auth/two-factor/verify-totp", { code: code() })).status).toBe(200);
    expect((await nativeLocation()).protocol).toBe("zmux-dev:");
    expect((await request("/api/auth/sign-out", {})).status).toBe(200); jar.clear();
    expect((await request("/api/auth/sign-in/email", credentials)).status).toBe(200);
    expect((await request("/api/auth/two-factor/verify-backup-code", { code: enrollment.backupCodes[0] })).status).toBe(200);
    expect((await nativeLocation()).protocol).toBe("zmux-dev:");
    expect((await request("/api/auth/sign-out", {})).status).toBe(200); jar.clear();
    expect((await request("/api/auth/sign-in/email", credentials)).status).toBe(200);
    expect((await request("/api/auth/two-factor/verify-backup-code", { code: enrollment.backupCodes[0] })).ok).toBe(false);
    expect((await nativeLocation()).pathname).toBe("/");
    expect((await request("/api/auth/two-factor/verify-backup-code", { code: enrollment.backupCodes[1] })).status).toBe(200);
    expect((await request("/api/auth/two-factor/disable", { password: "incorrect password" })).ok).toBe(false);
    expect(await isEnabled()).toBe(true);
    expect((await request("/api/auth/two-factor/disable", { password: credentials.password })).status).toBe(200);
    expect(await isEnabled()).toBe(false);
    // A fourth password login also observes the real ten-second auth rate window.
    await Bun.sleep(10_100);
    expect((await request("/api/auth/sign-out", {})).status).toBe(200); jar.clear();
    const passwordOnly = await request("/api/auth/sign-in/email", credentials);
    expect(passwordOnly.status).toBe(200);
    expect((await passwordOnly.json() as { twoFactorRedirect?: boolean }).twoFactorRedirect).not.toBe(true);
    expect((await nativeLocation()).protocol).toBe("zmux-dev:");
  }, 30_000);
});


test("readiness rejects a password-only schema before MFA migrations", async () => {
  await pool.query('ALTER TABLE "twoFactor" RENAME TO zmux_pending_mfa');
  try {
    expect((await fetch(`${origin}/ready`)).status).toBe(503);
  } finally {
    await pool.query('ALTER TABLE zmux_pending_mfa RENAME TO "twoFactor"');
  }
  await pool.query('ALTER TABLE "user" RENAME COLUMN "twoFactorEnabled" TO zmux_pending_mfa_enabled');
  try {
    expect((await fetch(`${origin}/ready`)).status).toBe(503);
  } finally {
    await pool.query('ALTER TABLE "user" RENAME COLUMN zmux_pending_mfa_enabled TO "twoFactorEnabled"');
  }
  expect((await fetch(`${origin}/ready`)).status).toBe(200);
});
