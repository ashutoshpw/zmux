import { createApp } from "./app";
import { createAuth } from "./auth";
import { loadConfig } from "./config";
import { createPool } from "./db";
import { MAX_RELAY_BODY_BYTES } from "./device-terminal";

const config = loadConfig();
const pool = createPool(config);
const auth = createAuth(pool, config);
const app = createApp(pool, auth, config);

const server = Bun.serve({
  hostname: config.host,
  port: config.port,
  maxRequestBodySize: MAX_RELAY_BODY_BYTES,
  fetch(request, server) {
    return app(request, { ip: server.requestIP(request)?.address });
  },
});

console.info(`zmux control plane listening on ${server.url.origin}`);

async function shutdown(): Promise<void> {
  await server.stop();
  await pool.end();
  process.exit(0);
}

process.once("SIGINT", shutdown);
process.once("SIGTERM", shutdown);
