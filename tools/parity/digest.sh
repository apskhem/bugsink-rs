#!/usr/bin/env bash
# Drop-in check at the database level. Replays the same digest steps four ways and diffs the resulting databases:
#   A: upstream only   B: bugsink-rs only
#   C: bugsink-rs bootstraps the database and digests the first half, upstream migrates it and digests the rest
#   D: upstream creates the database and digests the first half, bugsink-rs digests the rest
# Done once per settings configuration (see `configs`). Needs docker; starts its own Postgres.
# Usage: tools/parity/digest.sh [image]  (default: bugsink/bugsink:2.6.3)
set -euo pipefail
cd "$(dirname "$0")/../.."
image="${1:-bugsink/bugsink:2.6.3}"
out="$(mktemp -d)"
net=bsrs-parity pg=bsrs-parity-pg port=55499

# Each configuration is a set of upstream settings (environment variables), given to both sides.
configs=(
    "default:"
    # 5-minute quota trips on every event, the hourly one regularly, the monthly one once; installation-wide hourly too
    "quotas:MAX_EVENTS_PER_PROJECT_PER_5_MINUTES=1 MAX_EVENTS_PER_PROJECT_PER_HOUR=5 MAX_EVENTS_PER_PROJECT_PER_MONTH=60 MAX_EVENTS_PER_HOUR=7"
)

docker network create "$net" >/dev/null 2>&1 || true
docker rm -f "$pg" >/dev/null 2>&1 || true
docker run -d --rm --name "$pg" --network "$net" -p "$port:5432" -e POSTGRES_PASSWORD=pw postgres:17 >/dev/null
trap 'docker rm -f "$pg" >/dev/null' EXIT
until docker exec "$pg" pg_isready -q -U postgres; do sleep 1; done

python3 tools/parity/make_digest_steps.py >/dev/null
steps=tools/parity/digest-steps.jsonl
half=$(( $(wc -l < "$steps") / 2 ))
cargo build -q -p bugsink-server --example replay

settings=()
upstream() { # db, extra docker args...; steps on stdin
    local db="$1"; shift
    local envs=()
    for kv in ${settings[@]+"${settings[@]}"}; do envs+=(-e "$kv"); done
    docker run --rm -i --network "$net" -e SECRET_KEY=parity -e "DATABASE_URL=postgres://postgres:pw@$pg:5432/$db" \
        ${envs[@]+"${envs[@]}"} "$image" "$@"
}
upstream_migrate() { upstream "$1" bugsink-manage migrate --no-input | grep -E "No migrations|Applying" | tail -1; }
upstream_digest() { upstream "$1" bugsink-manage shell -c "$(cat tools/parity/upstream_digest.py)" | grep -v "objects imported" || true; }
ours() { env ${settings[@]+"${settings[@]}"} DATABASE_URL="postgres://postgres:pw@localhost:$port/$1" ./target/debug/examples/replay; }

status=0
for config in "${configs[@]}"; do
    name="${config%%:*}"
    read -r -a settings <<< "${config#*:}"
    echo "=== $name ${settings[*]+${settings[*]}}"
    for db in a b c d; do docker exec "$pg" dropdb -U postgres --if-exists "$db" 2>/dev/null; docker exec "$pg" createdb -U postgres "$db"; done

    echo "A: upstream";                  upstream_migrate a; upstream_digest a < "$steps"
    echo "B: bugsink-rs";                ours b < "$steps"
    echo "C: bugsink-rs, then upstream"; head -n "$half" "$steps" | ours c; upstream_migrate c; tail -n "+$((half + 1))" "$steps" | upstream_digest c
    echo "D: upstream, then bugsink-rs"; upstream_migrate d; head -n "$half" "$steps" | upstream_digest d; tail -n "+$((half + 1))" "$steps" | ours d

    for db in a b c d; do
        docker exec -i "$pg" psql -U postgres -d "$db" -At -F ' | ' -f - < tools/parity/normalize.sql > "$out/$name-$db.txt"
    done
    echo "rows compared: $(wc -l < "$out/$name-a.txt")"
    for db in b c d; do
        if diff "$out/$name-a.txt" "$out/$name-$db.txt" > "$out/$name-a-$db.diff"; then echo "A == $db"
        else echo "A != $db:"; head -40 "$out/$name-a-$db.diff"; status=1; fi
    done
done
echo "outputs in $out"
exit $status
