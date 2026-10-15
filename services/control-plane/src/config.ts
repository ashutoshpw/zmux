import { isIP } from "node:net";

export type ControlPlaneConfig = {
  origin: string;
  databaseUrl: string;
  authSecret: string;
  port: number;
  host: string;
  production: boolean;
  trustProxy: boolean;
};

function required(env: NodeJS.ProcessEnv, name: string): string {
  const value = env[name]?.trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
}

function parseBoolean(value: string | undefined, name: string): boolean {
  if (value === undefined) return false;
  if (value === "true") return true;
  if (value === "false") return false;
  throw new Error(`${name} must be true or false`);
}

export function loadConfig(env: NodeJS.ProcessEnv = process.env): ControlPlaneConfig {
  const production = env.NODE_ENV === "production";
  const originUrl = new URL(required(env, "ZMUX_CLOUD_ORIGIN"));
  const loopback = originUrl.hostname === "localhost" || originUrl.hostname === "127.0.0.1" || originUrl.hostname === "[::1]";

  if (originUrl.username || originUrl.password || originUrl.search || originUrl.hash || originUrl.pathname !== "/") {
    throw new Error("ZMUX_CLOUD_ORIGIN must be an origin without credentials, path, query, or fragment");
  }
  if (originUrl.protocol !== "https:" && !(originUrl.protocol === "http:" && !production && loopback)) {
    throw new Error("ZMUX_CLOUD_ORIGIN must use HTTPS; development HTTP is limited to loopback hosts");
  }

  const authSecret = required(env, "BETTER_AUTH_SECRET");
  if (new TextEncoder().encode(authSecret).length < 32) {
    throw new Error("BETTER_AUTH_SECRET must be at least 32 bytes");
  }

  const portText = env.PORT ?? "3000";
  const port = Number(portText);
  if (!Number.isSafeInteger(port) || port < 1 || port > 65535) throw new Error("PORT must be an integer from 1 to 65535");
  const host = env.HOST ?? "0.0.0.0";
  if (!isIP(host)) throw new Error("HOST must be an IPv4 or IPv6 address");

  return {
    origin: originUrl.origin,
    databaseUrl: required(env, "DATABASE_URL"),
    authSecret,
    port,
    host,
    production,
    trustProxy: parseBoolean(env.TRUST_PROXY, "TRUST_PROXY"),
  };
}
