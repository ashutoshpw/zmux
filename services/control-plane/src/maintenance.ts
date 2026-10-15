import type { Pool } from "pg";
import { withTransaction } from "./db";

/** One bounded sweep; callers explicitly decide when to run another batch. */
export async function cleanupExpiredNativeCredentials(pool: Pool): Promise<{ codes: number; tokens: number }> {
  return withTransaction(pool, async (client) => {
    const codes = await client.query(`
      WITH expired AS (
        SELECT code_hash FROM native_authorization_code
        WHERE expires_at <= now() ORDER BY expires_at, code_hash
        LIMIT 1000 FOR UPDATE SKIP LOCKED
      )
      DELETE FROM native_authorization_code AS credential USING expired
      WHERE credential.code_hash = expired.code_hash`);
    const tokens = await client.query(`
      WITH expired AS (
        SELECT id FROM native_access_token
        WHERE expires_at <= now() ORDER BY expires_at, id
        LIMIT 1000 FOR UPDATE SKIP LOCKED
      )
      DELETE FROM native_access_token AS credential USING expired
      WHERE credential.id = expired.id`);
    return { codes: codes.rowCount ?? 0, tokens: tokens.rowCount ?? 0 };
  });
}
