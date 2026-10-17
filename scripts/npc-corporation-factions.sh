#!/usr/bin/env bash
# Regenerates crates/sekondo/data/npc_corporation_factions.csv (corporation_id,faction_id)
# from the `npcCorporations` file of CCP's static data export (SDE).
# Usage: scripts/npc-corporation-factions.sh [path/to/sde.zip]
# Without an argument the latest SDE is downloaded. Needs curl, unzip and python3.
set -euo pipefail

cd "$(dirname "$0")/.."
url="https://developers.eveonline.com/static-data/eve-online-static-data-latest-jsonl.zip"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

zip="${1:-}"
if [ -z "$zip" ]; then
    zip="$work/sde.zip"
    curl -sSfL -o "$zip" "$url"
fi
unzip -q -o "$zip" npcCorporations.jsonl _sde.jsonl -d "$work"

python3 -I - "$work" "$url" > crates/sekondo/data/npc_corporation_factions.csv <<'PY'
import json, sys

work, url = sys.argv[1], sys.argv[2]
sde = json.loads(open(f"{work}/_sde.jsonl").readline())
print(f"# Source: CCP static data export, build {sde['buildNumber']} ({sde['releaseDate']}), npcCorporations.factionID")
print(f"# URL: {url}")
print("# Regenerate with scripts/npc-corporation-factions.sh")
print("corporation_id,faction_id")
rows = []
for line in open(f"{work}/npcCorporations.jsonl"):
    row = json.loads(line)
    if "factionID" in row:
        rows.append((row["_key"], row["factionID"]))
for corp, faction in sorted(rows):
    print(f"{corp},{faction}")
PY
