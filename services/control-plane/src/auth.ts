import { betterAuth } from "better-auth";
import type { BetterAuthOptions } from "better-auth";
import { organization, twoFactor } from "better-auth/plugins";
import type { Pool } from "pg";
import type { ControlPlaneConfig } from "./config";

export function createAuthOptions(pool: Pool, config: ControlPlaneConfig, validateSchema = true) {
  const options = {
    appName: "zmux",
    database: pool,
    baseURL: config.origin,
    basePath: "/api/auth",
    secret: config.authSecret,
    trustedOrigins: [config.origin],
    emailAndPassword: {
      enabled: true,
      minPasswordLength: 12,
      maxPasswordLength: 128,
    },
    advanced: {
      useSecureCookies: config.production || new URL(config.origin).protocol === "https:",
      database: { validateSchema },
      ipAddress: {
        ipAddressHeaders: ["x-zmux-client-ip"],
      },
    },
    rateLimit: {
      enabled: true,
      window: 60,
      max: 120,
      storage: "memory",
    },
    plugins: [
      twoFactor({ issuer: "zmux" }),
      organization({
        allowUserToCreateOrganization: true,
        membershipLimit: 100,
      }),
    ],
  } satisfies BetterAuthOptions;
  return options;
}

export function createAuth(pool: Pool, config: ControlPlaneConfig) {
  return betterAuth(createAuthOptions(pool, config));
}

export type ZmuxAuth = ReturnType<typeof createAuth>;
