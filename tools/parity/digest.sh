#!/usr/bin/env bash
# Drop-in check at the database level. Replays the same digest steps four ways and diffs the resulting databases:
#   A: upstream only   B: bugsink-rs only
#   C: bugsink-rs bootstraps the database and digests the first half, upstream migrates it and digests the rest
#   D: upstream creates the database and digests the first half, bugsink-rs digests the rest
# Needs docker; starts its own Postgres. Usage: tools/parity/digest.sh [image]  (default: bugsink/bugsink:2.6.3)
set -euo pipefail
cd "$(dirname "$0")/../.."
image="${1:-bugsink/bugsink:2.6.3}"
out="$(mktemp -d)"
net=bsrs-parity pg=bsrs-parity-pg port=55499

docker network create "$net" >/dev/null 2>&1 || true
docker rm -f "$pg" >/dev/null 2>&1 || true
docker run -d --rm --name "$pg" --network "$net" -p "$port:5432" -e POSTGRES_PASSWORD=pw postgres:17 >/dev/null
trap 'docker rm -f "$pg" >/dev/null' EXIT
until docker exec "$pg" pg_isready -q -U postgres; do sleep 1; done
for db in a b c d; do docker exec "$pg" createdb -U postgres "$db"; done

python3 tools/parity/make_digest_steps.py >/dev/null
steps=tools/parity/digest-steps.jsonl
half=$(( $(wc -l < "$steps") / 2 ))
cargo build -q -p bugsink-server --example replay

upstream() { # db, extra docker args...; steps on stdin
    local db="$1"; shift
    docker run --rm -i --network "$net" -e SECRET_KEY=parity -e "DATABASE_URL=postgres://postgres:pw@$pg:5432/$db" \
        "$image" "$@"
}
upstream_migrate() { upstream "$1" bugsink-manage migrate --no-input | grep -E "No migrations|Applying" | tail -1; }
upstream_digest() { upstream "$1" bugsink-manage shell -c "$(cat tools/parity/upstream_digest.py)" | grep -v "objects imported" || true; }
ours() { DATABASE_URL="postgres://postgres:pw@localhost:$port/$1" ./target/debug/examples/replay; }

echo "A: upstream";                  upstream_migrate a; upstream_digest a < "$steps"
echo "B: bugsink-rs";                ours b < "$steps"
echo "C: bugsink-rs, then upstream"; head -n "$half" "$steps" | ours c; upstream_migrate c; tail -n "+$((half + 1))" "$steps" | upstream_digest c
echo "D: upstream, then bugsink-rs"; upstream_migrate d; head -n "$half" "$steps" | upstream_digest d; tail -n "+$((half + 1))" "$steps" | ours d

for db in a b c d; do
    docker exec -i "$pg" psql -U postgres -d "$db" -At -F ' | ' -f - < tools/parity/normalize.sql > "$out/$db.txt"
done
echo "rows compared: $(wc -l < "$out/a.txt")"
status=0
for db in b c d; do
    if diff "$out/a.txt" "$out/$db.txt" > "$out/a-$db.diff"; then echo "A == $db"; else echo "A != $db:"; head -40 "$out/a-$db.diff"; status=1; fi
done
exit $status
