import { afterAll, expect, test } from "bun:test";
import { Pool } from "pg";
import { withTransaction } from "../src/db";

if (!process.env.DATABASE_URL) throw new Error("DATABASE_URL is required for transaction integration tests");
// One physical slot makes leaked/poisoned ownership observable on the next operation.
const pool = new Pool({ connectionString: process.env.DATABASE_URL, max: 1, connectionTimeoutMillis: 2_000 });
afterAll(async () => { await pool.end(); });

test("transactions commit results and roll back writes before reusing the pool", async () => {
  await pool.query("CREATE TEMP TABLE transaction_probe (value integer)");
  const result = await withTransaction(pool, async (client) => {
    await client.query("INSERT INTO transaction_probe VALUES (17)");
    return "committed";
  });
  expect(result).toBe("committed");
  const failure = new Error("operation failed");
  try {
    await withTransaction(pool, async (client) => {
      await client.query("INSERT INTO transaction_probe VALUES (23)");
      throw failure;
    });
    throw new Error("transaction unexpectedly succeeded");
  } catch (error) {
    expect(error).toBe(failure);
  }
  expect((await pool.query("SELECT value FROM transaction_probe ORDER BY value")).rows).toEqual([{ value: 17 }]);
  await pool.query("DROP TABLE transaction_probe");
});

test("the production statement deadline cancels work and leaves the slot usable", async () => {
  let failure: unknown;
  const started = performance.now();
  try {
    await withTransaction(pool, async (client) => { await client.query("SELECT pg_sleep(30)"); });
  } catch (error) { failure = error; }
  expect((failure as { code?: string })?.code).toBe("57014");
  expect(performance.now() - started).toBeLessThan(12_000);
  expect((await pool.query("SELECT 1 AS ready")).rows).toEqual([{ ready: 1 }]);
}, 15_000);

test("a terminated physical backend preserves both errors and is discarded", async () => {
  const originalPid = (await pool.query<{ pid: number }>("SELECT pg_backend_pid() AS pid")).rows[0]!.pid;
  let failure: unknown;
  try {
    await withTransaction(pool, async (client) => {
      // Real PostgreSQL terminates this connection during its in-flight operation.
      await client.query("SELECT pg_terminate_backend(pg_backend_pid())");
    });
  } catch (error) { failure = error; }
  expect(failure).toBeInstanceOf(AggregateError);
  const errors = (failure as AggregateError).errors;
  expect(errors).toHaveLength(2);
  expect((errors[0] as { code?: string }).code).toBe("57P01");
  expect(errors[1]).toBeInstanceOf(Error);
  const next = await pool.query<{ pid: number; ready: number }>("SELECT pg_backend_pid() AS pid, 1 AS ready");
  expect(next.rows[0]!.pid).not.toBe(originalPid);
  expect(next.rows[0]!.ready).toBe(1);
});
