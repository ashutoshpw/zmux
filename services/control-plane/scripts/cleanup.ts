import { loadConfig } from "../src/config";
import { createPool } from "../src/db";
import { cleanupExpiredNativeCredentials } from "../src/maintenance";

const pool = createPool(loadConfig());
try {
  console.info(JSON.stringify(await cleanupExpiredNativeCredentials(pool)));
} finally {
  await pool.end();
}
