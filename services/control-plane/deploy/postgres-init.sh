#!/bin/sh
set -eu

# Socket-only administration supports backups inside this isolated container.
# Every TCP connection, including app traffic, must use TLS and SCRAM.
cat > "$PGDATA/pg_hba.conf" <<'HBA'
local all all trust
hostssl all all all scram-sha-256
hostnossl all all all reject
HBA
