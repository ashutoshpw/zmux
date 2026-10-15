import { Pool, type PoolClient } from "pg";
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

/** Own the transaction and physical pool client through commit or failed cleanup. */
export async function withTransaction<T>(pool: Pool, operation: (client: PoolClient) => Promise<T>): Promise<T> {
  const client = await pool.connect();
  let discarded = false;
  let connectionFailure: Error | undefined;
  const onConnectionError = (error: Error) => { connectionFailure ??= error; };
  // pg emits asynchronous connection errors while a client is checked out. Keep
  // them owned until release instead of letting an EventEmitter error escape.
  client.on("error", onConnectionError);
  try {
    await client.query("BEGIN");
    await client.query("SET LOCAL statement_timeout = '5s'");
    await client.query("SET LOCAL lock_timeout = '3s'");
    const result = await operation(client);
    if (connectionFailure) throw connectionFailure;
    await client.query("COMMIT");
    if (connectionFailure) throw connectionFailure;
    return result;
  } catch (error) {
    try {
      await client.query("ROLLBACK");
    } catch (rollbackError) {
      client.release(true);
      discarded = true;
      throw new AggregateError([error, rollbackError], "Transaction and rollback failed");
    }
    throw error;
  } finally {
    try {
      if (!discarded) client.release();
    } finally {
      client.removeListener("error", onConnectionError);
    }
  }
}
