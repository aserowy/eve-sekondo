#!/usr/bin/env bash
# Generates the local dev certificates with mkcert into certs/.
# One-time prerequisite: `mkcert -install` (adds the mkcert root CA to your trust stores).
set -euo pipefail

cd "$(dirname "$0")/.."
mkdir -p certs

mkcert -cert-file certs/dev.pem -key-file certs/dev-key.pem localhost 127.0.0.1 ::1
mkcert -cert-file certs/db.pem -key-file certs/db-key.pem db localhost 127.0.0.1
cp "$(mkcert -CAROOT)/rootCA.pem" certs/rootCA.pem

echo "Wrote certs/{dev,dev-key,db,db-key,rootCA}.pem"
