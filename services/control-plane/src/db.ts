import { Pool } from "pg";
import type { ControlPlaneConfig } from "./config";

export function createPool(config: ControlPlaneConfig): Pool {
  return new Pool({
    connectionString: config.databaseUrl,
    max: 10,
    connectionTimeoutMillis: 5_000,
    idleTimeoutMillis: 30_000,
    application_name: "zmux-control-plane",
    ssl: config.production ? { rejectUnauthorized: true } : undefined,
  });
}
