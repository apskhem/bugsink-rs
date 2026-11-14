#!/usr/bin/env bash
# Compares issue and event search (tags/search.py) between upstream Bugsink (official image) and this port: the digest
# steps are replayed into a database for realistic tags, then upstream builds a query corpus from them (plus parser
# edge cases) and both sides answer every query on that same database. Needs docker.
# Usage: tools/parity/search.sh [image]  (default: bugsink/bugsink:2.6.3)
set -euo pipefail
cd "$(dirname "$0")/../.."
image="${1:-bugsink/bugsink:2.6.3}"
out="$(mktemp -d)"
net=bsrs-parity pg=bsrs-parity-search-pg port=55497

docker network create "$net" >/dev/null 2>&1 || true
docker rm -f "$pg" >/dev/null 2>&1 || true
docker run -d --rm --name "$pg" --network "$net" -p "$port:5432" -e POSTGRES_PASSWORD=pw postgres:17 >/dev/null
trap 'docker rm -f "$pg" >/dev/null' EXIT
until docker exec "$pg" pg_isready -q -U postgres; do sleep 1; done
docker exec "$pg" createdb -U postgres search
export DATABASE_URL="postgres://postgres:pw@localhost:$port/search"

python3 tools/parity/make_digest_steps.py >/dev/null
cargo build -q -p bugsink-server --example replay --example search_dump
# No messaging services: alerts don't matter here, and there is no capture server to send them to.
grep -v alerts_messagingserviceconfig tools/parity/digest-steps.jsonl | ./target/debug/examples/replay >/dev/null

docker run --rm -i --network "$net" -e SECRET_KEY=parity -e "DATABASE_URL=postgres://postgres:pw@$pg:5432/search" \
    "$image" bugsink-manage shell -c "$(cat tools/parity/upstream_search.py)" | grep '^{' > "$out/upstream.jsonl"
python3 -c 'import json, sys
for line in sys.stdin: print(json.dumps(json.loads(line)["input"]))' < "$out/upstream.jsonl" \
    | ./target/debug/examples/search_dump > "$out/ours.jsonl"

python3 - "$out/upstream.jsonl" "$out/ours.jsonl" <<'EOF'
import json, sys
upstream = [json.loads(line) for line in open(sys.argv[1])]
ours = [json.loads(line) for line in open(sys.argv[2])]
assert len(upstream) == len(ours), (len(upstream), len(ours))
kinds, diffs = {}, 0
for u, o in zip(upstream, ours):
    kind = u["input"]["kind"]
    kinds[kind] = kinds.get(kind, 0) + 1
    if u != o:
        diffs += 1
        if diffs <= 20:
            print("DIFF", json.dumps(u["input"], ensure_ascii=False))
            print("  upstream:", json.dumps(u["result"], ensure_ascii=False)[:300])
            print("  ours:    ", json.dumps(o["result"], ensure_ascii=False)[:300])
nonempty = sum(1 for u in upstream if isinstance(u["result"], list) and u["result"])
print(f"compared: {kinds}; with matches: {nonempty}; differences: {diffs}")
sys.exit(1 if diffs else 0)
EOF
