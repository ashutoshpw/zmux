import type { Pool } from "pg";

/** One bounded sweep; callers explicitly decide when to run another batch. */
export async function cleanupExpiredNativeCredentials(pool: Pool): Promise<{ codes: number; tokens: number }> {
  const client = await pool.connect();
  let discarded = false;
  try {
    await client.query("BEGIN");
    await client.query("SET LOCAL statement_timeout = '5s'");
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
    await client.query("COMMIT");
    return { codes: codes.rowCount ?? 0, tokens: tokens.rowCount ?? 0 };
  } catch (error) {
    try {
      await client.query("ROLLBACK");
    } catch (rollbackError) {
      client.release(true);
      discarded = true;
      throw new AggregateError([error, rollbackError], "Credential cleanup and rollback failed");
    }
    throw error;
  } finally {
    // pg permits release only once; failed rollback already discarded this client.
    if (!discarded) client.release();
  }
}
