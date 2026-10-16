#!/bin/sh
# The TLS key is mounted read-only and owned by the host user. Copy it to a tmpfs that only the
# unprivileged runtime user can read, then drop root and start the app.
set -eu
install -d -m 700 /run/sekondo
install -m 600 /certs/dev-key.pem /run/sekondo/tls-key.pem
chown -R sekondo:sekondo /run/sekondo
exec setpriv --reuid=sekondo --regid=sekondo --clear-groups --no-new-privs /app/sekondo "$@"
