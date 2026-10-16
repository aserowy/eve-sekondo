#!/bin/sh
# Postgres refuses a key not owned by postgres (or root with mode 0640 or tighter).
# Copy the read-only mounted key with the right owner and mode at every start.
set -eu
install -o postgres -g postgres -m 600 /certs-ro/db-key.pem /var/lib/postgresql/db-key.pem
install -o postgres -g postgres -m 644 /certs-ro/db.pem /var/lib/postgresql/db.pem
exec docker-entrypoint.sh "$@"
