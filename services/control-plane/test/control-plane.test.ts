import { afterAll, beforeAll, beforeEach, describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { Pool } from "pg";
import { createApp } from "../src/app";
import { createAuth } from "../src/auth";
import type { ControlPlaneConfig } from "../src/config";

const origin = process.env.ZMUX_TEST_ORIGIN ?? "http://127.0.0.1:41992";
const databaseUrl = process.env.DATABASE_URL;
if (!databaseUrl) throw new Error("DATABASE_URL is required for control-plane integration tests");

const config: ControlPlaneConfig = {
  origin,
  databaseUrl,
  authSecret: process.env.BETTER_AUTH_SECRET ?? "test-only-secret-test-only-secret-1234",
  port: Number(new URL(origin).port),
  production: false,
  trustProxy: false,
};

const pool = new Pool({ connectionString: databaseUrl, max: 10 });
let auth = createAuth(pool, config);
let app = createApp(pool, auth, config);
let server: ReturnType<typeof Bun.serve>;
let testClient = 1;

type Session = { cookie: string; user: { id: string; email: string } };

function cookieFrom(response: Response): string {
  const value = response.headers.get("set-cookie");
  if (!value) throw new Error("response did not set a session cookie");
  return value.split(";", 1)[0]!;
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
    fetch: (request) => app(request, { ip: request.headers.get("x-test-client-ip") ?? "127.0.0.1" }),
  });
});

beforeEach(async () => {
  await pool.query(
    `TRUNCATE device, native_access_token, native_authorization_code, invitation, member,
       organization, verification, account, "session", "user" CASCADE`,
  );
  auth = createAuth(pool, config);
  app = createApp(pool, auth, config);
});

afterAll(async () => {
  await server.stop();
  await pool.end();
});

describe("native OAuth contract", () => {
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
  test("registers, heartbeats, lists, and revokes the owner's device", async () => {
    const session = await signUp("devices@example.com");
    const token = await nativeToken(session.cookie);
    const headers = { authorization: `Bearer ${token}`, "content-type": "application/json" };
    const registered = await fetch(`${origin}/v1/devices`, {
      method: "POST",
      headers,
      body: JSON.stringify({ name: "Studio Mac", platform: "macOS" }),
    });
    expect(registered.status).toBe(201);
    const device = (await registered.json()) as { id: string; last_seen_at: string };

    const heartbeat = await fetch(`${origin}/v1/devices/${device.id}/heartbeat`, { method: "POST", headers });
    expect(heartbeat.status).toBe(200);

    const listed = await fetch(`${origin}/v1/devices`, { headers: { authorization: `Bearer ${token}` } });
    expect(await listed.json()).toEqual({
      devices: [{ id: device.id, name: "Studio Mac", platform: "macOS", last_seen_at: expect.any(String) }],
    });

    expect((await fetch(`${origin}/v1/devices/${device.id}`, { method: "DELETE", headers })).status).toBe(204);
    expect((await fetch(`${origin}/v1/devices/${device.id}/heartbeat`, { method: "POST", headers })).status).toBe(404);
  });

  test("does not reveal another user's device", async () => {
    const first = await signUp("first@example.com");
    const second = await signUp("second@example.com");
    const firstToken = await nativeToken(first.cookie);
    const secondToken = await nativeToken(second.cookie);
    const created = await fetch(`${origin}/v1/devices`, {
      method: "POST",
      headers: { authorization: `Bearer ${firstToken}`, "content-type": "application/json" },
      body: JSON.stringify({ name: "Owner Mac", platform: "macOS" }),
    });
    const device = (await created.json()) as { id: string };
    const foreignHeaders = { authorization: `Bearer ${secondToken}`, "content-type": "application/json" };
    expect((await fetch(`${origin}/v1/devices/${device.id}/heartbeat`, { method: "POST", headers: foreignHeaders })).status).toBe(404);
    expect((await fetch(`${origin}/v1/devices/${device.id}`, { method: "DELETE", headers: foreignHeaders })).status).toBe(404);
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
      organizations: "ready",
      vm: "unavailable",
      relay: "unavailable",
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
      capabilities: { identity: "unavailable", devices: "unavailable" },
    });
  } finally {
    await unmigratedPool.end();
    await pool.query(`DROP SCHEMA ${schema} CASCADE`);
  }
});
