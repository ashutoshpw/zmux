#!/bin/sh
set -eu

# PostgreSQL requires a private key owned by its runtime user. Keep the host's
# private directory private; copy only server material into the container.
mkdir -p /run/zmux-tls
cp /opt/zmux/tls/server.crt /run/zmux-tls/server.crt
cp /opt/zmux/tls/server.key /run/zmux-tls/server.key
chown -R postgres:postgres /run/zmux-tls
chmod 600 /run/zmux-tls/server.key
exec docker-entrypoint.sh postgres \
  -c listen_addresses=127.0.0.1 \
  -c "port=${ZMUX_LOCAL_DATABASE_PORT:?database port required}" \
  -c ssl=on \
  -c ssl_cert_file=/run/zmux-tls/server.crt \
  -c ssl_key_file=/run/zmux-tls/server.key
