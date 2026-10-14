import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { getMigrations } from "better-auth/db/migration";
import { createAuthOptions } from "../src/auth";
import { loadConfig } from "../src/config";
import { createPool } from "../src/db";

const config = loadConfig();
const pool = createPool(config);
const authOptions = createAuthOptions(pool, config, false);

try {
  const client = await pool.connect();
  try {
    await client.query("SELECT pg_advisory_lock(hashtext('zmux-control-plane-migrations'))");
    await getMigrations({ ...authOptions, logger: { disabled: true } }).then(({ runMigrations }) => runMigrations());
    await client.query(`CREATE TABLE IF NOT EXISTS zmux_schema_migration (
      name text PRIMARY KEY,
      applied_at timestamptz NOT NULL DEFAULT now()
    )`);

    const directory = join(import.meta.dir, "..", "migrations");
    const files = (await readdir(directory)).filter((name) => name.endsWith(".sql")).sort();
    for (const name of files) {
      const applied = await client.query<{ name: string }>("SELECT name FROM zmux_schema_migration WHERE name = $1", [name]);
      if (applied.rowCount) continue;
      const sql = await readFile(join(directory, name), "utf8");
      await client.query("BEGIN");
      try {
        await client.query(sql);
        await client.query("INSERT INTO zmux_schema_migration (name) VALUES ($1)", [name]);
        await client.query("COMMIT");
        console.info(`applied ${name}`);
      } catch (error) {
        await client.query("ROLLBACK");
        throw error;
      }
    }
  } finally {
    await client.query("SELECT pg_advisory_unlock(hashtext('zmux-control-plane-migrations'))").catch(() => undefined);
    client.release();
  }
} finally {
  await pool.end();
}
